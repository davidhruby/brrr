# Part 010 — Build settings and comparing station names with SIMD

**Dates:** 2026-11-08 – 2026-11-10
**Video progress:** stopped at [5:02:58](https://www.youtube.com/watch?v=tCY7p6dVAGE&t=18178s) when this part was published
**Code:** git tag [`part010`](https://github.com/davidhruby/brrr/tree/part010). To get the code exactly as it was for this part: `git checkout part010`

New here? Start with the [Introduction](intro.md).

## What changed

### 1. `codegen-units = 1` and `panic = "abort"`

Two common settings for a faster release build, added to the profile that already had
`lto = "fat"`:

```toml
[profile.release]
lto = "fat"
debug = "full"
codegen-units = 1
panic = "abort"
```

- **`codegen-units = 1`**: after LTO, the code is split again into 16 parts by default, which
  are compiled in parallel. With 1 it stays one part, so the compiler sees all of it when it
  decides about inlining, at the cost of a slower build.
- **`panic = "abort"`**: a panic ends the program right away instead of unwinding the stack,
  so the code for cleaning up on the way (*landing pads*) goes away. `Drop` doesn't run on a
  panic any more; the panic message is still printed.

The code in [`src/david/main.rs`](../src/david/main.rs) is the same as in
[Part 007](part007-hashing.md), so it compares directly with it. Measured with
[`scripts/measure.sh`](../scripts/measure.sh) on the M4. The output is identical to Part 007 on
both files. The raw output of these runs is gone: I later measured change #3 into the same
folder, and it overwrote them. Only the numbers below are left.

| | Part 007, 100M | **Part 010, 100M** | Part 007, 1B | **Part 010, 1B** |
|---|---|---|---|---|
| Wall time | 1.88 s | 1.85 s (−1.6 %) | 13.64 s | 14.01 s (+2.7 %) |
| User time | 1.34 s | 1.32 s | 13.53 s | 13.37 s (−1.2 %) |
| Instructions | 19.86 billion | 20.19 billion (+1.7 %) | 207.3 billion | 210.4 billion (+1.5 %) |
| Cycles | 5.57 billion | 5.64 billion (+1.3 %) | 58.42 billion | 58.01 billion (−0.7 %) |

- **No measurable difference.** The cycles and the user time move by ~1 % either way, within
  what they move between runs on the M4.
- **The wall time of 1B moved by ~0.5 s between runs of the same build:** an earlier run gave
  13.55 s, this one 14.01 s, with about the same cycles (58.57 vs 58.01 billion). The extra
  time isn't computing, it's spent outside the work (see change #2).
- **~3 instructions more per row** (~207 → ~210): with one codegen unit, the compiler decided
  a bit differently about inlining or the layout of the loop, not better.
- **`panic = "abort"` had nothing to remove in the hot loop**: it hardly calls anything that
  can panic.

I added both at once, so the two can't be told apart. I'm keeping them anyway: they don't make
it slower, and they're the usual settings for a program like this. On another CPU (x86 on
Linux) the compiler may decide differently, but I'd expect a difference of a few instructions
per row there too.

### 2. How long the end of the program takes

When a program ends, the kernel has to take down its memory, including the mapping of the
13.8 GB file. `time` waits until the process is really gone, so this cleanup counts, even
though the output was printed long before. A known trick from 1BRC is to leave the cleanup out
of the measurement: do the work in a child process, print the result in the parent and let it
end while the child is still cleaning up. Calling `std::process::exit` right after printing
only skips Rust's destructors (`Drop`); the kernel's cleanup happens at the exit anyway.

To see how big it is, I measured the time inside `main` with `std::time::Instant` (taken at
the start of `main`) and printed `elapsed()` to stderr twice: after the loop over the rows,
and as the last line of `main`. One run on 1B:

| 1B, one run | Time |
|---|---|
| End of the work (before printing) | 13.461 s |
| End of `main` (after printing) | 13.461 s |
| `real` from `time` | 14.02 s |
| **Outside `main`** | **~0.56 s (~4 %)** |

- **Printing the 413 lines takes 0.6 ms.**
- **~0.56 s are outside `main`.** Starting the process before `main` takes milliseconds, and my
  destructors after `main` are only the map and a few `Vec`s, so nearly all of it is the end of
  the process in the kernel: taking down the mapping of the file. `MMapHelper` has no `Drop`,
  so it never calls `munmap` itself; the kernel does it at the exit.
- **It changes a lot between runs.** The same build gave 13.55 s and 14.01 s of wall time with
  about the same cycles (change #1). The 1B file is on the SSD (`input/`), so how much of it is
  in the page cache at the end, and has to be released, may differ from run to run.

A few runs more would show whether it's usually ~0.1 or ~0.5 s. Only then is it clear whether
the trick with a child process is worth trying.

### 3. Comparing station names with SIMD instead of `memcmp`

**Why.** In the Time Profiler of the 1B run, **`_platform_memcmp` took 5.04 s, 35.8 % of the
time**. It comes from the lookup in the `HashMap`: the keys were `Vec<u8>`, so `get_mut(station)`
compares `[u8] == [u8]`, and the standard library does that with `memcmp`. That's a call into
`libsystem_platform` for every row, to compare names that are mostly 3 – 16 bytes long.

**A new key type.** I can't give `[u8]` my own `PartialEq` (the orphan rule: neither the trait
nor the type is mine), so the key is a new type:

```rust
pub struct StationName<'a> {
    rest: &'a [u8], // from the start of the name to the end of the mapped file
    len: usize,     // the length of the name
}
```

The name is `&rest[..len]`, but `rest` goes on to the end of the file. That's the point: a
16-byte SIMD load from the start of the name is then always allowed, even for a 5-byte name,
because the bytes after it (`;12.3\nNext...`) are still in the file.

- **`PartialEq`:** if the lengths differ, it's `false`. Otherwise, when the name fits into 16
  (or 32, or 64) bytes and both `rest`s are at least that long, it loads that many bytes of
  both names, compares them with `simd_eq`, and ignores the bytes past `len` with a mask
  (`indices < len`, where `indices` is a constant `0, 1, 2, …`). The result is
  `(equal | !valid).all()`. Names longer than 64 bytes and names in the last few bytes of the
  file fall back to `==`.
- **`Hash`:** only the bytes of the name, `&rest[..len]`, the same way as `[u8]` hashes them.
  The hash of every station is the same as before, so the table is filled the same way, and
  only the comparison changed.
- **No copies:** the keys point into the mapped file instead of to a `Vec` on the heap, so a
  new station no longer allocates. For the output, `str::from_utf8_unchecked` turns the name
  into a `&str` without copying.
- **`StationTemperatureIterator` returns `StationName`** directly, because it knows the map and
  the position of the line.

**A dead end first.** In the first version, the iterator built the key as
`StationName { rest: station, len: station.len() }`, with `rest` being only the name. So
`rest.len() == len`, and the condition "the name fits into 16 bytes and `rest` has at least 16
bytes" was true only for names of exactly 16 bytes. All other names went to the `==` fallback,
which is `memcmp` again. Three variants measured that way (1B):

| 1B | Change #1 (`memcmp`) | `u8x64` | `u8x16` | 16 / 32 / 64 |
|---|---|---|---|---|
| Wall time | 14.01 s | 14.74 s | 14.62 s | 14.90 s |
| Instructions | 210.4 billion | 217.1 billion | 223.9 billion | 229.0 billion |
| Cycles | 58.01 billion | 63.26 billion | 62.53 billion | 63.83 billion |

All slower, and the profile still had `_platform_memcmp` at 33.7 %. The disassembly of `main`
showed why: the compiler turned the conditions into a `ccmp x26, #0xf` and a branch to
`bl _memcmp` for every name shorter than 16 bytes. The SIMD comparison was compiled, but never
ran. What these runs measured was `memcmp` plus the checks in front of it.

**The fix:** `rest: &self.map[self.at..]`, built before `self.at` moves to the next line. The
output is identical to change #1 on both files.

| | Change #1, 100M | **SIMD, 100M** | Change #1, 1B | **SIMD, 1B** |
|---|---|---|---|---|
| Wall time | 1.85 s | 1.32 s | 14.01 s | **10.17 s (−27 %)** |
| User time | 1.32 s | 0.94 s | 13.37 s | 9.86 s (−26 %) |
| Instructions | 20.19 billion | 19.86 billion (−1.6 %) | 210.4 billion | 207.9 billion (−1.2 %) |
| Cycles | 5.64 billion | 4.02 billion (−29 %) | 58.01 billion | **43.79 billion (−24.5 %)** |
| Instructions per cycle | 3.6 | 4.9 | 3.6 | 4.7 |
| Page faults | 84,203 (Part 007) | 84,203 | 842,027 (Part 007) | 842,027 |

Raw output in [`measurements/part010/`](../measurements/part010/), measured with the
16 / 32 / 64 version of `PartialEq`.

- **About the same number of instructions, a quarter fewer cycles.** The comparison didn't get
  much shorter in instructions; what's gone is the call. In the disassembly of the dead-end
  build, around every `bl _memcmp`, the code in `main` saved and restored four vector registers (`stp` / `ldp q16 … q19`), and `memcmp` itself
  first decides by the length how to compare. Now it's two loads, a compare and a reduction
  inside the loop, without leaving it.
- **The page faults didn't change** (842,027, the same as in Part 007). The keys now point
  into the mapped file, and the helper thread releases the old blocks with `MADV_DONTNEED`, but
  that costs no extra faults.
- **`_platform_memcmp` is gone from the profile.** The time of the 1B run, 10.16 s in `main`:

| Where | Time | Share |
|---|---|---|
| `HashMap` lookup and insert (hash, probing, comparing the names) | 6.93 s | 66.5 % |
| `StationTemperatureIterator::next` | 1.71 s | 16.4 % |
| `MeasurementStats::update` | 536 ms | 5.1 % |
| `main` itself | 442 ms | 4.2 % |
| `parse_temperature` | 347 ms | 3.3 % |
| `NextBlockPreLoadInfo::request_next_block_load` | 188 ms | 1.8 % |

The `HashMap` is now two thirds of the time. It shows 0 s of self time because the hasher, the
probing and the comparison are all inlined into it.

The idea for this change comes from the
[barrcodes.dev write-up of 1BRC](https://barrcodes.dev/posts/1brc/#benchmarking-a-compliant-solution).

### 4. Only `u8x16`, without the 32- and 64-byte branches

Names longer than 16 bytes are rare, so the two extra branches of change #3 were mostly checks
that didn't pay off. `PartialEq` now has only the 16-byte comparison and the `==` fallback,
which handles every name longer than 16 bytes. The output is identical on both files. Raw
output in [`measurements/part010-PartialEq_u8x16/`](../measurements/part010-PartialEq_u8x16/).

| | 16 / 32 / 64, 100M | **`u8x16`, 100M** | 16 / 32 / 64, 1B | **`u8x16`, 1B** |
|---|---|---|---|---|
| Wall time | 1.32 s | 1.49 s | 10.17 s | **9.93 s** |
| User time | 0.94 s | 0.93 s | 9.86 s | 9.57 s (−3 %) |
| Instructions | 19.86 billion | 19.59 billion (−1.4 %) | 207.9 billion | 206.6 billion (−0.6 %) |
| Cycles | 4.02 billion | 3.88 billion (−3.5 %) | 43.79 billion | **42.86 billion (−2.1 %)** |
| Page faults | 84,203 | 84,203 | 842,027 | 842,031 |

- **2 – 3.5 % fewer cycles on both files.** That's close to how much runs differ on the M4,
  but both files moved the same way, and there are fewer instructions too: two branches less
  in every comparison.
- **The wall time of 100M went up** (1.32 → 1.49 s) with the same user time (0.94 vs 0.93 s).
  The difference is waiting outside the work, which has differed between 100M runs since
  Part 005 (see TODO), not the comparison.

I'm keeping `u8x16`: it's simpler and at least as fast. Against change #1, the 1B run went
from 14.01 s to 9.93 s and from 58.01 to 42.86 billion cycles (−26 %).

### 5. Where the keys in the `HashMap` point to

Since change #3, every key points to the first line of its station in the mapped file, and the
helper thread releases every 512 MB block with `MADV_DONTNEED` once the reading has moved past
it. So I wanted to know how many blocks and pages the 413 keys come from.

For that I wrote a small program,
[`src/analyze/station_name/main.rs`](../src/analyze/station_name/main.rs)
(`cargo run --release --bin analyze-station-names`). It fills the same `HashMap` with
`StationName` keys from the 1B file, and then counts the keys per 16 KB page, both by the
absolute address (`address / 16 KB`) and by the offset from the start of the map. It also
finds the line where the last of the 413 stations shows up for the first time, using the list
of names in `input/1B_unique_station_names.txt`.

```
First station name pointer: 12884901888 (0x300000000)
Last station name pointer: 12884945249 (0x30000A961)
Address diff: 43361 (pages: 2.6465454)
Page index with number of keys:
786432 => 391
786433 => 20
786434 => 2
MMap index with number of keys:
0 => 391
1 => 20
2 => 2
```

| Page of the file | Bytes | Keys |
|---|---|---|
| 0 | 0 – 16 KB | 391 |
| 1 | 16 – 32 KB | 20 |
| 2 | 32 – 48 KB | 2 |

- **All 413 keys are in the first 3 pages of the file, 48 KB.** The last new station shows up
  43,361 bytes (2.65 pages) into the 13.8 GB file. Every station is common, so each one
  appears within the first few thousand lines.
- **The map starts exactly at a page boundary.** `0x300000000` divided by 16 KB is 786,432 with
  no remainder, so page 786,432 of the address space is page 0 of the file, and the two tables
  are the same. `mmap` always returns an address aligned to a page; the remainder of 0 is what
  proves it here, the same counts in both tables alone wouldn't.
- **All the keys are in block 0**, out of the ~27 blocks of 512 MB.
- **`MADV_DONTNEED` on block 0 costs at most 3 page faults.** It's called once per block. On
  Linux, it removes the pages from the process's page table, but the data stays in the page
  cache, because it belongs to the file; the next comparison, on the very next row, maps each
  of the 3 pages back with a minor fault, without reading the disk. On macOS, `man madvise`
  describes `MADV_DONTNEED` only as lowering the priority of the pages, and since they're
  touched again right away, they stay. The page faults agree: 842,027 with the keys in `Vec`s
  (Part 007), 842,027 – 842,031 with the keys in the map.
- **The names are close together.** 48 KB fit into the L1 data cache of the M4 and need only
  3 entries in the TLB, while the names in `Vec<u8>` were spread over the heap.

I thought about leaving the first few pages out of `MADV_DONTNEED`, but there's nothing to
save: 3 minor faults are microseconds out of ~10 s. And the number of pages would only fit
this file: with 10,000 stations (the most the rules allow), the last new name shows up much
further into the file, and the keys spread over more pages.

## What I learned

## Next steps

## TODO

Things I want to try later. I'm not doing them yet, because Jon may get to them in a later
part of the stream, and I don't want to get ahead of him.

- **A smaller table on Linux:** with 4 KB pages, the 16,384 buckets of
  `with_capacity(10_000)` spread over ~130 – 160 pages and cause ~0.75 dTLB misses per row.
  Measure `HashMap::new()` (512 buckets, ~20 KB, 5 pages) there; on the M4, the capacity was
  7 % faster. On Zen 5 (change #5), halving the misses changed nothing, so probably not worth
  much; the Graviton may still differ.
- **Why Jon's code with `Vec<u8>` had only 0.004 dTLB misses per row on the Graviton**
  ([Part 008](part008-short-strings.md)), with the same 131,072-bucket table that gives ~1 per
  row on Zen 5, and mine ~0.75 with a table 8× smaller. Either the generic `dTLB-load-misses`
  counts something else on the Graviton, or something there I don't see yet.
- **`StrVec` with many more stations.** With 413 names everything is in the L1 cache, so keeping
  the names inside the keys saves nothing (changes #1 and #2). With 10,000 stations (the most
  the rules allow), the names on the heap wouldn't fit in L1 any more, and it might pay off.
- **Handle lines longer than 64 bytes in the SIMD iterator** ([Part 006, change #4](part006-finding-the-end-of-a-line.md#4-finding-the--and-the-n-of-several-lines-at-once-simd-iterator)): when a freshly
  loaded block has no `\n`, fall back to `next_line_swar` + `split_semicolon` for that line,
  and add a test with such a line. The rules allow station names of up to 100 bytes.
- **Try [samply](https://github.com/mstange/samply)**, a sampling profiler written in Rust
  that opens the profile in the Firefox Profiler (call tree, inverted call tree, flame graph).
  It demangles Rust's v0 symbol names itself, so it would work without the nightly build that
  Instruments needs for readable names.
- **Try [hotpath](https://hotpath.rs/)**, a Rust profiler used from the code: functions are
  marked with `#[hotpath::measure]` (and `main` with `#[hotpath::main]`), and it reports their
  time and allocations, and whether they're CPU- or I/O-bound.
- **Find out what the program waits for on 100M** since Part 005 (~0.6 – 0.8 s of wall time
  without CPU time, see [Part 005](part005-parsing-temperature.md#3-parsing-without-branches) and [Part 006](part006-finding-the-end-of-a-line.md#4-finding-the--and-the-n-of-several-lines-at-once-simd-iterator)), for example with the thread states in a System Trace. One
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
