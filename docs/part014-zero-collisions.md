# Part 014 — The hot loop: no collisions, inlining, SWAR, a huge page, `znver5` and a key of 32 bytes

**Dates:** 2026-11-18 – 2026-11-25
**Video progress:** the end of the stream, [10:33:47](https://www.youtube.com/watch?v=tCY7p6dVAGE&t=38027s), when this part was published
**Code:** git tag [`part014`](https://github.com/davidhruby/brrr/tree/part014). To get the code exactly as it was for this part: `git checkout part014`

New here? Start with the [Introduction](intro.md).

## What changed

At the end of [Part 013](part013-own-hash-table.md) the table had 16,384 slots for 413 stations,
and the probing was no longer most of what's left over the floor. But it wasn't zero either. This
part counts the collisions of the hash on the names of the data, and then looks for a hash that
has none. Then it goes through the rest of the hot loop: the call of `next`, the parser of the
temperature, the search for the end of the line, and where a row waits, which turned out to be
the translation of addresses of the table.

Measured the same way as in [Part 013](part013-own-hash-table.md#what-changed): the 1B file in
`tmpfs` with 2 MB pages, `perf stat` on one thread divided by 10⁹ to get the numbers per row, the
wall time of 8 threads from `time`, and the output compared with `cmp` against the output of the
hashbrown version of Part 012.

The starting point is the end of Part 013: **28.41 cycles, 151.92 instructions, 21.68 branches
and 0.108 mispredicted branches per row**, 6.35 s on one thread and 0.809 s on 8.

### 1. How many rows probe

A new binary, `analyze-collision-hashtable`
([`src/analyze/collision_hashtable/main.rs`](../src/analyze/collision_hashtable/main.rs)), runs
the table over the 1B file with a copy of `get_mut` that also returns how many extra slots it
probed. To use the table outside of the `david` binary, `src/david/lib.rs` makes `main.rs` a
module of the library (`#[path = "main.rs"] pub mod app;`), and the pieces it needs are `pub`.

With the hash of Part 013 (rotation 29, the constant `0x243F_6A88_85A3_08D3`), three stations
didn't land in their own slot:

| Station | Rows that probed |
|---|---|
| Denver | 2,423,860 |
| Charlotte | 2,421,726 |
| Winnipeg | 2,419,554 |
| **Extra slots probed** | **7,265,140** |

The sum of the rows is the number of extra slots, so each of them probed exactly one slot more.
That's **0.73 % of the rows**, 0.0073 extra probes per row: at most ~0.007 mispredictions and,
at ~35 cycles each ([Part 013, change #6](part013-own-hash-table.md#6-the-number-of-slots)), at
most ~0.3 – 0.5 cycles per row.

Three is what a random hash gives here. The expected number of stations that share a slot with
another is ≈ n² / 2m = 413² / (2 × 16,384) ≈ 5.2, and the chance of none at all is
e^(−5.2) ≈ **0.55 %**. One hash in ~180 has no collision on these names, so the way to find one
is to try many.

### 2. The rotation doesn't help

The hash is a rotation of the second word, a multiplication and the top 14 bits:

```rust
pub fn hash(word_1: u64, word_2: u64) -> usize {
    let words = word_1 ^ word_2.rotate_left(29u32);
    let multiplied_by_magic_constant = words.wrapping_mul(0x243F_6A88_85A3_08D3 as u64);
    (multiplied_by_magic_constant as usize) >> (64 - SLOT_BITS)
}
```

The tool hashes the 413 names of `input/1B_unique_station_names.txt` and counts the indices that
are already taken, the first try with every rotation from 0 to 63 and the same constant. The
fewest was **1 collision** (rotations 6, 13, 26 and 50), the most 9, and none had zero.

The reason is in the names: **272 of the 413 have 8 bytes or fewer**, so their `word_2` is 0, and
`0.rotate_left(r)` is 0 for every `r`. The hash of most names doesn't depend on the rotation at
all. Between 272 names the expected number of collisions is 272² / (2 × 16,384) ≈ 2.3, and they
are the same for all 64 rotations. That one collision that no rotation removed is most likely two
of the short names (I haven't printed which ones).

The constant is different: the multiplication mixes every bit of `words`, so another constant
moves the short names too.

### 3. Searching for a constant

The candidates come from **splitmix64**, a small generator of pseudo-random numbers: a counter
that grows by `0x9E37_79B9_7F4A_7C15` (2⁶⁴ / φ, the same number as in Fibonacci hashing) and two
rounds of xor-shift and multiplication that mix it, so that consecutive candidates have nothing
in common. `| 1` makes each of them odd, because a multiplication by an odd number loses no bits
of a `u64`.

```rust
fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}
```

The keys of the 413 names are computed once into a `Vec<(u64, u64)>`, and for every candidate
the tool tries ten rotations with a copy of the hash that takes the rotation and the constant as
arguments:

```rust
let mut state: u64 = 0;
for _ in 0..10_000 {
    let multiplier = splitmix64(&mut state) | 1;
    for rotate in top_ten_rotate.iter() {
        // count the collisions of all 413 keys with hash_analyse(word_1, word_2, *rotate, multiplier)
    }
}
```

**More than a hundred combinations without a collision in a few seconds**, even in a debug build.
Some constants work with several rotations (one of them with 6, 14, 26 and 27): once a constant
separates the 272 short names, only the 141 longer ones are left for the rotation. I took the
first one:

```rust
let words = word_1 ^ word_2.rotate_left(7u32);
let multiplied_by_magic_constant = words.wrapping_mul(0xA903_8A92_1825_F10D as u64);
```

**No collision in the indices means no probing at all.** Every station is in its own slot, so no
station can take another station's slot either. The tool over the 1B file with the new hash: no
station probed. The rotation and the constant are in two places (`hash` in `main.rs` and the
calls of the copy in the tool), and the first run of the tool had the new pair in one and the old
one in the other, so it measured the old hash. One constant in one place would make that
impossible.

### 4. The result

| Per row, 1 thread | Part 013 | **No collision** | |
|---|---|---|---|
| Cycles | 28.41 | **28.02** | −0.39 (−1.4 %) |
| Instructions | 151.92 | 151.85 | −0.07 |
| Branches | 21.68 | 21.67 | −0.01 |
| Mispredicted | 0.108 | **0.100** | −0.008 |
| Time | 6.35 s | 6.28 s | |
| 8 threads | 0.809 s | **0.798 s** | |

The output is identical to hashbrown's (`cmp`), which also checks the arrays of a fixed size of
Part 013, the first version compared since the `Vec`.

- **The mispredictions fell by what the probing had**: 0.008 fewer per row, against 0.0073 rows
  that probed. Almost every probe was a misprediction.
- **The instructions and branches hardly moved.** One step of the probing on 0.73 % of the rows
  is ~0.007 branches. The cost wasn't the number of branches, it was that the one taken was a
  surprise.
- **~49 cycles for each misprediction that's gone** (0.39 / 0.008): the misprediction itself
  (~31 – 38 cycles when the branch waits for a load) and the probe, the load of the next slot
  and its comparison. One run, so the last digits may be noise.

**This is a hash tuned to the data.** It has no collisions for these 413 names. The rules allow
any names, up to 10,000 of them, and for any other set this constant is as random as the one
before. It stays correct, because the probing is still there and still handles every collision,
only the 0.39 cycles are a gain for this file.

### 5. `#[inline(always)]` on `next`

After the hash, `perf record -e branch-misses:u` split the 0.100 mispredictions per row by
function:

| Function | Share | Per row |
|---|---|---|
| The closure of the thread (`one` inlined, with `get_mut`, `key_words`, `hash` and `update`) | 79.7 % | 0.080 |
| `StationTemperatureIterator::next` | 19.0 % | 0.019 |
| `memcmp` (names of 16 bytes and more) | 1.1 % | 0.001 |

**On Zen 5 in this VM, the annotate of `branch-misses` doesn't point at branches.** The most
samples were on the instructions right after loads: 15.4 % on the `test` of the `word_1` just
loaded from the slot, 9.2 % on the load of a digit of the temperature. That's skid: without
precise sampling (IBS), the sample goes to the instruction that's waiting to finish when the
interrupt of the counter comes, not to the branch that was mispredicted. What the annotate did
show is the structure:

- **The parser of the temperature has no branches**: the 3 or 4 digits and the sign are both
  `cmov`.
- **`next` was still a function called on every row**, the one piece of the hot loop that wasn't
  inlined. Its result, an `Option<(StationName, &[u8])>` of five words, was written to the stack
  (`leaq 0x60(%rsp),%rdi`) and read back right after the call (`movq 0x60(%rsp),%r9` and the
  next four), and the state of the iterator lived on the stack too (`leaq 0x88(%rsp),%rsi`). The
  key, the hash and the load of the slot all waited for that round trip through memory.

The change is one line, `#[inline(always)]` on `fn next` in the `impl Iterator for
StationTemperatureIterator`:

| Per row, 1 thread | No collision | **`next` inlined** | |
|---|---|---|---|
| Cycles | 28.02 | **24.94** | −3.08 (−11 %) |
| Instructions | 151.85 | 130.54 | −21.31 |
| Branches | 21.67 | 17.91 | −3.76 |
| Mispredicted | 0.100 | 0.101 | 0 |
| Time | 6.28 s | 5.59 s | |
| 8 threads | 0.798 s | **0.707 s** | −11.4 % |

The output is identical to hashbrown's (`cmp`).

- **The biggest step since the table itself**, from one attribute.
- **3.76 branches fewer, not 2.** The `call` and the `ret` are two of them. The others are checks
  the compiler could join or drop once it saw both sides: `next` returned an `Option` that the
  closure checked again right after the call, and the bounds checks of the slices in `next`.
  That's what I expect, not what I've seen; the annotate of the inlined loop would show it.
- **The mispredictions didn't move**: inlining removes branches that were always predicted, not
  the ones that weren't.

### 6. A SWAR parser of the temperature

SWAR is *SIMD Within A Register*: one `u64` holds several values, here the bytes of the
temperature, and one instruction works on all of them. The trick is from the winning Java
solutions (merykitty, thomaswue). A temperature in 1BRC is always `X.X`, `XX.X`, `-X.X` or
`-XX.X`, so the parser loads 8 bytes from its start as one `u64` and computes the value with a
few bit operations, without a branch and without looking at the bytes one by one:

```rust
pub fn parse_temperature_swar(input: &[u8]) -> i16 {
    let word = if input.len() >= 8 {
        u64::from_le_bytes(input[..8].try_into().unwrap())
    } else {
        let mut helper_array = [0u8; 8];
        helper_array[..input.len()].copy_from_slice(input);
        u64::from_le_bytes(helper_array)
    };
    let dot_index =  (!word & 0x1010_1000).trailing_zeros() / 8;
    let sign =  ((!word << 59) as i64) >> 63;
    let without_sign = word & !((sign as u64) & 0xFF);
    let aligned = without_sign << (8 * (3 - dot_index));
    let digits = aligned & 0x0F_000F_0F00;
    let value = (digits.wrapping_mul(0x640A_0001) >> 32) & 0x3FF;
    ((value as i64 ^ sign) - sign) as i16
}
```

Line by line:

- **`from_le_bytes`**: the first byte of the text becomes the lowest byte of the number, so byte
  *k* of the text is bits 8k to 8k + 7. Both x86 and ARM are little-endian, so it's a plain load.
- **The dot**: the digits `0x30`–`0x39` have bit 4 set (`0011_xxxx`), the dot `0x2E` doesn't
  (`0010_1110`). The dot is always on byte 1, 2 or 3, so `0x1010_1000` takes bit 4 of those three
  bytes (bits 12, 20 and 28), `!` makes the dot the only one, and `trailing_zeros() / 8` is its
  index.
- **The sign**: `-` (`0x2D`) has no bit 4 either. `<< 59` moves bit 4 of byte 0 to bit 63, and the
  arithmetic shift `>> 63` of an `i64` copies it to all 64 bits: −1 for `-`, 0 for a digit. On a
  `u64` the shift would be logical and give 1 instead of −1.
- **Without the sign**: `!(sign & 0xFF)` clears byte 0 only for a negative number. The `-` becomes
  0, which then works as a leading zero: `-1.5` looks like `01.5`.
- **Aligned**: the shift puts the dot on byte 3. The tens are then on byte 1 (0 for `X.X`), the
  ones on byte 2 and the tenths on byte 4, and `0x0F_000F_0F00` keeps their low halves, the
  values 0–9, and clears the dot and everything after the tenths.
- **One multiplication for three**: `0x640A_0001` is 100 · 2²⁴ + 10 · 2¹⁶ + 1, so the product is
  the sum of three shifted copies of `digits`, and the column of byte 4 gets 100 × tens, 10 ×
  ones and the tenths:

  ```text
  byte:                    7     6     5     4     3     2     1     0
  digits * 1                                 c     0     b     a
  digits * 10 << 16              10c    0   10b   10a
  digits * 100 << 24      100c    0  100b  100a
  ```

  (`a` the tens, `b` the ones, `c` the tenths.) 999 needs 10 bits, so the result is bits 32 to 41,
  `>> 32` and `& 0x3FF`; the other products end up below bit 32 or above bit 41. In a `u32` the top
  two bits of anything from 25.6 up would be lost: my first version gave 231 for 99.9.
- **The sign back**: `x ^ 0 - 0` is `x`, and `x ^ -1 - (-1)` is `!x + 1`, which is `-x` in two's
  complement. Two instructions of 1 cycle each instead of a multiplication of 3.

**The iterator returns the rest of the chunk** from the start of the temperature, like the
`rest` of `StationName`, so that there are 8 bytes to load. Only the last temperature of a chunk
has fewer, and goes through the copy into `helper_array`: 8 times in the whole file.

A test compares the parser with the old one for every temperature from −99.9 to 99.9, with
`\nAbcdefgh` after it to check that the bytes after the temperature don't matter.

| Per row, 1 thread | `next` inlined | **SWAR parser** | |
|---|---|---|---|
| Cycles | 24.94 | **24.08** | −0.86 (−3.4 %) |
| Instructions | 130.54 | 123.54 | −7.00 |
| Branches | 17.91 | 16.88 | −1.03 |
| Mispredicted | 0.101 | 0.101 | 0 |
| Time | 5.59 s | 5.38 s | |
| 8 threads | 0.707 s | **0.685 s** | −3.1 % |

The output is identical to hashbrown's (`cmp`).

- **One branch fewer, not one more.** I expected the `input.len() >= 8` branch to add one. But the
  old parser had three bounds checks on every row in the annotate of change #5 (an empty
  temperature, nothing after the `-`, `digits[len - 3]` below zero), because the compiler didn't
  know a temperature has 3 to 5 bytes. That they are what went away is my guess; an annotate of
  the new loop would show it.
- **More cycles than instructions would explain.** The temperature is on the critical path: the
  `update` of the stats waits for it, so a shorter chain of dependent instructions counts more
  than the number of instructions.

### 7. The iterator with only the `;` mask

The parser knows where the line ends: the dot is `dot_index` bytes after the start of the
temperature, one digit and the `\n` follow, so the next line starts `dot_index + 3` bytes after
it. The iterator doesn't need to look for the `\n` any more. It calls the parser itself, a copy
of the one of change #6 that also returns `dot_index` (`parse_temperature_swar_with_dot_index`),
and returns the temperature as an `i16`:

```rust
let temperature = &self.map[self.block_start + delimiter_index + 1..];
let (parsed_temperature, dot_index) = parse_temperature_swar_with_dot_index(temperature);
// if at = 0 for abc;1.0\ndef;2.0\n so dot index is 3 and next at has to be 0 + 3 + 2 + 4 = 9 to start from def;2.0
self.at = self.block_start + delimiter_index + dot_index as usize + 4;
Some((station_name, parsed_temperature))
```

The `Item` of the iterator is `(StationName<'a>, i16)` now, and `one` only does
`stats.get_mut(station).update(temperature)`. The second SIMD comparison with `\n`, its
`to_bitmask` and the `newline_bit_mask` are gone; the next block is loaded when the mask of `;`
runs out. Every line has exactly one `;`, so a line whose `;` is in the next block starts that
block.

| Per row, 1 thread | SWAR parser | **Only `;`** | |
|---|---|---|---|
| Cycles | 24.08 | **24.03** | −0.05 |
| Instructions | 123.54 | **114.45** | −9.09 |
| Branches | 16.88 | 16.82 | −0.06 |
| Mispredicted | 0.101 | **0.120** | **+0.019** |
| Time | 5.38 s | 5.40 s | |
| 8 threads | 0.685 s | **0.683 s** | |

The output is identical to hashbrown's (`cmp`).

- **9 instructions fewer, and the same cycles.** At ~35 cycles each
  ([Part 013, change #6](part013-own-hash-table.md#6-the-number-of-slots)), the 0.019 more
  mispredictions are ~0.7 cycles per row, about what the instructions saved.
- **The same as in the first version of Part 013**, where the same iterator gave 0.02 more
  mispredictions. So it isn't noise; it's what this iterator does.
- **Which branch, I don't know.** My guess is the one that loads the next block
  (`delimiter_bit_mask == 0`): it's taken about every 4th or 5th row, depending on the lengths of
  the lines, so it's only partly predictable, and its prediction depends on the history of the
  branches around it, which changed when the code around the `\n` mask went away. Not checked; a
  record of the last branches (LBR), if the VM allows it, would show it.
- **I kept it.** It's as fast and 9 instructions shorter, and the next step goes after the very
  branch I suspect.

### 8. Where a row waits: top-down again

At 114 instructions in 24 cycles, the CPU already does 4.8 instructions per cycle (Zen 5 can do
up to 8). Before guessing what to remove next, `perf stat -M PipelineL1,PipelineL2` split the
slots of the pipeline again, as in [Part 013, change #1](part013-own-hash-table.md#1-where-hashbrown-loses-top-down-and-branches-on-zen-5):

| Slots | hashbrown (Part 013) | Now |
|---|---|---|
| Retiring (useful work) | 45.6 % | **50.9 %** |
| Bad speculation | 12.2 % | **4.2 %** |
| Frontend bound | 22.3 % | 8.5 % (7.0 % latency) |
| Backend bound | 19.9 % | **36.4 %** (33.9 % memory, 2.5 % CPU) |

I had predicted ~15 % of bad speculation from 0.120 mispredictions at ~35 cycles each, and
backend bound at 10 – 15 %. Both wrong:

- **Bad speculation counts only the slots thrown away on the wrong path.** The empty pipeline
  after a misprediction, until the right instructions arrive, is frontend latency. The
  mispredictions cost 4 – 11 %, not 17 %.
- **The backend waits for memory.** AMD computes it from the cycles in which nothing retired: in
  93 % of them (4.57 G of 4.91 G) the oldest instruction was a load that wasn't done.

Waiting for a load doesn't have to be a cache miss; a load from L1 takes 4 – 5 cycles too. The
generic `L1-dcache-*` events give 0 in this VM, but the TLB ones work, and on AMD their names are
misleading:

| `perf` name | What it counts on AMD Zen | Count | Per row |
|---|---|---|---|
| `dTLB-loads` | misses in the L1 DTLB (the translation looked up in the L2 TLB) | 279 M | **0.28** |
| `dTLB-load-misses` | misses in the L2 TLB too (a page walk) | 24,624 | ~0 |

Every 3rd or 4th row didn't find the translation of an address in the L1 DTLB. Not the input:
it's in `tmpfs` with 2 MB pages, ~6,900 pages for the whole file. My guess was the table: it's on
the heap, so on 4 KB pages, 512 KB of slots and 256 KB of names are 192 pages, the hash spreads
the 413 stations over all of them, and the L1 DTLB of Zen 5 has ~96 entries.

**The check**, with `SLOT_BITS = 10` for one run (1,024 slots, 12 pages; correct for 413
stations, only with collisions, since the constant of change #3 is for 14 bits): `dTLB-loads`
went from 279 M to **0.4 M**. It's the table. Earlier, when I thought about a smaller table
after change #4, I had guessed that the dTLB would cost "a fraction of a per cent". It was 28 % of
the rows.

### 9. The slots on a 2 MB page

The table should stay big (the rules and no collisions) and still be one entry in the TLB.

**`huge=always` of the `tmpfs` doesn't help**: it's for the files in it. The heap is anonymous
memory, and for that `/sys/kernel/mm/transparent_hugepage/enabled` was `[madvise]`: huge pages
only for memory that asks for them. Switching it to `always` for one run didn't change the 279 M
either, because a huge page covers only 2 MB aligned to 2 MB, and the allocations of the heap
weren't.

So the slots get their own allocation, aligned to 2 MiB, and ask for the huge page before the
first write:

```rust
// 2MB page size
#[repr(C, align(2_097_152))]
pub struct HugePageAligned<T>(pub T);

pub struct StationTable<'a> {
    pub slots: Box<HugePageAligned<[Slot; SLOTS]>>,
    pub names: Box<[&'a [u8]; SLOTS]>,
}
```

```rust
pub fn new() -> Self {
    // allocated, nothing written yet, so no 2 MiB on the stack
    let mut slots = Box::<HugePageAligned<[Slot; SLOTS]>>::new_uninit();
    let ptr = slots.as_mut_ptr();
    unsafe {
        // before the first write: the page fault then gets a 2 MiB page
        #[cfg(target_os = "linux")]
        if libc::madvise(ptr as *mut libc::c_void, size_of::<HugePageAligned<[Slot; SLOTS]>>(), libc::MADV_HUGEPAGE) != 0 {
            panic!("madvise MADV_HUGEPAGE failed: {:?}", std::io::Error::last_os_error());
        }
        for i in 0..SLOTS {
            (&raw mut (*ptr).0[i]).write(Slot::default());
        }
    }
    Self {
        slots: unsafe { slots.assume_init() },
        // this way it's not created on stack before boxing
        names: (0..SLOTS).map(|_| &[][..])
            .collect::<Vec<_>>()
            .into_boxed_slice()
            .try_into()
            .unwrap(),
    }
}
```

- **`align` goes on the type in the `Box`.** On `StationTable` it would align only the two
  pointers, and make the struct 2 MiB big, on the stack.
- **`Box::new_uninit()`** allocates with the alignment of the type and writes nothing, so nothing
  of 2 MiB is built on the stack first (the gotcha of
  [Part 013, change #7](part013-own-hash-table.md#7-arrays-of-a-fixed-size-instead-of-vec)).
  `&raw mut (*ptr).0[i]` is a pointer to an element without a reference to memory that isn't
  initialized yet, and `assume_init()` is the promise that all of it is.
- **`madvise` before the first write**: the kernel gives the huge page at the page fault of the
  first write, and only if it knows by then. `MADV_HUGEPAGE` exists only on Linux, so on macOS the
  call is left out by `#[cfg]`.
- **The names stay on 4 KB pages**: they're read only on an insert and for the 10 names of 16
  bytes and more.
- `into_entries` goes through references now, `self.names.iter().zip(self.slots.0.iter())`, so
  that it doesn't move the arrays out of the boxes onto the stack, and `MeasurementStats` derives
  `Clone, Copy` for that.

**First I had both arrays in one struct** aligned to 2 MiB (768 KB in one page). Measured right
after each other, the same code without and with the `madvise`:

| Per row, 1 thread | One struct, 4 KB pages | **One struct, 2 MB page** |
|---|---|---|
| Cycles | 24.49 | **23.43** (−1.06, −4.3 %) |
| Instructions | 116.47 | 115.47 |
| `dTLB-loads` | 267 M | **0.18 M** |
| Time | 5.52 s | 5.26 s |

The huge page alone is −1.06 cycles, ~3.8 cycles for each miss in the L1 DTLB that's gone. But
the one struct made the loop 1 – 2 instructions longer than with two boxes, and commenting out
the `madvise`, which isn't in the loop, changed the instructions by one per row too: `new` is
inlined into the function of the thread, and a different `new` gives a different allocation of
registers in the whole function. My guess; I haven't looked at the annotate.

With the slots in their own allocation, the instructions went back to 114.45:

| Per row, 1 thread | Only the `;` mask | **Slots on a 2 MB page** | |
|---|---|---|---|
| Cycles | 24.03 | **23.70** ± 0.35 % | −0.33 |
| Instructions | 114.45 | 114.45 | 0 |
| Mispredicted | 0.120 | 0.120 | 0 |
| `dTLB-loads` | 279 M | **0.29 M** | |
| Time | 5.40 s | 5.27 s | |
| 8 threads | 0.683 s | **0.668 s** | −2.2 % |

The output is identical to hashbrown's (`cmp`). The names on 4 KB pages added only ~0.1 M misses.
The cycles are from another session than the one struct (the instance had been restarted), with
a spread of 0.35 %, so the 0.27 cycles between the two versions are no evidence either way; the
8 threads were faster with this one, 0.668 s against 0.684 s, in two runs each.

**The frequency on 8 threads.** With the one struct on a 2 MB page, 8 threads did the same
cycles in total as one thread (23.47 G against 23.43 G), so the huge page works on every thread,
but at 4.27 GHz instead of 4.46 GHz, and the wall time didn't move. The turbo of all the cores
together is lower than of one. Whether the saved cycles partly turned into a lower clock, I
haven't measured: that needs the cycles of 8 threads without the huge page. **From here on, the
cycles are the measure, not the time**: the clock of this VM changes between sessions and with
the load.

### 10. Three experiments, kept none

**Against the mispredictions.** Without LBR in this VM ("doesn't support branch stack sampling"), it's not possible to see which
branch is mispredicted. My suspect was the branch that loads the next block of 64 bytes when the
mask of `;` runs out: it's taken every 4th or 5th row, depending on the lengths of the lines.

**Two blocks per load.** A `u128` mask of two blocks of 64 bytes, so the branch is taken half as
often:

| Per row, 1 thread | One block (`u64`) | Two blocks (`u128`) | |
|---|---|---|---|
| Cycles | 23.70 | 24.00 | +0.30 |
| Instructions | 114.45 | 128.99 | **+14.54** |
| Branches | 16.82 | 16.68 | −0.14 |
| Mispredicted | 0.120 | **0.078** | **−0.042** |
| 8 threads | 0.668 s | 0.682 – 0.688 s | |

Half of the branch took 0.042 of the mispredictions, so all of it is ~0.08 of the 0.120. That's
the evidence I couldn't get from LBR. But every operation on the mask in every row doubled: two
`tzcnt` and a choice for `trailing_zeros`, a subtraction with a carry over two registers for
`mask &= mask - 1`, two registers for the mask. I had predicted ±2 instructions.

**Two phases.** The idea of removing the branch altogether, as simdjson does: a `refill` goes
through the next 4 KB of the chunk, makes the `u64` mask of `;` of each block of 64 bytes and
writes the positions of the `;` into an array, always 8 positions per block (`trailing_zeros` and
`mask &= mask - 1` eight times) and the count moved by `count_ones()`, so without a branch on how
many `;` a block has. `next` then takes the next position from the array, and has a branch only
on the end of the array, once per ~280 rows.

| Per row, 1 thread | One block (`u64`) | Two phases | |
|---|---|---|---|
| Cycles | 23.70 | 24.51 | **+0.81** |
| Instructions | 114.45 | 131.23 | **+16.78** |
| Branches | 16.82 | 16.25 | −0.57 |
| Mispredicted | 0.120 | **0.031** | **−0.089** |
| 8 threads | 0.668 s | 0.699 – 0.700 s | |

The mispredictions went down even more than I predicted (~0.04), so the branch of the next block
was ~0.09 of them. But 16.8 instructions more, at ~5 per cycle ~3.4 cycles, against ~2.6 cycles
saved: **~29 cycles per misprediction**, and the top-down had said it, only 4.2 % of bad
speculation. The 8 writes per block are almost twice the ~4.6 that are needed, ~40 instructions
per block, ~10 per row. I had predicted +4 to +8.

**I went back to one block.** What I keep from both: the branch of the next block makes ~0.09
of the 0.120 mispredictions, and removing it costs more than ~2.6 cycles per row so far.

**Two halves of the chunk in turns.** The top-down said 34 % of the slots wait for a load, so the
next idea was thomaswue's: split the chunk of a thread at a line in the middle, and take a row
from each half in every step of the loop, so that the CPU can work on one row while the other
waits. In the first version of Part 013 three such scanners made it 15 – 19 % slower, most likely
with spills; two need fewer registers, and the loop is leaner now. Measured with the default
build, in one session:

| Per row, 1 thread | One stream | Two halves |
|---|---|---|
| Cycles | 23.62 | 23.70 |
| Instructions | 114.45 | 118.49 |
| Branches | 16.82 | 16.35 |
| Mispredicted | 0.120 | 0.123 |
| 8 threads | 0.667 s | 0.673 s |

The same cycles, four instructions more (the annotate showed stores and loads of the stack), and
slower on 8 threads. **The CPU already overlaps the rows itself**: the out-of-order window of
Zen 5 holds ~450 instructions, ~4 rows of 114. A row doesn't wait for the one before, there's
just a lot of work in it. I went back to one stream.

### 11. `-C target-cpu=znver5`

The `perf annotate` with `cycles` of the two halves had no instruction with more than 6 % of the
samples: no single bottleneck, just many instructions. And among them, for every block of 64
bytes:

```text
movdqu   0x10(%r11,%r8), %xmm3
pcmpeqb  %xmm4, %xmm0
pmovmskb %xmm3, %ecx
orl      %eax, %ecx
```

Without a `target-cpu`, the compiler targets the basic x86-64 of 2003, with SSE2 and registers of
16 bytes, so that the binary runs on any 64-bit x86. A `u8x64` is then four loads, four
comparisons, four `pmovmskb` and the shifts and `or`s that join the four masks, ~18 instructions
per block. The mask of `key_words`, a `u128` shifted by a variable, was a sequence of `shrd`,
`shr`, `test $0x40` and `cmov`.

`znver5` is the name of AMD Zen 5 for LLVM. With it the compiler may use everything Zen 5 has
(AVX2, BMI2, AVX-512 with its extensions) and tunes the code for its timings. `x86-64-v3` is AVX2
and BMI2 only. One stream, three builds in one session:

| Per row, 1 thread | default (SSE2) | `x86-64-v3` (AVX2) | **`znver5`** |
|---|---|---|---|
| Cycles | 23.62 | 22.30 | **21.35** (−2.27, −9.6 %) |
| Instructions | 114.45 | 105.25 | **103.75** (−10.7) |
| Branches | 16.82 | 16.82 | 16.60 |
| Mispredicted | 0.120 | 0.120 | 0.119 |
| Time | 5.27 s | 4.97 s | **4.75 s** |
| 8 threads | 0.668 s | | **0.608 s** (−9.0 %) |

The output is identical to hashbrown's (`cmp`).

- **`x86-64-v3` makes most of the instructions**: two registers of 32 bytes per block instead of
  four of 16, and BMI2 shifts by a variable in one instruction.
- **`znver5` gives another cycle for 1.5 instructions and 0.22 branches.** With AVX-512 the
  comparison of a whole block goes straight into a mask register; the rest is the tuning for
  Zen 5, I guess, I haven't looked at its annotate.
- **In the first version of Part 013 it was the other way around**, `znver5` +8 % of the cycles,
  the same as the AVX-512 build of
  [Part 009](part009-masked-loads-and-dtlb-misses.md#4-jons-whole-program-on-x86). That code had
  two comparisons per block, another parser and `next` as a call. Why it was slower then, I don't
  know; it isn't now.
- I predicted ~108 instructions and 22.4 – 22.7 cycles.

The binary now runs only on a CPU with everything Zen 5 has; on another one it stops with
"illegal instruction". For 1BRC, where the machine is given, that's fine. So that every build on
the instance uses it, without `RUSTFLAGS`, it's in `.cargo/config.toml`, only for Linux on x86-64,
so the builds on the Mac stay as they were:

```toml
[target.x86_64-unknown-linux-gnu]
rustflags = ["-C", "target-cpu=znver5"]
```

`RUSTFLAGS` in the environment overrides this file.

### 12. A key of 32 bytes

The key was the first 16 bytes of the name in two `u64`, so a row compared `word_1` and `word_2`
of the slot, and for the 10 names of 16 bytes and more also called `memcmp` on the names. The
longest name in the data has 26 bytes. With AVX-512 from change #11, the first 32 bytes of a
name are one register, so the key can be the whole name:

```rust
// first 32 of FF and last 32 of 0
static KEY_MASK_BYTES: [[u8; 32]; 2] = [[0xFF; 32], [0; 32]];

#[inline(always)]
pub fn key(station: &StationName) -> u8x32 {
    let in_key = station.len.min(32);
    let mask = u8x32::from_slice(&KEY_MASK_BYTES.as_flattened()[32 - in_key..64 - in_key]);
    let bytes = if station.rest.len() < 32 {
        u8x32::load_or_default(station.rest)
    } else {
        u8x32::from_slice(&station.rest[..32])
    };
    bytes & mask
}
```

- **The mask is a window into 64 bytes**: 32 bytes of `0xFF`, then 32 of 0, and the 32 bytes that
  start at `32 - len` have `0xFF` in the first `len` of them. All 33 masks (for 0 to 32 bytes) are
  windows of one array, one cache line, instead of a table of 33 × 32 bytes. One load and one
  `and`, and no shift of a `u128` by a variable.
- **`.min(32)`**: the rules allow names of up to 100 bytes, and `32 - len` of a `usize` would
  overflow.
- **`load_or_default` at the end of the chunk**, where fewer than 32 bytes are left; the same
  branch as `key_words` had, taken only for the last rows of a chunk.
- **Not with a comparison of lane indices.** My first version made the mask with
  `simd_lt` and `select`. The nightly on the instance is newer than the one on the Mac, and it
  has `select` in a trait `Select` and no `to_int` on a mask, so neither compiled there. The window
  into an array needs no method of a mask at all.

The hash stays the same, so the table stays without collisions. `key_hash_words` takes its two
words from the first 16 bytes of the key, which are the same bytes `key_words` gave: the name and
zeros after it, or the first 16 bytes of a longer name. And the output, identical to hashbrown's,
shows that every station still finds its own slot.

The slot is the key and the stats, 48 bytes, aligned to a cache line:

```rust
// one cpu cache line per slot to reduce slower split load
#[derive(Debug)]
#[repr(C, align(64))]
pub struct Slot {
    key: u8x32,
    stats: MeasurementStats,
}
```

Without the alignment every other slot would start in the middle of a cache line and end in the
next one (48, 96, 144…), and its key would be a load split over two lines. `repr(C)` keeps the
key at offset 0 and the stats at 32; without it Rust may reorder the fields, as it did in
[Part 013, change #4](part013-own-hash-table.md#4-reading-get_mut-in-perf-annotate). 16,384 slots
of 64 bytes are 1 MB, still in one page of 2 MB.

```rust
pub fn get_mut(&mut self, station_name: StationName<'a>) -> &mut MeasurementStats {
    let key = key(&station_name);
    let (word_1, word_2) = key_hash_words(key);
    let mut index = hash(word_1, word_2);
    loop {
        if self.slots.0[index].key == key && (station_name.len <= 32 || self.names[index] == station_name.as_bytes()) {
            return &mut self.slots.0[index].stats;
        } else if self.slots.0[index].key == u8x32::splat(0) {
            self.slots.0[index].key = key;
            self.names[index] = station_name.as_bytes();
            return &mut self.slots.0[index].stats;
        } else {
            index = (index + 1) & SLOT_MASK;
        }
    }
}
```

- **Found first.** 999,999,587 of the 10⁹ rows find their station, 413 find an empty slot. Before,
  every row tested the empty slot first.
- **One comparison of 32 bytes** (`==` of two `u8x32` compares all the lanes) instead of two
  loads, two comparisons and two branches of `word_1` and `word_2`, and `len <= 32` instead of
  `len < 16`, always true in the data, so the names are never compared and `memcmp` is gone from
  the loop. A name of exactly 32 bytes is whole in the key too, so `<=`.
- **The empty slot by its key**, all zeros, which no name can have. My first version of this
  looked at `stats.count == 0`, which made the table depend on its caller calling `update` after
  every `get_mut`; the tests that don't, failed.

| Per row, 1 thread | Key of 16 bytes | **Key of 32 bytes** | |
|---|---|---|---|
| Cycles | 21.35 | **19.52** | −1.83 (−8.6 %) |
| Instructions | 103.75 | **93.06** | −10.69 |
| Branches | 16.60 | **12.46** | **−4.14** |
| Mispredicted | 0.119 | 0.112 | −0.007 |
| Time | 4.75 s | 4.41 s | |
| 8 threads | 0.608 s | **0.559 s** | −8.1 % |

The output is identical to hashbrown's (`cmp`).

- **The gain is less work, not fewer mispredictions.** I predicted −0.02 mispredictions from the
  `len < 16` branch, which had been ~0.03 over the floor in Part 013. It was −0.007. The four
  branches fewer are the comparisons of the two words, the empty slot tested first, and `memcmp`
  for 2.4 % of the rows, which has branches of its own.
- **I predicted −0.5 to −1 cycle, −3 to −8 instructions and −1 to −2 branches**; all three were
  bigger.
- **My version had one branch more and 0.23 cycles more** (13.46 branches, 19.75 cycles), with
  the same 93.06 instructions. The differences are small: the mask from a `[u8; 64]` instead of
  `[[u8; 32]; 2]` with `as_flattened()`, and the order of the two branches in `key`. My guess is
  that the compiler could drop one more bounds check in this one; the annotate of both would show
  it.

### 13. Summary

| 1B, Zen 5, 2 MB pages | Cycles per row | Mispredicted per row | 1 thread | 8 threads |
|---|---|---|---|---|
| End of [Part 013](part013-own-hash-table.md) | 28.41 | 0.108 | 6.35 s | 0.809 s |
| + a hash without collisions | 28.02 | 0.100 | 6.28 s | 0.798 s |
| + `next` inlined | 24.94 | 0.101 | 5.59 s | 0.707 s |
| + a SWAR parser | 24.08 | 0.101 | 5.38 s | 0.685 s |
| + only the `;` mask | 24.03 | 0.120 | 5.40 s | 0.683 s |
| + the slots on a 2 MB page | 23.70 | 0.120 | 5.27 s | 0.668 s |
| + `target-cpu=znver5` | 21.35 | 0.119 | 4.75 s | 0.608 s |
| **+ a key of 32 bytes** | **19.52** | **0.112** | **4.41 s** | **0.559 s** |

From now on, every change is measured against the step before it. The floor of Part 013 was
measured with `next` as a call, so it isn't a floor any more.

## What I learned

## Next steps

1. **`perf annotate` with `cycles`** on the current build: which of the ~93 instructions per row
   are bounds checks, spills and moves, against an estimate of ~50 that a row needs.
2. **The branch of the next block for less than ~2.6 cycles**: the first two experiments of change #10
   show what it costs and what it gives; the annotate of both would show where their extra
   instructions are.

## TODO

Things I want to try later. I'm not doing them yet, because Jon may get to them in a later
part of the stream, and I don't want to get ahead of him.

- **The rotation and the constant in one place**, as `pub const`s used by `hash` and by the tool.
- **The copies for the analysis** (`hash_analyse`, `get_mut_analyse`) out of the `david` binary,
  or `get_mut` and `hash` generic over them.
- **Which two names** made the one collision that no rotation removed.
- **The comment in `hash`** still says "from 0 to 2_047".
- **More than one run** of `perf stat` for the 0.39 cycles.
- **`key_words`** is used only by the tool and its tests now; the tool could use `key` and
  `key_hash_words`.
- **Names longer than ~60 bytes.** The rules allow up to 100 bytes. A block of 64 bytes that
  starts at such a name has no `;`, the mask is 0, `trailing_zeros()` gives 64 and the line is
  split in the wrong place. The longest name in the data has 26 bytes. The iterator had this
  before too, with the `\n` mask.
- **`parse_temperature_swar` and `parse_temperature_swar_with_dot_index`** are two copies of the
  same parser; one is enough.
- The TODO of [Part 013](part013-own-hash-table.md#todo).
