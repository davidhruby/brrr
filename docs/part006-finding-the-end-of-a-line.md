# Part 006

**Dates:** 2026-11-02 – 2026-11-05
**Video progress:** stopped at [2:35:32](https://www.youtube.com/watch?v=tCY7p6dVAGE&t=9332s) when this part was published
**Code:** git tag [`part006`](https://github.com/davidhruby/brrr/tree/part006). To get the code exactly as it was for this part: `git checkout part006`

New here? Start with the [Introduction](intro.md).

## What changed

### 1. `memchr` instead of `split` for finding the end of a line

The lines aren't split with `split` any more, which calls a closure for every byte. The loop
keeps a position `at` in the mapped file and asks the C library's `memchr` for the next `\n`
after it. `memchr` returns a pointer to the `\n` it found (or null when there's none left);
the distance from the start of the rest gives the length of the line, and `at` moves past the
`\n`.

Before:

```rust
for line in map.split(|c| *c == b'\n') {
    if line.is_empty() {
        break;
    }
```

After:

```rust
let mut at = 0;
loop {
    let rest = &map[at..];
    let next_new_line = unsafe { libc::memchr(rest.as_ptr() as *const c_void, b'\n' as c_int, rest.len() as size_t) };
    let line = if next_new_line.is_null() {
        break;
    } else {
        let line_len = unsafe { (next_new_line as *const u8).offset_from_unsigned(rest.as_ptr()) };
        // line_len is count wihthout \n therefore +1 in the end
        at += line_len + 1;
        &rest[..line_len]
    };
```

`libc::memchr` is the C function `void *memchr(const void *s, int c, size_t n)`: it looks for
the byte `c` in the first `n` bytes from `s`. It returns a `*mut c_void`, a pointer of no
particular type, so it has to be cast to `*const u8` before `offset_from_unsigned` can count
the bytes between it and the start of `rest`. The `mut` is a leftover of the C signature;
the pointer points into the read-only mapping and must never be written through.

`memchr` itself already uses SIMD. On macOS it's `_platform_memchr` in
`libsystem_platform`, written by hand with NEON: it compares 16 bytes at once (see change #2
for its instructions). glibc on Linux has versions of it for NEON, SVE and AVX2, and the Rust
crate `memchr` uses SIMD too, with SWAR where there's no SIMD. So the question isn't whether
`memchr` uses SIMD, but how much of each call is left for it: here it's called once for
every ~14-byte line.

Measurements after this change (the raw output wasn't kept: change #2 was measured into the
same folder and overwrote it):

| | `david` 100M | `david` 1B |
|---|---|---|
| Wall time | 3.0 s | 23.9 s |
| CPU time (user + sys, both threads) | 2.4 s | 25.4 s |
| Peak memory footprint | 1.9 MB | 8.0 MB |
| Swapped to the SSD | 0 | 0 |
| Instructions per row | ~363 | ~372 |
| CPU cycles | 9.6 billion | 100.3 billion |
| Page faults | 84,203 | 842,025 |

**1B: 24.7 s → 23.9 s (−3 %).** The output is identical to Part 005 on both files.

About 32 instructions fewer per row (1B ~404 → ~372, −8 %), but only −3.3 % of the cycles
(1B 103.7 → 100.3 billion, 100M 9.9 → 9.6 billion). On 100M the program still waits: 3.0 s
of wall time, but only 2.4 s of CPU time.

### 2. Finding the `\n` 8 bytes at a time (SWAR)

**Why `memchr` helped so little.** A benchmark of finding the lines alone (see below) gave
`memchr` only ~16 % over `split`. In Instruments, a double click on `_platform_memchr` in the
Time Profiler shows its instructions. It does search with NEON (SIMD), 16 bytes at a time:

```text
dup.16b   v1, w1          '\n' copied into all 16 lanes
and       x0, x0, #…fff0  align the pointer down to 16 bytes
ldr       q0, [x0]        load 16 bytes at once
sub.16b   v0, v0, v1      every byte minus '\n': a '\n' becomes 0
ldr       q3, [x3] …      a mask from a table, to ignore the bytes before the start
uminv.16b b2, v0          the minimum of the 16 bytes: 0 if there was a '\n'
cbz       w3, …           found → find the index of the first one (cmhi, orr, uminv, add)
```

But a line is only ~14 bytes, so the `\n` is almost always in the first 16 bytes: the loop
over the blocks of 16 bytes hardly ever runs (4 ms of samples), and every call pays mostly
for the setup (the call into the system library through a stub, aligning, the mask from the
table) and for finding the index (1.63 s of samples right at the start of the "found" path).
SIMD pays off on long strings, not on many short ones.

**SWAR** (*SIMD within a register*) does the same in a normal 64-bit register: 8 bytes are
loaded as one `u64`, and a bit trick tests all 8 of them for `\n` at once. It's a few
instructions the compiler puts right into the loop, with no call and no setup:

```rust
// SWAR: tests 8 bytes at once for '\n'
fn find_new_line(bytes: &[u8]) -> Option<usize> {
    const NEW_LINES: u64 = u64::from_ne_bytes([b'\n'; 8]);
    const ONES: u64 = 0x0101_0101_0101_0101;
    const HIGH_BITS: u64 = 0x8080_8080_8080_8080;

    let mut i = 0;
    while i + 8 <= bytes.len() {
        // first byte in memory is the lowest byte of the number
        let chunk = u64::from_le_bytes(bytes[i..i + 8].try_into().unwrap());
        // bytes that were '\n' are 0 now
        let x = chunk ^ NEW_LINES;
        // highest bit set in every byte that is 0
        let mask = x.wrapping_sub(ONES) & !x & HIGH_BITS;
        if mask != 0 {
            // the lowest set bit is the first '\n'
            return Some(i + (mask.trailing_zeros() / 8) as usize);
        }
        i += 8;
    }
    // fewer than 8 bytes left, only at the end of the file
    bytes[i..].iter().position(|&b| b == b'\n').map(|position| i + position)
}

pub fn next_line_swar<'a>(map: &'a [u8], at: &mut usize) -> Option<&'a [u8]> {
    let rest = &map[*at..];
    let line_len = find_new_line(rest)?;
    // line_len is count without \n therefore +1 in the end
    *at += line_len + 1;
    Some(&rest[..line_len])
}
```

How it works, on 3 bytes `A`, `\n`, `1` instead of 8 (the constants are just shorter:
`0x010101`, `0x808080`):

1. **Load as a number.** `from_le_bytes`: the first byte in memory is the lowest byte of the
   number, so the bytes read from right to left: `0x31_0A_41`.
2. **XOR with `\n` in every byte.** Equal bits give 0, so a byte that was `\n` becomes 0, and
   any other byte stays non-zero: `0x3B_00_4B`. The question "is it `\n`?" is now "is it 0?".
3. **Find the zero byte.** For one byte `b`, `(b − 1) & !b & 0x80` has its highest bit set
   only when `b` is 0: zero is the only value whose highest bit becomes 1 by subtracting 1
   (`0000_0000 − 1 = 1111_1111`), and `!b` removes the values that had it set already
   (128 – 255). With the constants repeated, one subtraction, one negation and two `&` do it
   for all bytes at once:

   ```text
   x            = 0011_1011  0000_0000  0100_1011
   x − 0x010101 = 0011_1001  1111_1111  0100_1010   (the zero byte borrows from the one above)
   !x           = 1100_0100  1111_1111  1011_0100
   & & 0x808080 = 0000_0000  1000_0000  0000_0000
   ```

4. **The position.** `trailing_zeros` counts the zero bits from the bottom: 15, and 15 / 8 = 1,
   the `\n` at position 1.

The borrow in step 3 can set a wrong bit, but only in a byte above the real zero byte, so
the lowest set bit, the first `\n`, is always right. A line longer than 8 bytes just takes
another step of the loop; with ~14 bytes per line that's usually 2.

**A benchmark of the three ways to find the lines**, in
[`benches/parse_newline_criterion.rs`](../benches/parse_newline_criterion.rs): the whole
1M file read into memory once (`std::fs::read`), and every iteration goes through all its
~1 million lines. Not the 1B file: an iteration would take seconds and would mostly measure
the SSD and the page faults, not finding the lines.

| 1M file, ~1 million lines | Time | Per line |
|---|---|---|
| `split` | 9.43 ms | 9.4 ns |
| `memchr` (change #1) | 7.93 ms | 7.9 ns |
| SWAR | **4.91 ms** | **4.9 ns** |

Measurements after this change (raw output in
[`measurements/part006/`](../measurements/part006/)):

| | `david` 100M | `david` 1B |
|---|---|---|
| Wall time | 2.7 s | 21.0 s |
| CPU time (user + sys, both threads) | 2.2 s | 22.2 s |
| Peak memory footprint | 1.9 MB | 8.0 MB |
| Swapped to the SSD | 0 | 0 |
| Instructions per row | ~348 | ~356 |
| CPU cycles | 8.7 billion | 88.2 billion |
| Page faults | 84,203 | 842,026 |

**1B: 23.9 s → 21.0 s (−12 %), and from Part 005 (24.7 s) −15 %.** The output is identical to
Part 005 on both files.

- **The instructions went down only 4 % (1B ~372 → ~356 per row), the cycles 12 %** (100.3 →
  88.2 billion), and the instructions per cycle went up from 3.71 to 4.04. The same pattern
  as with the parser without branches: not much less work, but the CPU doesn't stall on it,
  with no call into the library and no waiting for the mask from the table.
- **This time the benchmark predicted the program well.** ~3 ns saved per line × 1 billion
  rows ≈ 3 s; the program saved 2.9 s of wall time and 3.1 s of CPU time. Finding the lines
  is a much bigger part of the program (~18 % of the main thread in the profile) than the
  parser was (~7.5 %).
- On 100M the program still waits: 2.7 s of wall time, but only 2.2 s of CPU time.

### 3. An attempt: finding the `;` with `u8x64` (slower, not used)

Jon uses `u8x64` from `std::simd` (*portable SIMD*): a vector of 64 bytes that is compared as
one value. It's only in nightly Rust, so the repository now has a `rust-toolchain.toml` with
`channel = "nightly"`, and `src/david/lib.rs` starts with `#![feature(portable_simd)]`. I
tried it for splitting a line at the `;`, in `lib.rs` only, next to the current way in
`main.rs`:

```rust
// main.rs, what the program uses
pub fn split_semicolon(line: &[u8]) -> (&[u8], &[u8]) {
    let mut fields = line.rsplitn(2, |c| *c == b';');
    let (Some(temperature), Some(station)) = (fields.next(), fields.next()) else {
        panic!("unexpected line: {:?}", String::from_utf8_lossy(line));
    };
    (station, temperature)
}
```

```rust
// lib.rs, the attempt
pub fn split_semicolon_simd(line: &[u8]) -> (&[u8], &[u8]) {
    const DELIMITER: u8x64 = u8x64::splat(b';');
    if line.len() > 64 {
        // fallback to normal not optimized approach
        line.rsplit_once(|c| *c == b';').unwrap()
    } else {

        let del_eq = DELIMITER.simd_eq(u8x64::load_or_default(line));
        // SAFETY: we are promised there is a ; in every line
        let index_of_delim = unsafe { del_eq.first_set().unwrap_unchecked() };
        (&line[..index_of_delim], &line[index_of_delim + 1..])
    }
}
```

A benchmark in
[`benches/parse_semicolon_criterion.rs`](../benches/parse_semicolon_criterion.rs): the lines of
the 1M file split in advance, then every iteration splits all ~1 million of them at the `;`:

| 1M file, ~1 million lines | Time | Per line |
|---|---|---|
| `rsplitn` | 2.68 ms | **2.7 ns** |
| `u8x64` | 17.01 ms | **17.0 ns** |

**The SIMD version is ~6× slower.** I haven't measured why yet, but from the code, my guesses
are:

- **`load_or_default` copies the line.** A line is ~14 bytes, but a `u8x64` needs 64. So
  `load_or_default` makes a 64-byte buffer of zeros, copies the line into it, and only then
  loads it into the registers. That's a copy of a variable length on every line, and a read
  of the same memory right after small writes to it: the CPU can't hand the result of several
  small stores directly to one big load (*store-to-load forwarding*), so it waits until the
  stores are done.
- **Turning the comparison into a position costs more on ARM.** The result of `simd_eq` is 64
  lane masks in four 128-bit NEON registers. `first_set()` has to turn them into a bit mask
  and find the first bit, and ARM has no single instruction for that (like `movemask` on
  x86), so it takes several. For one line, that doesn't pay off.
- **`rsplitn` is hard to beat here.** It searches from the end, and after the `;` there are
  only the 3 – 5 bytes of the temperature, so it looks at 4 – 6 bytes and stops. The SIMD
  version handles 64 bytes at once, but the setup around it costs more than the whole
  `rsplitn`.

The same lesson as with `memchr` in change #1: SIMD pays off on a lot of data at once, not on
one short line at a time. What could work instead: load directly from the mapped file (the
line goes on with the next lines, so 64 bytes can be read without copying, with a fallback
only at the end of the file), and find all the `;` and `\n` of ~4 – 5 lines in one 64-byte
block.

> **Note:** with `rust-toolchain.toml`, everything is now built with nightly Rust, not with
> stable 1.97.1 as all the measurements so far. How much that changes on its own is in
> change #4 (~1 %).

### 4. Finding the `;` and the `\n` of several lines at once (SIMD iterator)

Change #3 loaded 64 bytes for every line and threw most of them away. A line is ~14 bytes,
so one 64-byte block holds 4 – 5 of them. The iterator loads a block once, compares it with
`;` and with `\n`, and keeps both results as bit masks (`to_bitmask()`: bit *i* set when
byte *i* matches). Every call of `next()` takes the lowest bit of both masks, which are the
`;` and the `\n` of the next line, returns the station and the temperature, and clears the
two bits (`mask &= mask - 1`). Only when the `\n` mask is empty, it loads the next block,
starting exactly at the beginning of the next line, so no line is split between two blocks.
It replaces both `next_line_swar` and `rsplitn` in the loop:

```rust
for (station, temperature) in StationTemperatureIterator::new(map) {
```

```rust
pub struct StationTemperatureIterator<'a>{
    map: &'a [u8],
    at: usize,
    block_start: usize,
    delimiter_bit_mask: u64,
    newline_bit_mask: u64,
}

impl <'a> StationTemperatureIterator<'a> {
    pub fn new(map: &'a [u8]) -> StationTemperatureIterator<'a> {
        Self {
            map,
            at: 0,
            block_start: 0,
            delimiter_bit_mask: 0,
            newline_bit_mask: 0,
        }
    }
}

//TODO this code works with full pairs in the SIMD only so it can be optimized
// for example delimiter in first simd and newline in another simd
impl <'a> Iterator for StationTemperatureIterator<'a> {
    type Item = (&'a [u8], &'a [u8]);

    fn next(&mut self) -> Option<Self::Item> {
        const SIZE_64: usize = 64;
        const DELIMITER: u8x64 = u8x64::splat(b';');
        const END_LINE: u8x64 = u8x64::splat(b'\n');


        if self.newline_bit_mask == 0 {
            //file ends with empty line
            if self.at == self.map.len() {
                return None;
            }
            self.block_start = self.at;
            let simd_value = if self.at + SIZE_64 <= self.map.len() {
                u8x64::from_slice(&self.map[self.at..self.at + SIZE_64])
            } else {
                u8x64::load_or_default(&self.map[self.at..self.map.len()])
            };
            let del_eq = DELIMITER.simd_eq(simd_value);
            self.delimiter_bit_mask = del_eq.to_bitmask();

            let end_line_eq = END_LINE.simd_eq(simd_value);
            self.newline_bit_mask = end_line_eq.to_bitmask();
        }
        let delimiter_index = self.delimiter_bit_mask.trailing_zeros() as usize;
        self.delimiter_bit_mask &= self.delimiter_bit_mask - 1;
        let newline_index = self.newline_bit_mask.trailing_zeros() as usize;
        self.newline_bit_mask &= self.newline_bit_mask - 1;
        let station = &self.map[self.at..self.block_start + delimiter_index];
        let temperature = &self.map[self.block_start + delimiter_index + 1..self.block_start + newline_index];
        self.at = self.block_start + newline_index + 1;
        Some((station, temperature))
    }
}
```

- **The block always starts at the beginning of a line**, so the *i*-th `;` in the mask
  belongs to the *i*-th `\n`. The `;` of the last line of a block, which is cut off, may stay
  in the mask, but it's thrown away with it when the next block is loaded.
- **The positions are relative to `block_start`**, the beginning of the block, while `at` is
  the beginning of the current line. My first versions mixed them up: `at +=` instead of
  `at = block_start + …` worked only for the first line of a block.
- **The end of the file** (fewer than 64 bytes left) is loaded with `load_or_default`, which
  fills the rest of the block with zeros; that happens once per file.
- **Not handled yet: a line longer than 64 bytes.** Then the block has no `\n`, and
  `trailing_zeros` gives 64 and `mask - 1` overflows. The generated files have no such lines
  (the longest is ~33 bytes), but the rules allow station names of up to 100 bytes.

On the way there I benchmarked the steps in
[`benches/parse_newline_and_semicolon_criterion.rs`](../benches/parse_newline_and_semicolon_criterion.rs),
all the ~1 million lines of the 1M file, split into station and temperature:

| 1M file, per line (nightly) | Time |
|---|---|
| SWAR for `\n` + `rsplitn` for `;` (change #2) | 6.03 ns |
| One block per line, `u8x16` | 5.95 ns |
| One block per line, `u8x32` | 5.15 ns |
| One block per line, `u8x64` | 6.11 ns |
| `u8x64`, up to 3 lines per block (a first version returning a struct of 3 lines) | 3.01 ns |
| **The iterator, all lines of the block** | **2.85 ns** |

With one block per line, `u8x32` is the best: most lines fit into 32 bytes, and it's half the
work of `u8x64` (2 NEON registers instead of 4, and a smaller mask to turn into positions).
`u8x16` is less work again, but many lines don't fit into 16 bytes and go to the slower way.
The big step comes from reusing the block for several lines: the load and the conversion of
the masks are paid once for 4 – 5 lines, and every further line costs only a
`trailing_zeros` and a `mask &= mask - 1`. The same with the 100M file gave the same time per
line (5.99 ns and 6.23 ns for the first two), so the size of the data doesn't matter here:
it's limited by computing, not by memory.

**Nightly vs stable.** Since change #3 everything is built with nightly Rust. To separate the
compiler from the code, I measured the code of Part 005 again with nightly: 1B 24.47 s and
24.54 s instead of 24.70 s with stable, 100M 2.94 s instead of 3.06 s. The compiler alone
makes ~1 %.

Measurements after this change (nightly, raw output in
[`measurements/part006/`](../measurements/part006/)):

| | `david` 100M | `david` 1B |
|---|---|---|
| Wall time | 2.3 s | 18.5 s |
| CPU time (user + sys, both threads) | 1.9 s | 19.8 s |
| Instructions per row | ~323 | ~331 |
| CPU cycles | 7.7 billion | 78.3 billion |

**1B: 21.0 s (change #2) → 18.5 s (−12 %), and from Part 005 (24.7 s) −25 %.** The output is
identical to Part 005 on both files. A second run gave 18.75 s.

The instructions went down 7 % (~356 → ~331 per row), the cycles 11 % (88.2 → 78.3 billion).
On 100M the program still waits: 2.3 s of wall time, but only 1.9 s of CPU time.

## What I learned

### 1. SIMD pays off only when it's used the right way

The same idea, comparing many bytes with one instruction, came out anywhere from 6× slower to
2× faster than the plain code, depending only on how it was used. Per line of the 1M file:

| | Per line |
|---|---|
| `u8x64` for the `;`, with the line copied into the vector (`load_or_default`) | 17.0 ns |
| `split` for `\n` + `rsplitn` for `;` (no SIMD) | ~12 ns (9.4 + 2.7, measured separately) |
| `memchr` for `\n` (SIMD in the C library) | 7.9 ns, the `\n` alone |
| SWAR for `\n` + `rsplitn` for `;` | 6.0 ns |
| `u8x64` for `;` and `\n`, one block per line | 6.1 ns |
| `u8x32` for `;` and `\n`, one block per line | 5.2 ns |
| **`u8x64` for `;` and `\n`, one block for all its 4 – 5 lines (the iterator)** | **2.85 ns** |

What made the difference:

- **A fixed cost has to be shared.** Every use of SIMD costs something before the useful
  work: loading the vector, and on ARM turning the comparison back into a position (there's
  no `movemask` like on x86, so it takes several instructions). `memchr` adds a call into the
  system library and setting up on every call. For one ~14-byte line, that's more than the
  whole search. Spread over the 4 – 5 lines of a block, it almost disappears.
- **No copying into the vector.** `load_or_default` copied every short line into a zeroed
  64-byte buffer first. Loading straight from the mapped file, where the line just goes on
  with the next lines, removed that.
- **The width has to fit the data.** With one block per line, `u8x32` beat both `u8x16`
  (too many lines don't fit and take the slow way) and `u8x64` (twice the work for bytes
  that aren't used).
- **One load, several questions.** Once 64 bytes are in the registers, comparing them with
  `;` as well as with `\n` costs only a few instructions more, and it replaced the whole
  second search (`rsplitn`).
- **Bit masks are the bridge back to normal code.** `to_bitmask()` turns the comparison into a
  `u64` with one bit per byte; `trailing_zeros()` finds the first match, and
  `mask &= mask - 1` removes it, so the next one comes out the same way.

And it carried over to the whole program: the benchmark of the iterator promised ~3.2 ns less
per line than SWAR + `rsplitn` (~3 s on 1 billion rows), and the whole program on 1B got
from 21.0 s to 18.5 s.

## Next steps

## TODO

Things I want to try later. I'm not doing them yet, because Jon may get to them in a later
part of the stream, and I don't want to get ahead of him.

- **Handle lines longer than 64 bytes in the SIMD iterator** (change #4): when a freshly
  loaded block has no `\n`, fall back to `next_line_swar` + `split_semicolon` for that line,
  and add a test with such a line. The rules allow station names of up to 100 bytes.
- **Try [samply](https://github.com/mstange/samply)**, a sampling profiler written in Rust
  that opens the profile in the Firefox Profiler (call tree, inverted call tree, flame graph).
  It demangles Rust's v0 symbol names itself, so it would work without the nightly build that
  Instruments needs for readable names.
- **Try [hotpath](https://hotpath.rs/)**, a Rust profiler used from the code: functions are
  marked with `#[hotpath::measure]` (and `main` with `#[hotpath::main]`), and it reports their
  time and allocations, and whether they're CPU- or I/O-bound.
- **A hasher without branches on the length of the key.** ~62 % of the branch mispredictions
  come from SipHash handling the rest of the station name (length % 8) with branches (see
  [Part 005, change #3](part005-parsing-temperature.md#3-parsing-without-branches)). A hasher that treats every name the same way, whatever its length, would
  remove most of them.
- **Find the `;` without a byte-by-byte loop.** The end of the search for the `;` is the
  second source of mispredictions (~0.25 – 0.3 per row). The temperature is only 3 – 5
  bytes, so its position could be found with a fixed number of steps instead of a loop.
- **Find out what the program waits for on 100M** since change #1 (~0.6 – 0.8 s of wall time
  without CPU time, see [Part 005](part005-parsing-temperature.md#3-parsing-without-branches) and change #1 below), for example with the thread states in a System Trace. One
  guess: the main thread is faster now and catches up with the helper thread, which starts
  the next chunk only half a chunk ahead.
- **Check the waiting in Instruments.** The `thread-state` table of a System Trace shows how
  long the thread was running and how long it was blocked, which would confirm the waiting
  directly instead of from `real` − `user` − `sys`.
- **Write the output through a `BufWriter`** instead of calling `println!` for every
  station. In the System Trace of the 100M run, my version made **413 `write` system calls**,
  one per line of output, because `println!` writes each line out on its own. A `BufWriter`
  collects the lines in a buffer and writes them in a few big calls. With 413 lines it
  doesn't matter yet, but with a bigger output (up to 10,000 stations by the rules) or a
  faster program it will.
- **Find out why the `HashMap` version runs more instructions per cycle** (5.0 instead of 4.3,
  see [Part 003, change #2](part003-hashmap-bytes-and-lto.md#2-hashmap-instead-of-btreemap-sorted-only-for-the-output)).
  My guess: the linear search in a B-tree node ends at a different key for every station,
  which is hard for the CPU to predict, while the `HashMap` lookup mostly goes the same way
  every time. Measure the branch mispredictions of both versions, and compare the `BTreeMap`
  used for sorting the output with another way of sorting the stations.
