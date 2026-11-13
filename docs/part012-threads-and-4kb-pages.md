# Part 012 — Threads, and what 4 KB pages cost them

**Dates:** 2026-11-12 – 2026-11-13
**Video progress:** stopped at [8:32:45](https://www.youtube.com/watch?v=tCY7p6dVAGE&t=30765s) when this part was published
**Code:** git tag [`part012`](https://github.com/davidhruby/brrr/tree/part012). To get the code exactly as it was for this part: `git checkout part012`

New here? Start with the [Introduction](intro.md).

## What changed

Until now, everything ran on one thread (plus the helper thread for `madvise`). In this part I
split the file between several threads, first on the M4, then on an AWS instance set up like the
machine of the original challenge. Most of the part is about where the time goes once the work
is split, and the answer turned out to be the pages of the file, not the threads.

### 1. Splitting the file into chunks

The idea is the simplest one: cut the mapped file into as many equal chunks as there are
threads, give each thread its own chunk and its own `HashMap`, and merge the maps at the end.

- **The chunk boundaries.** `file_len / number_of_threads` almost never falls on the start of a
  line. Every chunk except the last one is therefore extended to the next `\n` (found with the
  SWAR `find_new_line`, 8 bytes at a time), and the next chunk starts right after it. The last
  chunk goes to the end of the file, without searching.
- **The iterator** gets only its chunk and ends at its end (`if self.at >= self.map.len()`),
  instead of the `end` parameter it had before.
- **The threads** are spawned in `thread::scope`, so they can borrow the mapped file. Each one
  returns its map through an `mpsc::sync_channel`.
- **The merge.** The main thread takes the maps from the channel as they arrive and adds them up
  with `MeasurementStats::update_stats` (the minimum of the minimums, the sum of the sums, and
  so on). The output is the same as before, through a `BTreeMap`.
- **The number of threads** comes from the second argument on the command line, so one build
  can be measured with any number of threads.
- **No helper thread.** `mmap` and one `madvise(MADV_WILLNEED)` on the whole file are done in
  `main` directly.

**Mistakes on the way**, all of the kind the compiler doesn't catch:

- `MMapHelper` was created **inside** the `thread::scope` closure. The threads may only borrow
  what lives longer than the scope (`'scope`), so it didn't compile (`E0597`). The mapping has to
  be created before `thread::scope`.
- `find_new_line(&map[start + chunk_size..])` returns the position **relative to that slice**,
  not to the whole file. Used as an absolute index, the first chunk was 13 bytes long.
- In the last iteration, searching for the `\n` after `start + chunk_size` went past the end of
  the file (`range start index 13795313003 out of range for slice of length 13795312991`). The
  last chunk must not search.
- With every thread getting a `tx.clone()`, the original `tx` stays alive in the main thread,
  and `for one_stats in rx` never ends. It needs a `drop(tx)` before the loop.
- I first used `number_of_threads - 1` worker threads, to leave a core free for the merge. It
  isn't needed: the main thread sleeps in `recv()` until a map arrives, so it takes no core, and
  the merge is 413 stations per thread, a few microseconds.

### 2. On the M4, from the SSD: slower than one thread

The first run of 4 threads on 1B, with the usual `scripts/measure.sh` (the file on the SSD, the
file cache emptied before the run):

| 1B, M4, from the SSD | One thread (Part 011) | 2 threads | 4 threads |
|---|---|---|---|
| Wall time | 9.88 s | 11.12 s | **22.84 s** |
| User time | 9.67 s | 10.39 s | 10.59 s |
| Sys time | | 1.67 s | 4.08 s |

`iostat` shows why, the average size of one read from the SSD and the speed:

| | KB per read | MB/s |
|---|---|---|
| One thread, helper thread with `madvise` | ~300 | ~1,400 |
| 2 threads, no helper thread | ~130 – 280 | ~1,300 |
| 4 threads, no helper thread | **~20** | **~580** |

- **The SSD is the limit in all three:** 13.8 GB at ~1.4 GB/s is ~10 s, and one thread already
  gets there. More threads have nothing to speed up.
- **They can only make it worse.** Without the helper thread asking for big blocks ahead, the
  kernel reads only what a page fault needs plus a little readahead. With 4 threads reading at 4
  places of the file, the reads fell to ~20 KB, and the SSD manages less than half its speed with
  such small reads.
- **The memory filled up:** without `MADV_DONTNEED` behind the threads, the free memory fell
  from ~3 GB to ~60 MB during the run, and the kernel had to drop pages while the program ran.

So on a 16 GB laptop, threads can't be shown on the 1B file from the SSD. 1B doesn't fit on the
RAM disk next to everything else, either (13.8 GB, and the file cache can keep a second copy of
what's read through `mmap`).

### 3. The machine of the original challenge

To have numbers comparable with the challenge, I looked up how it was measured
([README](https://github.com/gunnarmorling/1brc#evaluating-results) and `evaluate.sh` of the 1BRC
repository):

- a Hetzner AX161 server, AMD EPYC 7502P (Zen 2, 32 cores), 128 GB RAM;
- **8 cores**: `numactl --physcpubind=0-7 hyperfine …`. A program could start any number of
  threads, but only 8 could run at once;
- the file on a **RAM disk**, so reading from a disk wasn't measured;
- `hyperfine`, 5 runs, the fastest and the slowest dropped, the mean of the other three.

The closest I have is an AWS `r8a.2xlarge`: AMD EPYC 9R45 (Zen 5, ~4.5 GHz), **8 vCPUs, each a
whole core** (`Thread(s) per core: 1`), 64 GB RAM, and the 1B file in `tmpfs` (see
[Part 009](part009-masked-loads-and-dtlb-misses.md#1-the-question-why-is-jons-code-46-slower-on-the-graviton)).
The cores are newer and faster than the Zen 2 of the challenge, so my seconds won't be the same
as the challenge's seconds. A fair comparison is a winning solution run on the same instance.

### 4. Scaling from 1 to 10 threads

On the `r8a.2xlarge`, 1B in `tmpfs`, one run each with `time` (the run with one thread is from
`perf stat`, the same build):

| Threads | Wall time | User time | Ideal (user time of 1 thread / n) | Efficiency |
|---|---|---|---|---|
| 1 | 11.41 s | 11.11 s | | |
| 3 | 4.03 s | 11.14 s | 3.70 s | 92 % |
| 4 | 3.05 s | 11.17 s | 2.78 s | 91 % |
| 5 | 2.47 s | 11.18 s | 2.22 s | 90 % |
| 6 | 2.08 s | 11.21 s | 1.85 s | 89 % |
| 7 | 1.80 s | 11.24 s | 1.59 s | 88 % |
| **8** | **1.59 s** | 11.22 s | 1.39 s | 87 % |
| 9 | 1.77 s | 11.25 s | | |
| 10 | 1.89 s | 11.24 s | | |

- **Almost linear up to 8.** The user time stays at ~11.2 s, so splitting the work adds almost
  no work, it only spreads it.
- **More than 8 is slower.** With 9 equal chunks on 8 cores, two threads share a core, and the
  others wait for them at the end. Equal chunks only work when their number is exactly the
  number of free cores.
- **But 13 % is lost at 8 threads** (1.59 s against the ideal 1.39 s). That's what the rest of
  this part is about.

### 5. Where the 13 % goes

**The counters for 1 and 8 threads** (`perf stat`, all threads added up):

| | 1 thread | 8 threads |
|---|---|---|
| Cycles | 50.77 billion | 50.85 billion (+0.15 %) |
| Instructions | 181.49 billion | 181.51 billion |
| task-clock (CPU time of all threads) | 11.31 s | 11.46 s |
| Time elapsed | 11.41 s | 1.59 s |
| **CPUs utilized** (task-clock / elapsed) | 0.99 | **7.22** of 8 |
| Page faults | 210,889 | 211,952 |

- **The threads don't get in each other's way.** The same cycles and instructions with 1 and 8
  threads: no fight over the caches or over the memory bandwidth.
- **The loss is waiting:** 7.22 cores busy instead of 8.
- **Even with one thread, the process is ~0.1 s off the CPU** (elapsed − task-clock), which is
  ~6 % of the 8-thread run.
- **~211,000 page faults** on 13.8 GB is one per ~65 KB: the kernel maps 16 neighbouring 4 KB
  pages per fault (fault-around).

**A timeline.** To see *when* the waiting happens, I printed the time since the start of `main`
at a few places to stderr (and the time since the start of each thread at its end, which is ~10
ms behind). 8 threads, in milliseconds since the start of `main`:

| Step | Time |
|---|---|
| `mmap` + `madvise` done | 9 ms |
| threads 0 – 3 done | ~1,230 ms |
| thread 4 done | ~1,249 ms |
| threads 5 – 7 done | ~1,321 – 1,325 ms |
| merge + output done | ~1,325 ms |
| process ended (`real`) | 1,421 ms |

Two losses of about the same size:

- **The last three threads are ~95 ms (7 %) slower**, and in a second run they were the same
  three. All chunks have the same size, and threads aren't pinned to cores, so it depends on the
  **position in the file**: threads 5 – 7 work on its last three eighths.
- **~96 ms after `main` ends**: the process taking down the mapping of 13.8 GB (~3.4 million
  pages of 4 KB) at the exit, the same thing as in
  [Part 010, change #2](part010-build-settings-and-simd-name-comparison.md#2-how-long-the-end-of-the-program-takes).
  It fits the ~0.1 s off the CPU with one thread.

(This build already had ~15 % fewer instructions than the one in the scaling table, ~153.5
against 181.5 billion, and 9.9 s of user time instead of 11.2 s. The binary changed between the
two, so I compare only runs of the same build with each other below; change #7 says more about
why the builds differ. In `one`, the capacity of the table went from 10,000 to 1,000 at about
the same time; measured on its own, it changed nothing, 9.86 against 9.91 s of user time.)

**The counters per thread.** `perf stat --per-thread` works only for a running process (`-p`),
so I used `perf record -s` and `perf report -T`, which prints the counters of every thread at the
end:

| Per thread | Threads 0 – 3 | Thread 4 | Threads 5 – 7 |
|---|---|---|---|
| Instructions | ~19.19 billion | ~19.19 billion | ~19.20 billion |
| Cycles | ~5.45 billion | ~5.54 billion | **~5.89 billion (+8 %)** |

**The same instructions, 8 % more cycles.** It isn't the clock (cycles don't depend on it), and
it isn't more work. The slow threads wait for memory longer. So the same with the TLB events of
[Part 009](part009-masked-loads-and-dtlb-misses.md#5-the-dtlb-misses-jon-complained-about-on-x86):

| Per thread | Threads 0 – 3 | Thread 4 | Threads 5 – 7 |
|---|---|---|---|
| Page walks (`ls_l1_d_tlb_miss.all_l2_miss`) | ~171,000 – 181,000 | ~237,000 | **~487,000 (2.8×)** |
| Misses in the L1 dTLB | ~1.07 – 1.38 million | ~1.18 million | ~1.80 – 1.88 million |
| Coalesced page hits | ~226,000 – 247,000 | ~229,000 | ~254,000 – 282,000 |
| Cycles | ~5.47 billion | ~5.57 billion | ~5.83 billion |

(`perf record` itself makes the run slower, ~1.5 – 2 s instead of 1.4 s, but the counters are
counted, not sampled.)

**The end of the file needs almost 3× more page walks.** A chunk is ~1.72 GB, ~420,000 pages of 4
KB: ~0.4 page walks per page for the first threads, ~1.16 for the last ones.

**A new copy of the same file.** If it's where the kernel put the file in memory, a new copy
should change it. So I deleted the file in `tmpfs`, emptied the file cache
(`echo 3 | sudo tee /proc/sys/vm/drop_caches`) and copied it again:

| | Page walks per thread | Cycles per thread | Spread of the thread ends | Wall time |
|---|---|---|---|---|
| Old copy, threads 0 – 3 | ~175,000 | ~5.47 billion | | |
| Old copy, threads 5 – 7 | ~487,000 | ~5.83 billion | ~95 ms | 1.42 s |
| **New copy, all threads** | **~487,000** | **~5.81 – 5.86 billion** | **~11 ms** | 1.46 s |

The slow threads were gone, but not because the end got faster: **now all of them were slow.**
The same file and the same binary, and only where the copy landed in memory decides. The old copy
had its first ~5/8 in "better" memory, the new one is all in the worse one. Why, I can't see from
inside the virtual machine. A likely guess: in a VM, every page walk goes through two sets of
page tables (the guest's and the hypervisor's, AMD's nested paging), and how the hypervisor backs
each part of the guest's memory (with big or small pages of its own) changes the cost.

For measuring, it means **two copies of the same file in `tmpfs` can differ by ~5 – 7 %** without
any change in the code. Between versions of the code, I keep the same copy and compare cycles.

### 6. Huge pages for `tmpfs`

If the page walks are the cost, backing the file with **2 MB pages** instead of 4 KB ones should
remove them: 13.8 GB is ~6,600 pages of 2 MB against ~3.4 million of 4 KB. `tmpfs` doesn't use
huge pages by default ([Part 009, change #5](part009-masked-loads-and-dtlb-misses.md#5-the-dtlb-misses-jon-complained-about-on-x86)),
but it can be mounted with them:

```sh
sudo umount /mnt/ramdisk
sudo mount -t tmpfs -o size=15G,huge=always tmpfs /mnt/ramdisk
cp /home/ubuntu/brrr/input/1B_measurements.txt /mnt/ramdisk/
grep ShmemHugePages /proc/meminfo
```

`ShmemHugePages: 13473792 kB`: the whole file in 2 MB pages. The same binary, 8 threads:

| | `tmpfs`, 4 KB (new copy) | **`tmpfs`, 2 MB** |
|---|---|---|
| Wall time | 1.457 s | **1.139 s (−22 %)** |
| User time | 10.32 s | 9.07 s (−12 %) |
| Sys time | 0.337 s | **0.012 s** |
| `mmap` + `madvise` | 9 ms | 0 ms |
| Spread of the thread ends | 11 ms | 3 ms |
| End of `main` → end of the process | ~125 ms | **~3 ms** |
| Efficiency of 8 threads (user / 8 / wall) | 89 % | **99.5 %** |

| Per thread | 4 KB, "better" memory | 4 KB, worse memory | **2 MB** |
|---|---|---|---|
| Page walks | ~175,000 | ~487,000 | **~2,900 (−99 %)** |
| Misses in the L1 dTLB | ~1.1 – 1.4 million | ~1.6 – 1.9 million | **~140,000 (−92 %)** |
| Cycles | ~5.47 billion | ~5.81 billion | **~5.09 billion** |

(One thread showed only zeros in `perf report`, a glitch of the recording: in the timeline it
ended with the others.)

All three losses had the same cause, **the 4 KB pages of the file**:

- **the page walks** made the work itself slower: 7 % more cycles than with 2 MB pages in the
  "better" memory, 12 % in the worse one;
- **the page faults** cost 0.3 s of sys time, now ~6,600 faults instead of ~211,000;
- **taking down the mapping** at the exit cost ~125 ms, now 3 ms.

With 2 MB pages, the threads scale almost perfectly: 9.07 s of user time on 8 cores is an ideal
1.134 s, and the run took 1.139 s.

**The page walks don't explain all of it, though.** ~0.72 billion cycles fewer per thread for
~484,000 page walks fewer would be ~1,500 cycles per walk, too much even for a nested walk in a
VM. A part of it is the page faults: `perf record` without `:u` counts the cycles in the kernel
too, and those went away with the 0.33 s of sys time. What's left is something else that depends
on the 4 KB pages, and these counters don't show it. My guess is the hardware prefetcher, which
usually doesn't cross the boundary of a 4 KB page, but I haven't measured that.

**This is a change of the environment, not of the code.** In the default mount (`huge=never`),
`madvise(MADV_HUGEPAGE)` on the mapped file does nothing: the file is already in the file cache
in 4 KB pages, and `mmap` only shows them. Reading the file with `read()` into my own memory with
huge pages would always work, but it costs a copy of 13.8 GB, more than it saves. I don't know how
the RAM disk of the challenge was mounted, so my main numbers stay on the default `tmpfs`, and the
huge pages separately.

### 7. The same code, a different inlining

The runs of this part come from several builds of the same algorithm, and their instructions on
1B differed a lot: 181.5 billion in the scaling table, ~153.5 billion in changes #5 and #6. When I
measured the code again before committing it, on a new start of the instance, it was **170.3
billion with the timeline prints (`eprintln!` with the time) and 202.2 billion without them**.
So the compiler made different code.

**What `one` calls.** The calls inside `one`, from the disassembly of both builds:

```sh
objdump -d --no-show-raw-insn -C /home/ubuntu/brrr/target/release/david | awk '/<david::one>:/,/^$/' | grep call
```

| Called from `one` on every row | With the prints (170 billion) | Without them (202 billion) |
|---|---|---|
| `HashMap::get_mut` | inlined | **`call`** |
| `IdealHasherBuilder::hash_one` | **`call`** | inside `get_mut` |
| `bcmp` (names longer than 16 bytes) | `call` | inside `get_mut` |

(The other calls in the list are cold: `malloc` and `memset` for the table at the start,
`rustc_entry` only for a new station, the panics, and `free` at the end.)

With the prints, the compiler put hashbrown's probing and the comparison of the names into the
loop and called only the small hash function. Without them, it calls the whole `get_mut` on
every row: the key passed in, registers saved and restored, the `Option<&mut>` returned, and
nothing shared with the loop around it. That's the 32 instructions per row.

**How the compiler decides.** For every call, LLVM estimates the cost of inlining it: roughly 5
units per instruction of the called function, minus bonuses for what gets simpler after inlining
(a constant argument that removes a branch, the only call of a function that can then be
deleted), plus penalties (calls inside it, a big `switch`). Below a threshold, it inlines; above
it, it leaves a `call`. With `opt-level = 3` the threshold is around 225 – 275. And it goes
bottom up: first the small functions into the ones that call them, then those into theirs, so
whether `get_mut` fits depends on what was already put into it. `get_mut` is only `#[inline]` in
hashbrown: that makes its body available to other crates and adds a little, but the cost against
the threshold still decides. hashbrown's lookup is just over the edge, and the prints, a few
blocks outside the loop, tipped it.

**Two guesses that were wrong:**

- *`one` gets inlined into the two closures of `main` and grows too big for `get_mut`.* With
  `#[inline(never)]` on `one`, it stayed a function of its own, and nothing changed (1.355 s).
- *The hash inlined into `get_mut` makes `get_mut` too big.* With `#[inline(never)]` on
  `Hash for StationName`, the hash stayed a call, `get_mut` stayed a call too, and it got worse:
  214.1 billion instructions, 1.41 s.

**A bigger threshold.** `-C llvm-args=-inline-threshold=…` sets the threshold for the whole
program; with `lto = "fat"`, it also applies to the inlining across crates (my code and
hashbrown). Each build into its own `--target-dir`:

```sh
RUSTFLAGS="-C llvm-args=-inline-threshold=1000" cargo build --release --bin david --target-dir /home/ubuntu/target-inline1000
```

| 1B, 2 MB pages | Calls in `one` | Instructions (1 thread) | Cycles (1 thread) | 8 threads, `time` |
|---|---|---|---|---|
| Without the prints (committed) | `get_mut` | 202.2 billion | 47.7 billion | 1.36 s |
| `#[inline(never)]` on the hash | `get_mut` | 214.1 billion | 50.1 billion | 1.41 s |
| Threshold 500 | `get_mut` | 188.1 billion | 45.6 billion | 1.29 s |
| With the prints | `hash_one` | 170.3 billion | 43.0 billion | 1.22 s |
| **Threshold 1000** | **none on the hot path** | **158.1 billion** | **38.4 billion** | **1.09 s** |

(The threshold builds still had the `#[inline(never)]` on `one` from the first wrong guess; built
again without it, they ran exactly the same instructions, 158.1 and 188.1 billion.)

At 1000, `get_mut` and the hash are both in the loop, and what's left are `bcmp` for the long
names and `rustc_entry` for a new station. **−20 % against the committed code, without changing
a line of it.** And the wall time of 8 threads follows the cycles of one thread exactly, so
`perf stat` on one thread is a quicker way to compare builds than `time` on eight.

**I don't keep it.** The threshold is for the whole program, not for the hot loop: cold code gets
inlined too, the binary grows, and more code means more room taken in the instruction cache. And
the next change of the code may move the edge again, as the prints did. `#[inline(always)]`
skips the cost estimate and is reliable, but it can only go on my own functions. So the real fix
is to have the hot path in my own code: [Part 013](part013-own-hash-table.md).

So the absolute numbers in this part belong to the build they were measured with. The committed
code: 1 thread 10.68 s (47.7 billion cycles, 202.2 billion instructions), 2 threads 5.37 s (the
same instructions), 8 threads 1.36 s. The conclusions of changes #4 – #6 don't change: every
comparison there is between runs of one binary (1 against 8 threads, 4 KB against 2 MB pages,
the old against the new copy of the file).

### 8. Summary

| 1B, `r8a.2xlarge`, 8 threads | Wall time | User time | Efficiency |
|---|---|---|---|
| 4 KB, old copy | 1.42 s | 9.9 s | 87 % |
| 4 KB, new copy | 1.46 s | 10.3 s | 89 % |
| **2 MB** | **1.14 s** | **9.07 s** | **99.5 %** |
| The code of this part as committed, 2 MB | 1.36 s | 10.78 s | 98 % |
| The same, `-inline-threshold=1000`, 2 MB | 1.09 s | 8.64 s | |

- **Splitting into equal chunks and merging is fine.** The threads don't slow each other down,
  and the merge costs microseconds.
- **The loss of the parallel version was the pages of the file:** page walks, page faults and the
  cleanup at the exit, all of them because of 4 KB pages.
- **Where the file lands in memory changes the time by ~5 – 7 %.** A new copy of the file is a
  new measurement.
- **Whether hashbrown's lookup gets inlined changes the time by 20 %**, and a few prints outside
  the loop decided it. Count the instructions, and when they change, look at the calls in the
  disassembly.
- **On the M4 from the SSD, threads don't help:** the SSD is the limit already with one thread.
- **The winner of the challenge in Java** (thomaswue) took ~1.5 s on 8 cores of a Zen 2. I have
  1.42 – 1.46 s on 8 cores of a Zen 5, which are much faster per core. So I'm still behind the
  best solutions; by how much, only their code on the same instance will show.

## What I learned

## Next steps

1. **The winning Java solution on the same instance**, with `hyperfine --runs 5` and `numactl
   --physcpubind=0-7`, the way the challenge measured.
2. **Small blocks taken from an atomic counter** instead of equal chunks: every thread takes the
   next block of a few MB when it's done with the previous one. With 2 MB pages the threads end
   together anyway, but on the M4 (4 performance and 6 efficiency cores) or with more threads
   than cores, equal chunks make the fast cores wait for the slow ones.
3. **My own hash table** from [Part 011](part011-ideas-from-other-solutions.md#next-steps), now
   for every thread.

## TODO

Things I want to try later. I'm not doing them yet, because Jon may get to them in a later
part of the stream, and I don't want to get ahead of him.

- **The SIMD comparison of names on Zen 5:** ~50.8 cycles per row in this part, against ~45.6 in
  [Part 009](part009-masked-loads-and-dtlb-misses.md#4-jons-whole-program-on-x86) on the same CPU,
  although the code got ~26 % faster on the M4 since then. Find out whether
  [Part 010, change #3](part010-build-settings-and-simd-name-comparison.md#3-comparing-station-names-with-simd-instead-of-memcmp)
  is slower on x86, with the cycles of one thread with and without it.
- **`hash_one` is a call on every row** even in the faster build: `#[inline(always)]` on the
  methods of `IdealHasher` and on `Hash for StationName` would likely put it into the loop.
- **The number of threads from `std::thread::available_parallelism()`** when no argument is
  given, so that `scripts/measure.sh`, which passes no second argument, works again.
- **How the RAM disk of the challenge was mounted**, with huge pages or without: in
  `evaluate.sh` or the issues of the 1BRC repository.
- **`hyperfine` with 5 runs** instead of one `time` per number of threads.
- **The prefetcher and 4 KB pages:** find a counter on Zen 5 that shows whether the hardware
  prefetcher stops at the boundaries of 4 KB pages.
