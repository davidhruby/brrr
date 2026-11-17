# Part 013 — My own hash table

**Dates:** 2026-11-13 – 2026-11-18
**Video progress:** the end of the stream, [10:33:47](https://www.youtube.com/watch?v=tCY7p6dVAGE&t=38027s), when this part was published
**Code:** git tag [`part013`](https://github.com/davidhruby/brrr/tree/part013). To get the code exactly as it was for this part: `git checkout part013`

New here? Start with the [Introduction](intro.md).

## What changed

The `HashMap` has been two thirds of the time since [Part 010](part010-build-settings-and-simd-name-comparison.md),
and the first thing on the list of [Part 011](part011-ideas-from-other-solutions.md#next-steps)
was to replace it with my own table, after thomaswue. This part does that, in small pieces,
each with its own tests, on the `r8a.2xlarge` of [Part 012](part012-threads-and-4kb-pages.md)
(AMD Zen 5, 8 cores).

How everything here is measured:

- the 1B file in `tmpfs` mounted with `huge=always` (2 MB pages,
  [Part 012, change #6](part012-threads-and-4kb-pages.md#6-huge-pages-for-tmpfs)), so that the
  numbers show only the computation;
- cycles, instructions, branches and mispredicted branches from `perf stat` on **one thread**,
  divided by 10⁹ to get the numbers **per row**:

  ```sh
  sudo LC_NUMERIC=en_US.UTF-8 perf stat -e cycles,instructions,branches,branch-misses ./target/release/david /mnt/ramdisk/1B_measurements.txt 1 > /dev/null
  ```

  (`LC_NUMERIC` only because Ubuntu's default `C.UTF-8` locale has no thousands separator);
- the wall time of **8 threads** from `time`;
- the output compared with `cmp` against the output of the hashbrown version of Part 012.

The starting point is the code of Part 012 as committed: **47.74 cycles, 202.2 instructions,
27.06 branches and 0.545 mispredicted branches per row**, 10.7 s on one thread and 1.36 s on 8.

### 1. Where hashbrown loses: top-down and branches on Zen 5

Before writing anything, what the CPU does with hashbrown. `perf stat -M PipelineL1` and
`-M PipelineL2` split the **slots** of the pipeline (Zen 5 dispatches up to 8 ops per cycle, so
slots = 8 × cycles) by what happened to them. Measured on the build with the timeline prints of
[Part 012, change #7](part012-threads-and-4kb-pages.md#7-the-same-code-a-different-inlining):

| Slots | Share | Level 2 |
|---|---|---|
| Retiring (useful work) | 45.6 % | |
| Bad speculation | 12.2 % | all of it mispredicted branches |
| Frontend bound | 22.3 % | 20.7 % latency: cycles in which nothing came into the pipeline at all |
| Backend bound | 19.9 % | 14.8 % memory, 4.6 % CPU |

The empty frontend cycles and the bad speculation go together: after every mispredicted branch
the pipeline is thrown away and starts again from the right address. Divided by the number of
mispredictions, that's **~16.5 empty cycles each**, and there were more than half a misprediction
per row.

Which branches, from `perf record -e branch-misses` and `perf annotate`:

- the `jne` after `self.len != other.len` in my `eq` of the station names, the most of all
  (12.5 % of the samples of its function);
- the branch on the result of the SIMD `.all()` of the same comparison;
- hashbrown's loop over the control bytes and the groups it probes;
- `hash_one`, which wasn't inlined, with branches on the length of the name;
- `memcmp`, from `eq` for names of more than 16 bytes.

All of these are about **which station the row is**, which the predictor can't know.

### 2. A floor: how much can any table give?

Before writing a table, an experiment that removes the lookup altogether: the stats indexed by
**the length of the name**, `stats[station.len]`, an array of 128. No hash, no comparison, one
load from L1. The output is wrong (stations of the same length are added up), but the cycles
are the floor that no real table can get under:

```rust
#[allow(dead_code)]
fn one_upper_bound(map: &[u8]) -> [MeasurementStats; 128] {
    let mut stats_array: [MeasurementStats; 128] = std::array::from_fn(|_| MeasurementStats::default());
    for (station, temperature) in StationTemperatureIterator::new(map) {
        let t = parse_temperature(temperature);
        let measurement_stats = &mut stats_array[station.len];
        measurement_stats.update(t);
    }
    stats_array
}
```

(`[MeasurementStats::default(); 128]` doesn't compile: the `[x; N]` syntax copies `x`, so it
needs `Copy`. `std::array::from_fn` calls the closure once for every element instead.)

| Per row, 1 thread | hashbrown | **Floor** | The lookup (the difference) |
|---|---|---|---|
| Cycles | 47.74 | **17.37** | **30.4 (64 %)** |
| Instructions | 202.2 | 81.9 | 120.3 |
| Branches | 27.06 | 10.48 | 16.6 |
| Mispredicted | 0.545 | 0.076 | **0.469** |
| Time | 10.7 s | 3.90 s | |

Measured three times, ±0.03 %. Two thirds of the program is the lookup, and 86 % of the
mispredicted branches are in it.

### 3. The table

`StationTable`, after thomaswue, in [`src/david/main.rs`](../src/david/main.rs), written piece by
piece, each piece with its tests before the next one.

**The key** is the first 16 bytes of the name as two `u64`s, read straight from the mapped file
(a `StationName` points to the rest of the chunk, as since
[Part 010](part010-build-settings-and-simd-name-comparison.md#3-comparing-station-names-with-simd-instead-of-memcmp)),
with the bytes past the end of the name zeroed:

```rust
fn key_words(station: &StationName) -> (u64, u64) {
    if station.rest.len() < 16 {
        let mut station_name_with_zeros = [0u8; 16];
        station_name_with_zeros[..station.len].copy_from_slice(&station.rest[..station.len]);
        let station_first_16_bytes = u128::from_le_bytes(station_name_with_zeros);
        let word_1 = station_first_16_bytes as u64;
        let word_2 = (station_first_16_bytes >> 64) as u64;
        (word_1, word_2)
    } else {
        let mask = u128::MAX >> (128 - (8 * station.len.min(16)));
        let station_first_16_bytes = u128::from_le_bytes(station.rest[..16].try_into().unwrap());
        let masked_station = station_first_16_bytes & mask;
        let word_1 = masked_station as u64;
        let word_2 = (masked_station >> 64) as u64;
        (word_1, word_2)
    }
}
```

- Fewer than 16 bytes left in the chunk happens only on its last lines. Then only the name is
  copied into an array of zeros, so it needs no mask. `try_into()` into `[u8; 16]` panics when
  the slice isn't exactly 16 bytes, which is why the short case can't use it.
- `.min(16)` because for a name of 16 bytes or more the shift would be `128 - 8 × len`, an
  underflow of `usize`.
- A name never contains a 0 byte, so the zeros also encode the length: "Bata" and a longer name
  that starts with "Bata" can't have the same key.

**The hash** is the two words mixed and multiplied by a constant, and **the top bits** of the
result, because the multiplication mixes into the high bits and the low bits of a product
depend only on the low bits of the inputs:

```rust
fn hash(word_1: u64, word_2: u64) -> usize {
    // reduce overlapping by rotating second word
    let words = word_1 ^ word_2.rotate_left(29u32);
    let multiplied_by_magic_constant = words.wrapping_mul(0x243F_6A88_85A3_08D3 as u64);
    (multiplied_by_magic_constant as usize) >> (64 - SLOT_BITS)
}
```

`rotate_left` and not `<<`: a shift would throw the top 29 bits of `word_2` away, a rotation
moves them to the bottom. (So `rotate_left` of all ones is again all ones, which is why
`hash(u64::MAX, u64::MAX)` is the hash of 0.) `wrapping_mul` because the overflow is the point;
plain `*` panics on it in debug builds.

**A slot** is `word_1`, `word_2` and the stats, 32 bytes. An empty slot has `word_1 == 0`, which
a real key never has. The names are in a second array, used only for the output and for names of
16 bytes or more.

**The lookup** is linear probing:

```rust
#[inline(always)]
fn get_mut(&mut self, station_name: StationName<'a>) -> &mut MeasurementStats {
    let (word_1, word_2) = key_words(&station_name);
    let mut index = hash(word_1, word_2);
    loop {
        if self.slots[index].word_1 == 0 {
            self.slots[index].word_1 = word_1;
            self.slots[index].word_2 = word_2;
            self.names[index] = station_name.as_bytes();
            return &mut self.slots[index].stats;
        } else if self.slots[index].word_1 == word_1
            && self.slots[index].word_2 == word_2
            && (station_name.len < 16 || self.names[index] == station_name.as_bytes()) {
            return &mut self.slots[index].stats;
        } else {
            index = (index + 1) & SLOT_MASK;
        }
    }
}
```

- **A name of 16 bytes or more is compared in full.** The key holds only its first 16 bytes:
  `Petropavlovsk-Ka` (16) and `Petropavlovsk-Kamchatsky` (24) have the same key. My first
  version had `<= 16`, and then a 16-byte name found the slot of a longer name with the same
  start. The test for it has to insert the longer name first; in the other order, the bug
  doesn't show.
- `||` short-circuits, so the names array isn't read for short names.
- `& SLOT_MASK` wraps the probing from the last slot to the first. It works only because the
  number of slots is a power of two: `SLOT_MASK` is all ones in the low bits.
- `self.names` can keep `station_name.as_bytes()` because both have the lifetime `'a` of the
  mapped file: the table can't outlive the data its names point to.
- The `return` of a `&mut` from inside the `loop` is fine for the borrow checker; the `loop`
  never ends without a `return`, so nothing has to follow it.
- **The table must never be full**: with as many stations as slots, the loop wouldn't end.

**Out of the table**, `into_entries(self)` gives a `Vec<(&[u8], MeasurementStats)>` of the
occupied slots: `zip` of the names and the slots, `filter` on `word_1 != 0`, `collect`. It takes
`self` by value, because `MeasurementStats` isn't `Copy` and can only be moved out of a table that
is consumed.

**Merging** the threads in `main` goes through a `BTreeMap<&[u8], MeasurementStats>` with
`entry(name).or_default()`: 413 stations × 8 threads, so the speed doesn't matter, and the
`BTreeMap` keeps the names sorted. Bytes sort the same as the UTF-8 strings they encode, so the
order is the same as the old `BTreeMap<&str, …>`.

**Tests:** `key_words` for names of 4, 11, 16 and 24 bytes and for the end of a chunk; `hash` in
range, and the same index for the same name in the middle and at the end of a chunk (the two
branches of `key_words`); `new`; `get_mut` for the same name twice (two different positions in
the data), for a name of 11 against one of 19 bytes with the same start, and for the 16 against
the 24 bytes above; `into_entries` for an empty table, two stations and the same station twice.
The order of `into_entries` depends on the hash, so the test sorts the result first; otherwise it
would break with any change of the hash or the size.

| Per row, 1 thread | hashbrown | **StationTable** | Floor |
|---|---|---|---|
| Cycles | 47.74 | **35.32 (−26 %)** | 17.37 |
| Instructions | 202.2 | 173.5 (−14 %) | 81.9 |
| Branches | 27.06 | 25.07 | 10.48 |
| Mispredicted | 0.545 | **0.221 (−59 %)** | 0.076 |
| Time | 10.7 s | 7.90 s | 3.90 s |
| 8 threads | 1.36 s | **1.003 s** | |

The lookup over the floor went from 30.4 to 18.0 cycles.
Instructions fell by a quarter, cycles by 41 %, mispredictions by 69 %: at ~16 cycles or more
each, the mispredictions are most of the gain.

### 4. Reading `get_mut` in `perf annotate`

Where are the 0.221 mispredictions now? `perf record -e branch-misses`, then `perf annotate
--stdio`. How to read the output, since I needed it:

- **`perf report --sort symbol` first.** `annotate` gives percentages **within one function**
  ("percent: local period"), so first the share of each function: here the body of the thread
  (with `one` and `parse_temperature` inlined) 36.7 %, `StationTable::get_mut` 31.9 %,
  `StationTemperatureIterator::next` 30.2 %, `memcmp` 1.0 %. **`get_mut` and `next` were
  functions of their own**, called on every row.
- **Never sort the lines of the whole file.** Every function with a sample gets a section, also
  kernel functions with a single sample, and that one sample is 100 % of its function. Cut the
  file per function instead (each section starts with `Source code & Disassembly`):

  ```sh
  awk '/Source code & Disassembly/{n++} n==2' /home/ubuntu/annotate-table.txt > /home/ubuntu/annotate-2.txt
  ```

  (and `-e branch-misses:u` next time, without the kernel).
- **The sample lands after the branch (skid).** The address is recorded when the misprediction
  is found out, a few instructions later: after the branch, or at its target. So from a hot line,
  go up to the nearest `j…`, the `cmp` or `test` before it, and the source line above them. The
  `push`es at the start of a function have samples although they can't mispredict: those come
  from the branches of the caller just before the `call`.

What the disassembly of `get_mut` showed (x86, AT&T syntax: `cmp %a, %b` compares `b` with `a`):

- **The layout.** `shlq $0x5` is the index × 32, the size of a `Slot`. `word_1` and `word_2` are
  at offsets `0x10` and `0x18`: **Rust put the stats first**, it may reorder the fields of a
  struct. So the address of the slot is also the address of its stats, the `&mut` that is
  returned. The names are `shlq $0x4`, 16 bytes each: a pointer and a length.
- **`key_words` and `hash` have no branches.** `.min(16)` is a `cmovb`; the `u128` shift is two
  64-bit shifts with `testb $0x40` and two `cmov`s for shifts of 64 and more; `try_into().unwrap()`
  has no check, because the compiler knows `rest.len() > 15` there; then `rolq $0x1d` (29),
  `imulq` with the constant, `shrq $0x35` (53).
- **The short case of `key_words` calls `memcpy`**: `copy_from_slice` with a length known only
  at run time. Only for the last lines of a chunk.
- **Three copies of the loop.** `station_name.len < 16` doesn't change inside the loop, so the
  compiler tested it once before the loop and made one loop for short names (only the two
  words), one for long names (the length of the name and `memcmp`), and one for a name longer
  than the rest of the chunk, which can't happen and ends in a panic. This is called *loop
  unswitching*.
- **A bounds check on every probe**: `cmpq %rbp, %r12; jae … panic_bounds_check`. `slots` is a
  `Vec`, and the compiler doesn't know its length, so it doesn't know that `index & SLOT_MASK`
  is always inside it.

And the samples: ~43 % of those of `get_mut` in the loop for short names, the hottest
instruction (18.7 %) right behind the target of the two `jne`s that go to the next slot when the
words don't match, and ~12 % on the `pop`s of the return, where the loop ended although the
predictor expected another probe. **The mispredicted branch in the table is the probing.** ~38 %
were on the first instructions of the function, in code without any branch: skid from the
caller.

### 5. `#[inline(always)]` on `get_mut`

| Per row, 1 thread | `get_mut` a function | **Inlined** | |
|---|---|---|---|
| Cycles | 35.32 | **34.19** | −1.13 (−3 %) |
| Instructions | 173.5 | 157.5 | −16.0 (−9 %) |
| Branches | 25.07 | 23.09 | −1.98 |
| Mispredicted | 0.221 | 0.222 | 0 |
| 8 threads | 1.003 s | 0.971 s | |

- **Exactly 2 branches per row fewer**: the `call` and the `ret`. The CPU counts both as
  branches.
- **16 instructions fewer, only 1.1 cycles**: the `push`es and `pop`s of six registers and the
  loads of the pointers and lengths of `slots` and `names` from `self` on every call. They don't
  depend on each other or on anything slow, so they ran next to the real work. The IPC fell from
  4.91 to 4.61: what went away was mostly easy work.
- **The mispredictions stayed**: inlining moves the branches, it doesn't change them.

**hashbrown's `get_mut`, from Part 012.** That `get_mut` was a call on every row without the
timeline prints and inlined with them
([Part 012, change #7](part012-threads-and-4kb-pages.md#7-the-same-code-a-different-inlining)),
and the only fix I had was `-inline-threshold=1000` for the whole program. I asked on Reddit how
this is handled in code where performance matters:
[Removing eprintln! made my hot loop 11 % slower. How do you control inlining in production Rust code?](https://www.reddit.com/r/rust/comments/1xvohjp/removing_eprintln_made_my_hot_loop_11_slower_how/)
The answer from the comments: **`#[inline(always)]` on `one`** made hashbrown's `get_mut`
inlined too. I had tried `#[inline(never)]` on `one` and on the hash before, but not this.

What I still don't understand is why. `#[inline(always)]` is on `one` only, and I didn't expect
it to reach the calls inside `one`. Maybe it doesn't: once `one` is inlined into the closure of
the thread, the call of `get_mut` is in a different function, and the compiler decides about it
again, in a new context that happens to come out under the threshold. With my own table the
question is gone, because `get_mut` is my code and the `#[inline(always)]` goes right on it.

### 6. The number of slots

The size was in four places (the two arrays, the mask and the shift of the hash), so first one
constant and the others derived from it:

```rust
const SLOT_BITS: usize = 14;
const SLOTS: usize = 1 << SLOT_BITS;
const SLOT_MASK: usize = SLOTS - 1;
```

The shift of the hash is `>> (64 - SLOT_BITS)`; the parentheses matter, `>>` binds tighter than
`-`. With `SLOT_BITS = 11` (2,048 slots, as before), the instructions and branches were the same
to the last digit and the cycles within 0.1 %, so the constants cost nothing. Then the sizes:

| `SLOT_BITS` | Slots | Full | Cycles | Instructions | Mispredicted | Over the floor | 1 thread | 8 threads |
|---|---|---|---|---|---|---|---|---|
| 11 | 2,048 | 20 % | 34.15 | 157.54 | 0.223 | 0.146 | 7.63 s | 0.971 s |
| 12 | 4,096 | 10 % | 31.69 | 156.81 | 0.159 | 0.083 | 7.08 s | 0.904 s |
| 13 | 8,192 | 5 % | 30.41 | 156.34 | 0.117 | 0.041 | 6.80 s | 0.858 s |
| **14** | **16,384** | **2.5 %** | **30.05** | **156.23** | **0.107** | **0.031** | **6.72 s** | **0.851 s** |

- **Every doubling halves the mispredictions of the probing**, from 11 to 13: half as full, half
  as many lookups whose first slot is another station's.
- **From 13 to 14 only 0.010 fewer.** What's left over the floor, ~0.03 per row, is no longer the
  probing. It's most likely the `len < 16` branch: names of 16 bytes and more come at random.
- **A misprediction costs ~31 – 38 cycles here**, not the ~16.5 of change #1 (2.46 cycles for
  0.064 fewer from 11 to 12, 1.28 for 0.042 from 12 to 13). The branch of the probing depends on
  the load of the slot, so the CPU finds out about the mistake only when that load is done, and
  all the work it did on the wrong path in the meantime is thrown away. The ~16.5 cycles were
  only the empty frontend after it.
- **The bigger table cost nothing in the cache**, even at 768 KB (16,384 × 32 bytes of slots and
  × 16 bytes of names). Only the ~413 slots that are used are ever loaded, and they are always the
  same ones.
- **14 is the smallest size that follows the rules**: they allow up to 10,000 different stations,
  and with 8,192 slots such a file would make `get_mut` loop forever. With 16,384, 10,000 stations
  are 61 % full.

The output with 14 is identical to hashbrown's (`cmp`).

### 7. Arrays of a fixed size instead of `Vec`

The bounds check of change #4 is there because the length of a `Vec` is a value at run time.
The length of an array is part of its type, so with `[Slot; SLOTS]` the compiler sees that the
index, the top `SLOT_BITS` bits of the hash or a value `& SLOT_MASK`, is always inside it. On the
heap, because 512 KB of slots is too much for the stack:

```rust
struct StationTable<'a> {
    slots: Box<[Slot; SLOTS]>,
    names: Box<[&'a [u8]; SLOTS]>,
}
```

**Building it.** My first version was `Box::new(std::array::from_fn(|_| Slot::default()))`, and
that builds the array **on the stack first**: `Box::new` is an ordinary function, and its argument
is a value that exists before the call. In a release build the optimizer may build it on the heap
directly, but nothing guarantees it, and in a debug build (`cargo test`) it doesn't, while the
threads of the tests have 2 MB of stack. The way that never touches the stack is a `Vec`, then
`into_boxed_slice()` (a `Box<[Slot]>`, no copy), then `try_into()` (a `Box<[Slot; SLOTS]>`, the
length checked once). Its `Err` is the original `Box<[Slot]>`, so `unwrap()` needs `Slot: Debug`.

| Per row, 1 thread | `Vec` | **`Box<[…; SLOTS]>`** | |
|---|---|---|---|
| Cycles | 30.05 | **28.41** | −1.64 (−5.5 %) |
| Instructions | 156.23 | 151.92 | −4.31 |
| Branches | 22.76 | 21.68 | −1.08 |
| Mispredicted | 0.107 | 0.108 | 0 |
| Time | 6.72 s | 6.35 s | |
| 8 threads | 0.851 s | **0.809 s** | |

One branch per row fewer, the bounds check of the first probe, which never mispredicted. I had
expected tenths of a cycle from it, because a branch that is always predicted right "costs
nothing". It was 1.64 cycles. Why that much, these counters don't say: the number of branches the
CPU can handle per cycle, a smaller loop, or a different layout of the code. **A predicted branch
isn't free, only cheaper.**

### 8. Summary

| 1B, Zen 5, 2 MB pages | Cycles per row | Mispredicted per row | 1 thread | 8 threads |
|---|---|---|---|---|
| hashbrown ([Part 012](part012-threads-and-4kb-pages.md)) | 47.74 | 0.545 | 10.7 s | 1.36 s |
| StationTable, 2,048 slots, `Vec` | 35.32 | 0.221 | 7.90 s | 1.003 s |
| + `get_mut` inlined | 34.19 | 0.222 | 7.64 s | 0.971 s |
| + 16,384 slots | 30.05 | 0.107 | 6.72 s | 0.851 s |
| **+ arrays of a fixed size** | **28.41** | **0.108** | **6.35 s** | **0.809 s** |
| Floor (no lookup) | 17.37 | 0.076 | 3.90 s | |

The lookup costs 11.0 cycles per row over the floor; it cost 30.4 with hashbrown.

- **A floor before writing anything.** Replacing the lookup with an array by length took ten
  minutes and said what the table could give at most, and the numbers of every step after it are
  measured against it.
- **Count all four: cycles, instructions, branches, mispredictions.** Inlining removed 9 % of the
  instructions and 3 % of the cycles; a bigger table removed almost no instructions and 12 % of
  the cycles. The mispredictions said which one would work.
- **A misprediction costs the most when the branch waits for a load**: ~35 cycles here.
- **Check what the compiler made.** `perf annotate` showed the calls, the bounds check, the
  reordered fields and the three copies of the loop; none of it is visible in the source.
- **Tests that can fail.** The test for names of 16 and 24 bytes caught the `<= 16` only with the
  longer name inserted first, and a test that relied on the order of the hash would have broken
  with the next change of the size.

## What I learned

## Next steps

1. **`perf annotate` with `cycles`** instead of `branch-misses`: where the 11 cycles of the
   lookup wait now.

## TODO

Things I want to try later. I'm not doing them yet, because Jon may get to them in a later
part of the stream, and I don't want to get ahead of him.

- **The iterator with only the `;` mask**, once the SWAR parser is in: the end of the line is
  known from the `.`, so the second SIMD comparison and `to_bitmask` go away.
- **Why AVX-512 makes the iterator slower on Zen 5**: a micro-benchmark of the 64-byte
  comparison and `to_bitmask` in both builds.
- **Remove the `three`, `hashbrown` and `upper` variants** and the timeline prints before the
  final measurement, or keep `hashbrown` behind the argument for comparisons.
- **Two blocks (128 bytes) per reload** of the `;` mask: the "next block" branch half as
  often, ~1 cycle per row, for a `u128` mask or two masks.
- **The `len < 16` branch** (~0.04 mispredictions per row, ~1.3 cycles): a key of 32 bytes
  compared with SIMD would cover every name in the data without a branch on the length.
- **`madvise(MADV_POPULATE_READ)`** per thread on its chunk: the pages mapped in batches
  instead of ~211,000 page faults (0.3 s of sys time, ~0.04 s of wall time with 8 threads).
- **Thread 0 is the last to finish by ~10 ms** in every run; find out why (the first pages of
  the file? the thread that is spawned first?). Blocks from an atomic counter would absorb it.
- **The spills**: stores and loads of the stack in the hot loop, even with one iterator.
- **The slot count**: 8,192 for 413 stations; with 10,000 stations (the rules) the table would
  have to grow to keep the probing rare, and the dTLB would start to matter.
- The TODO of [Part 012](part012-threads-and-4kb-pages.md#todo).
