# Part 011 — Ideas from other 1BRC solutions, on one thread

**Dates:** 2026-11-12 – 2026-11-12
**Video progress:** stopped at [5:02:58](https://www.youtube.com/watch?v=tCY7p6dVAGE&t=18178s) when this part was published
**Code:** git tag [`part011`](https://github.com/davidhruby/brrr/tree/part011). To get the code exactly as it was for this part: `git checkout part011`

New here? Start with the [Introduction](intro.md).

## What changed

Before going on with the stream, I went through three other solutions to see what they do that
I don't:

- [barrcodes.dev](https://barrcodes.dev/posts/1brc/), in Rust, single-threaded first and then on
  up to 112 threads;
- [curiouscoding.nl](https://curiouscoding.nl/posts/1brc/), in Rust, 4.93 s on one thread;
- [thomaswue](https://github.com/gunnarmorling/1brc/blob/main/src/main/java/dev/morling/onebrc/CalculateAverage_thomaswue.java),
  the winner of the [original challenge](https://github.com/gunnarmorling/1brc) in Java.

I'm staying on one thread for now, so I only took the ideas that make one thread faster. The
starting point is change #4 of [Part 010](part010-build-settings-and-simd-name-comparison.md):
**9.93 s and 42.86 billion cycles on 1B**, with the `HashMap` at two thirds of the time.

### 1. Parsing the temperature from 8 bytes at once (SWAR, not kept)

The trick from thomaswue's solution, by Quan Anh Mai (merykitty). It reads the 8 bytes after the
`;` as one `u64` and computes the temperature without branches:

- **Where the `.` is:** `(!word & 0x10101000).trailing_zeros()`. Digits have bit 4 set, `.`
  doesn't, so the first byte of 1 – 3 without it is the dot.
- **The sign:** `((!word) << 59) as i64 >> 63` is −1 for a `-` in the first byte, 0 for a
  digit. The `-` is then masked out of the word.
- **The digits** are shifted by `28 − dot` to fixed bytes and masked with `0x0F000F0F00`, so
  tens, ones and tenths are always in the same place.
- **One multiplication** by `0x640a0001` (100 · 2²⁴ + 10 · 2¹⁶ + 1) adds them up with their
  weights; the result is `>> 32 & 0x3FF`.
- **The sign is applied** with `(abs ^ sign) - sign`.

The 8 bytes may go past the temperature into the next line; that's allowed, because the
temperature now points to the rest of the mapped file, the same way as the key of
[Part 010, change #3](part010-build-settings-and-simd-name-comparison.md#3-comparing-station-names-with-simd-instead-of-memcmp).
In the last bytes of the file, fewer than 8 are left, so the old `parse_temperature` takes over.

A test goes through all 1,999 temperatures from −99.9 to 99.9, each followed by `\n` and another
line, and compares the result with the old function. In the criterion benchmark of the parser,
it was a quarter faster:

| | `parse_temperature` | SWAR |
|---|---|---|
| One value | ~826 – 832 ps | ~618 – 660 ps |
| 10,000 random values | 12.08 µs (1.21 ns each) | 9.19 µs (0.92 ns each, **−24 %**) |

**A bug first.** The first run gave a wrong output, one line in each file:
`Bridgetown=-22.5/27.0/275.8` instead of `…/75.0` on 1B. The last line of the file is
`Bridgetown;22.4\n`, so only 5 bytes are left after the `;`, and the fallback ran. It expected
only the temperature, but it got `22.4\n` and took the `\n` as the tenths. The fix: cut the
bytes at the `\n` first. A test for a temperature with `\n` and fewer than 8 bytes would have
caught it.

With the fix, the output is identical on both files:

| | Part 010, 100M | **SWAR, 100M** | Part 010, 1B | **SWAR, 1B** |
|---|---|---|---|---|
| Wall time | 1.49 s | 1.60 s | 9.93 s | 9.92 s |
| User time | 0.93 s | 0.96 s | 9.57 s | 9.77 s |
| Instructions | 19.59 billion | 18.47 billion (−5.7 %) | 206.6 billion | 194.7 billion (**−5.8 %**) |
| Cycles | 3.88 billion | 3.98 billion (+2.6 %) | 42.86 billion | 42.88 billion (**0 %**) |

**12 instructions fewer per row, and not a single cycle less.** Parsing the temperature wasn't
what the program waits for: the CPU does it while it waits for the `HashMap` anyway, so shorter
code there doesn't make the row faster. The 24 % of the benchmark became 0 % in the whole
program, the same thing as in [Part 009](part009-masked-loads-and-dtlb-misses.md#6-summary),
only all the way down to zero.

I reverted it. It would only pay off as a part of a bigger change: from the position of the `.`,
the end of the line is known (`.` + 3), so the iterator wouldn't have to look for the `\n` at
all (see TODO).

### 2. The exit in a child process (not kept)

[Part 010, change #2](part010-build-settings-and-simd-name-comparison.md#2-how-long-the-end-of-the-program-takes)
showed that taking down the mapping of the 13.8 GB file at the exit costs ~0.1 – 0.56 s outside
`main`. thomaswue and barrcodes leave it out of the measurement: the work runs in a child process,
and the parent ends as soon as the output is printed, while the child is still cleaning up.

How I did it, with `libc`:

- at the very start of `main`, before `mmap` and before the helper thread (`fork` copies only the
  calling thread), a pipe (`pipe`) and `fork`;
- **the child** does all the work and prints straight to the inherited stdout, flushes it,
  writes one byte to the pipe as "done", and closes stdout and the pipe **before** it ends.
  Without closing them, the parent would get the end of the pipe only after the kernel has
  unmapped the file (Linux, for example, takes down the memory of an ending process before it
  closes its files), and nothing would be saved;
- **the parent** closes its end of the pipe for writing, reads until the end of the pipe, ends
  with 0 if the "done" byte came, and with 1 if it didn't (the child crashed). It doesn't wait
  for the child.

One run on 1B: **9.83 s** against 9.93 s, with an identical output. 0.1 s is within how much
runs differ, and `/usr/bin/time -l` now measures only the parent (0.00 s of user time, 13 page
faults), so the instructions and cycles of the work aren't in the measurement any more.

I didn't keep it. The cleanup still happens, the CPU and the kernel still do it, the
measurement just doesn't see it any more. The rules of the challenge allowed it, but my numbers
are meant to show what the program costs. A fair way is to make the cleanup smaller instead of
hiding it (see TODO).

### 3. Branches for the minimum and the maximum (kept)

`MeasurementStats::update` used `min()` and `max()`:

```rust
self.min = self.min.min(measurement);
self.max = self.max.max(measurement);
```

That compiles without branches (`cmp` + `csel`), and both values are written back on every row,
even when they didn't change. But each station has ~2.4 million measurements, and a new minimum
or maximum comes only a few times, mostly at the start. So a branch is almost never taken, and
the branch predictor guesses it right:

```rust
if measurement < self.min {
    self.min = measurement;
}
if measurement > self.max {
    self.max = measurement;
}
```

barrcodes saved 2 % with it, curiouscoding 3.5 %. Measured twice, raw output of the second run
in [`measurements/part011/`](../measurements/part011/), the output identical:

| 1B | Part 010, `min()` / `max()` | **`if`, run 1** | **`if`, run 2** |
|---|---|---|---|
| Wall time | 9.93 s | 10.38 s | **9.88 s** |
| User time | 9.57 s | 9.66 s | 9.67 s |
| Instructions | 206.6 billion | 201.7 billion (−2.4 %) | 201.7 billion (−2.4 %) |
| Cycles | 42.86 billion | 42.42 billion (−1.0 %) | 42.50 billion (−0.8 %) |

- **5 instructions fewer per row**: two `csel` and two stores against two branches. So the
  compiler kept the branches and didn't turn them back into `csel`.
- **~1 % fewer cycles in both runs.** Small, close to how much runs differ, but in the same
  direction twice.
- **The 10.38 s of the first run was waiting, not computing**: `real` − `user` was 0.72 s
  against 0.36 s before, with the same user time. That's reading from the SSD and the cleanup at
  the exit, which this change doesn't touch.

## What I learned

## Next steps

1. **My own hash table instead of hashbrown**, after thomaswue: the two 8-byte words of the name
   with the bytes after it masked out, the hash made from those two words, the comparison of two
   `u64`s, and a plain array with open addressing, without control bytes and tags. Names longer
   than 16 bytes take a slow path. The `HashMap` is two thirds of the time now.

## TODO

Things I want to try later. I'm not doing them yet, because Jon may get to them in a later
part of the stream, and I don't want to get ahead of him.

- **Three lines at once on one thread** (thomaswue): split every block into three parts and take
  one line from each in turn, so that the CPU can overlap the waiting of one line for the
  `HashMap` with the work on the other two. curiouscoding saved 22 % with four. It works best
  once a line is cheap and independent, after my own hash table.
- **The iterator with only the `;` mask:** with the SWAR parser of change #1, the end of the line
  is known from the `.`, so the second SIMD comparison and the second `to_bitmask` (several
  instructions on ARM, which has no `movemask`) go away. The risk: the start of the next line
  then depends on parsing this one, a chain that thomaswue hides with the three lines at once.
- **Unmap the processed blocks during the run** instead of all of them at the exit: the helper
  thread would call `munmap` on blocks the main thread is done with, except block 0, where the
  keys of the `HashMap` point
  ([Part 010, change #5](part010-build-settings-and-simd-name-comparison.md#5-where-the-keys-in-the-hashmap-point-to)).
  `munmap` takes a lock over the whole address space that the page faults of the main thread
  need too, so it has to be measured. First, a few runs to see whether the cleanup is usually
  ~0.1 or ~0.5 s.
- **`-C target-cpu=native`** on the M4: one line, unsure gain (barrcodes −1.3 % on x86).
- **A perfect hash**: every station gets its own slot, without collisions (curiouscoding with
  PtrHash, 23 → 17 s). Needs a fallback for names that aren't known in advance.
- **A smaller entry table behind an index** (barrcodes' "redirection table"), for fewer misses
  in the L1 cache.
- **The capacity of the table**: barrcodes uses 1,024, I have 16,384 buckets; on the M4,
  `with_capacity(10_000)` was faster than `HashMap::new()` in [Part 007](part007-hashing.md).
- **A smaller table on Linux:** with 4 KB pages, the 16,384 buckets of
  `with_capacity(10_000)` spread over ~130 – 160 pages and cause ~0.75 dTLB misses per row.
  Measure `HashMap::new()` (512 buckets, ~20 KB, 5 pages) there; on the M4, the capacity was
  7 % faster. On Zen 5 ([Part 009, change #5](part009-masked-loads-and-dtlb-misses.md#5-the-dtlb-misses-jon-complained-about-on-x86)),
  halving the misses changed nothing, so probably not worth much; the Graviton may still differ.
- **Why Jon's code with `Vec<u8>` had only 0.004 dTLB misses per row on the Graviton**
  ([Part 008](part008-short-strings.md)), with the same 131,072-bucket table that gives ~1 per
  row on Zen 5, and mine ~0.75 with a table 8× smaller. Either the generic `dTLB-load-misses`
  counts something else on the Graviton, or something there I don't see yet.
- **Handle lines longer than 64 bytes in the SIMD iterator** ([Part 006, change #4](part006-finding-the-end-of-a-line.md#4-finding-the--and-the-n-of-several-lines-at-once-simd-iterator)): when a freshly
  loaded block has no `\n`, fall back to `next_line_swar` + `split_semicolon` for that line,
  and add a test with such a line. The rules allow station names of up to 100 bytes.
- **Try [samply](https://github.com/mstange/samply)**, a sampling profiler written in Rust
  that opens the profile in the Firefox Profiler (call tree, inverted call tree, flame graph).
- **Try [hotpath](https://hotpath.rs/)**, a Rust profiler used from the code.
- **Find out what the program waits for on 100M** since Part 005 (~0.6 – 0.8 s of wall time
  without CPU time), for example with the thread states in a System Trace.
- **Write the output through a `BufWriter`** instead of calling `println!` for every
  station (413 `write` system calls now).
- **Find out why the `HashMap` version runs more instructions per cycle** than the `BTreeMap`
  one ([Part 003, change #2](part003-hashmap-bytes-and-lto.md#2-hashmap-instead-of-btreemap-sorted-only-for-the-output)).
