# Part 001 — Measurements

*The full measurements behind [Part 001 — The first naive version](../part002-first-naive-version.md).
The part itself only has a short summary.*

**Machine:** see [My machine](../notes/my-machine.md). All runs use a `--release` build and were made with
[`scripts/measure.sh`](../../scripts/measure.sh), one run per version and input size. The
totals, the time series and the Instruments trace of the 100M run are from 2026-10-19; the
P-cores vs E-cores runs and the Instruments traces of the 1M run are from 2026-10-18.

## How I measure

`scripts/measure.sh` is a thin wrapper. It only starts standard macOS tools side by side and
saves their raw, unmodified output:

```sh
# a fresh RAM disk with the two small files
diskutil erasevolume APFS RAMDisk $(hdiutil attach -nomount ram://4194304)
cp input/1M_measurements.txt input/100M_measurements.txt /Volumes/RAMDisk/

cargo build --release

scripts/measure.sh david 1M part002
scripts/measure.sh david 100M part002

scripts/measure.sh jonhoo 1M part002
scripts/measure.sh jonhoo 100M part002

hdiutil detach /Volumes/RAMDisk

scripts/measure.sh david 1B part002
scripts/measure.sh jonhoo 1B part002
```

> **Gotcha: the RAM disk isn't guaranteed to stay in RAM.** Its memory is ordinary memory, so
> under memory pressure macOS can swap it out to the SSD like any other. Once I ran the 1B
> version of `david` while the RAM disk was still there, and the next 100M run spent its
> first ~4 s reading the "RAM disk" back from swap (~1.5 GB of swap-ins at ~350 MB/s). So:
> create a fresh RAM disk right before the 1M and 100M runs, and do the 1B runs last.

An optional fourth parameter `e-cores` runs the program under `taskpolicy -c background`,
which keeps it on the efficiency cores (see [P-cores vs E-cores](#p-cores-vs-e-cores)).
The results then go into folders and files ending in `_e-cores`.

The 1M and 100M files are read from the RAM disk (see
[Reading the file from RAM](../notes/my-machine.md#reading-the-file-from-ram)). The 1B file doesn't fit there, so
it's read from the SSD. Every run starts with `sudo purge`, which empties the file cache, so
a second or third run starts from exactly the same state as the first: the 1B file comes
from the SSD, and the 1M and 100M files only from the RAM disk (not from a cached copy left
by the previous run).

The script writes into `measurements/part002/<size>_<binary>/`:

| File | Tool | What it shows |
|---|---|---|
| `time.txt` | `/usr/bin/time -l` | totals for the whole run |
| `top.txt` | `top -pid …`, every second | the process: CPU %, memory, compressed memory, context switches |
| `vm_stat.txt` | `vm_stat 1` | the whole system: page-ins, compression, swap-ins, swap-outs |
| `iostat.txt` | `iostat -w 1 disk0` | how much goes through the SSD |
| `powermetrics.txt` | `sudo powermetrics --samplers cpu_power -i 1000` | frequency and usage of the P- and E-clusters |
| `memory_before_run.txt`, `memory_after_run.txt` | `memory_pressure`, `sysctl vm.swapusage`, `vm_stat` | memory state before and after |

All raw files are in [`measurements/part002/`](../../measurements/part002/). The outputs of both
versions are **identical** on all three files (`diff` prints nothing).

> **Gotcha:** in zsh, `time` is a shell keyword and doesn't accept flags. `time -l ./brrr`
> fails with `command not found: -l`. Use the full path `/usr/bin/time -l` instead.

> **Gotcha:** on macOS, `/usr/bin/time -l` always reports `swaps 0` and
> `block input operations 0`, even when the process swaps a lot or reads from the SSD. The
> kernel doesn't fill in these counters. Use `vm_stat` and `iostat` instead.

## Totals

From `time.txt`:

| Input | Version  | Wall time | User + sys | Peak memory (RSS) | Peak memory footprint | Instr. per row | IPC  |
|-------|----------|-----------|------------|-------------------|-----------------------|----------------|------|
| 1M    | `david`  | 0.15 s    | 0.14 + 0.00 s | **15.6 MB**    | 15.0 MB               | ~2,046         | ~4.0 |
| 1M    | `jonhoo` | 0.13 s    | 0.12 + 0.00 s | **1.8 MB**     | 1.2 MB                | ~1,863         | ~4.2 |
| 100M  | `david`  | 12.54 s   | 12.34 + 0.18 s | **1.38 GB**   | 1.38 GB               | ~2,008         | ~4.0 |
| 100M  | `jonhoo` | 10.62 s   | 10.44 + 0.17 s | **1.9 MB**    | 1.2 MB                | ~1,822         | ~4.3 |
| 1B    | `david`  | **266.1 s** | 120.7 + 15.2 s | **7.2 GB**  | **13.8 GB**           | ~2,165         | ~3.9 |
| 1B    | `jonhoo` | **106.7 s** | 104.7 + 1.8 s  | **1.9 MB**  | 1.2 MB                | ~1,839         | ~4.3 |

Instructions per row = `instructions retired` ÷ rows. IPC (instructions per cycle) =
`instructions retired` ÷ `cycles elapsed`.

What the numbers say:

- **My memory grows with the file, Jon's doesn't.** Mine needs about as much memory as the
  file is big: 15 MB for the 13.8 MB file, 1.38 GB for the 1.38 GB file, 13.8 GB (peak
  footprint) for the 13.8 GB file. That's `read_to_string` loading everything. Jon's stays
  at ~1–2 MB whatever the size.
- **Mine runs ~10 % more instructions per row** (~2,000–2,170 vs ~1,820–1,870), mostly from
  the `Vec` allocated per line and the double map lookup. On 100M that makes mine ~18 %
  slower.
- **On 1B mine is 2.5× slower**, and almost all of that is waiting, not computing: 266.1 s of
  wall time, but only 136 s of CPU time. See [1B over time](#1b-over-time).
- **The cost per row is stable.** Jon's version needs ~1,820–1,870 instructions per row on
  every input size. So the 100M file is a reliable stand-in for the 1B file when comparing
  versions: just multiply the time by ~10.
- **The CPU isn't stalling, it's just doing too much.** An IPC of ~4 is high: the core isn't
  waiting for data, it simply executes ~1,800–2,000 instructions to parse something like
  `Hamburg;12.0`. Making it faster means doing less work per row, not feeding the CPU
  faster.
- **1M is too small to say much.** A run takes ~0.13–0.15 s, and the difference between the
  versions (0.14 vs 0.12 s of user time) is at the resolution of `time`.

## 1M and 100M over time

1M is too short for per-second sampling: `top` didn't catch a single sample. The table is
for 100M:

| | `david` 100M | `jonhoo` 100M |
|---|---|---|
| Memory (`top`) | **1,317 MB** from the first sample on | **~1.1 MB** the whole time |
| Compressed memory (`top` CMPRS) | 0 B | 0 B |
| Swap-ins / swap-outs (`vm_stat`) | 0 / 0 | 0 / 0 |
| CPU (`top`) | ~100 % of one core | ~100 % of one core |
| P-cluster (`powermetrics`) | 100 % active at **4.04–4.06 GHz** | 100 % active at **4.04–4.05 GHz** |
| Context switches (`time -l`, voluntary + involuntary) | 2 + 222 | 2 + 100 |

What the time series show:

- **The SSD isn't involved.** `iostat` shows only background noise (mostly under 30 MB/s).
  The file comes from the RAM disk.
- **Two very different ways to read the same file.** The RAM disk doesn't show up in
  `iostat`, but `vm_stat` counts its pages as *page-ins* (one page = 16 KB) when they're
  read into the file cache:

  | Second | 1 | 2 | 3 | 4 | 5 | 6 | … | 10 |
  |---|---|---|---|---|---|---|---|---|
  | `david` (page-ins) | **85,715** | 1,952 | 4,008 | 53 | 494 | 261 | … | 31 |
  | `jonhoo` (page-ins) | 12,050 | 9,718 | 8,490 | 9,827 | 8,273 | 7,749 | … | 8,485 |

  85,715 × 16 KB ≈ 1.40 GB: mine pulls the whole file into memory in the first second
  (`read_to_string`) and then only computes. Jon's pulls ~7,700–12,000 pages per second
  (~130–200 MB/s) for the whole run, exactly as fast as it processes the rows. That's
  streaming.
- **No memory pressure on 100M.** 1.3 GB fits easily, so there's no compression and no swap
  for either version.
- **Both ran on a P-core the whole time.** The P-cluster was 100 % active at ~4.05–4.1 GHz in
  every sample during the run (see also [Which cores did it run on?](#which-cores-did-it-run-on)).

## P-cores vs E-cores

macOS doesn't let a program choose a specific core. What it does allow is lowering the
priority: a process with the *background* QoS class runs only on the E-cores. So I ran my
version on 100M twice, once normally and once with `taskpolicy -c background`:

```sh
scripts/measure.sh david 100M part002
scripts/measure.sh david 100M part002 e-cores
```

Raw output: [`100M_david/`](../../measurements/part002/100M_david/),
[`100M_david_e-cores/`](../../measurements/part002/100M_david_e-cores/). The output is identical.

| | Normal (P-cores) | `taskpolicy -c background` (E-cores) | Ratio |
|---|---|---|---|
| Wall time | 12.33 s | **57.04 s** | **4.6×** |
| Instructions | 200.9 billion | 201.1 billion | same |
| CPU cycles | 50.0 billion | 67.0 billion | 1.34× |
| CPU frequency = CPU cycles ÷ CPU time (user + sys) | **4.07 GHz** | **1.18 GHz** | **3.4×** |
| IPC = instructions ÷ CPU cycles | **4.0** | **3.0** | **1.34×** |
| Involuntary context switches | 77 | 3,495 | |

The same code doing the same work gets 4.6× slower, and the slowdown splits cleanly into two
parts:

```text
4.6× = 3.4× (lower clock) × 1.34× (fewer instructions per cycle)
```

- **The clock is only ~1.2 GHz, not the ~2.9 GHz the E-cores can do.** Background QoS doesn't
  just mean "E-cores", it also means "save energy": macOS keeps the E-cluster at a low clock
  for this kind of work. `powermetrics` shows the E-cluster at ~1.1–1.2 GHz almost the whole
  run.
- **The E-cores are narrower.** Fewer execution units and smaller caches (64 KB L1d instead
  of 128 KB, 4 MB L2 instead of 16 MB), so ~25 % fewer instructions per cycle even at the same
  clock.
- **More interruptions.** Background work shares the E-cores with the rest of the system's
  background work, so it gets preempted ~45× more often.

**Did it really stay on the E-cores?** `powermetrics` shows how busy each core was in every
second. The sum over a cluster is ~100 % for the core time my single thread used, plus
whatever else the system was doing:

| Second (sample) | E-cores CPU 0–5, sum | P-cores CPU 6–9, sum |
|---|---|---|
| Normal run, typical second | 9–48 % (background) | **103–114 %** (my thread) |
| E-cores run, typical second | **109–155 %** (my thread) | **2–13 %** (background, almost all on CPU 6) |

In the E-cores run, the E-cluster sum stays at **~110 % or more in every second** of the run
(only the last sample, in which the run ended, is lower), and
the P-cores sit nearly idle: CPU 7–9 at ~0 % most of the time. In a few seconds (e.g. the
3rd, 4th and 18th) the P-cores woke up too, but the E-cluster sum went *up* at the same time
(to 190–240 %), so that was other work on the system, not my thread moving. If my thread had
moved to a P-core, the E-cluster sum would have dropped to the background level.

So yes: under `taskpolicy -c background` the thread stays on the E-cores. It still jumps
between the six E-cores (every second each of them shows some activity), just like it jumps
between the four P-cores in a normal run. Moves within a cluster are cheap, because its cores
share one L2 cache.

### Exact check with Instruments

`powermetrics` only shows how busy each core was, so the conclusion above is an inference.
Instruments' **System Trace** records every scheduling event: which thread ran on which core,
from when to when. These events aren't sampled: the kernel logs every change with a
nanosecond timestamp, so no move between cores can slip through. I recorded the whole 1M run
(~0.1 s normally, ~0.6 s on the E-cores) and the whole normal 100M run. A trace of the 100M
run on the E-cores (~57 s) would be several hundred MB, so I skipped that one.


```sh
mkdir -p measurements/part002/1M_david_traces
sudo purge
xcrun xctrace record \
  --template 'System Trace' \
  --output measurements/part002/1M_david_traces/system-trace.trace \
  --target-stdout /dev/null \
  --launch -- ./target/release/david /Volumes/RAMDisk/1M_measurements.txt

mkdir -p measurements/part002/1M_david_e-cores_traces
sudo purge
xcrun xctrace record \
  --template 'System Trace' \
  --output measurements/part002/1M_david_e-cores_traces/system-trace.trace \
  --target-stdout /dev/null \
  --launch -- /usr/sbin/taskpolicy -c background ./target/release/david /Volumes/RAMDisk/1M_measurements.txt
```

`xctrace --launch` doesn't search `PATH`, so `taskpolicy` needs its full path. The traces
(7.5 MB and 11 MB) aren't committed (`*.trace` is in `.gitignore`), but anyone can record
their own with the commands above. I read the `cpu-state` table out of each trace with
`xcrun xctrace export --input <trace> --xpath '/trace-toc/run[@number="1"]/data/table[@schema="cpu-state"]'`
and summed up, per core, how long my thread was *Running* there:

| | Normal | `taskpolicy -c background` |
|---|---|---|
| Time running | **0.120 s** | **0.572 s** (4.8× longer) |
| On P-cores | **100 %**, all of it on CPU 8 | 0.3 % (1.5 ms) |
| On E-cores | 0 % | **99.7 %**, spread over CPU 0–5 |
| Running intervals | 171 | 1,201 |
| Moves to another core | **0** | 55 (36 within a cluster, 19 between P and E) |

- **Normally, the whole 0.12 s ran on a single P-core** without moving once. The 171 running
  intervals are short breaks for hardware interrupts, after which the thread came back to the
  same core.
- **On the E-cores, the program's work ran only on E-cores**, but with a small exception
  that `powermetrics` couldn't show: all 1.5 ms on the P-cores happened in the **first ~9 ms**
  after the program started, at the priority *User Interactive – 46* instead of
  *Background – 4*. After that, every running interval was on an E-core at background
  priority. My guess is that this is the start of the process (the kernel loading the program
  after `taskpolicy` hands over to it), which runs at a temporarily raised priority. I haven't
  verified this.
- **More moves on the E-cores** (55 vs 0). Background work shares the E-cores with the rest
  of the system's background work, so the thread gets moved around more.

**The whole 100M run, normal.** The same recording on the 100M file, with every app I didn't
need closed (only the terminal was open), and without touching the Mac during the run. The
trace is 38 MB:

```sh
mkdir -p measurements/part002/100M_david_traces
sudo purge
xcrun xctrace record \
  --template 'System Trace' \
  --output measurements/part002/100M_david_traces/system-trace.trace \
  --target-stdout /dev/null \
  --launch -- ./target/release/david /Volumes/RAMDisk/100M_measurements.txt
```

This time I read the `thread-state` table, which has a *Core* column for every interval of
my thread. I count it with my own small tool,
[`src/analyze/xctrace/`](../../src/analyze/xctrace/), in its `cpu_usage` mode:

```sh
xcrun xctrace export --input measurements/part002/100M_david_traces/system-trace.trace \
  --xpath '/trace-toc/run[@number="1"]/data/table[@schema="thread-state"]' > tmp-thread-state-100M.txt
cargo run --release --bin analyze-xctrace -- tmp-thread-state-100M.txt cpu_usage
```

For every *Running* interval it adds the duration to the core it ran on, and it counts a
move whenever the core differs from the previous running interval (and a P↔E move when one
is a P-core and the other an E-core). The XML stores every value only once (`id="…"`) and
later rows only point to it (`ref="…"`), so the tool first collects all definitions and then
looks each row's values up. The result:

| State of my thread | Time | Intervals |
|---|---|---|
| Running | 12.21 s (99.8 %) | 14,915 |
| Blocked (waiting for something) | 0.004 s (~0 %) | 26 |
| Interrupted (hardware interrupts) | 0.03 s (0.2 %) | 14,841 |
| Runnable (ready, but no free core) | ~0 s | 25 |
| Preempted (another thread took the core) | ~0 s | 38 |

| Core | Running time | Share |
|---|---|---|
| CPU 6 (P) | 2.87 s | 23.5 % |
| CPU 7 (P) | 3.15 s | 25.8 % |
| CPU 8 (P) | 3.08 s | 25.2 % |
| CPU 9 (P) | 3.12 s | 25.5 % |
| CPU 0–5 (E) | 0 s | 0 % |

- **All of it ran on the P-cores, not a single microsecond on an E-core.**
- **The thread moves between the P-cores at a steady pace:** 39 moves in 12.2 s, about
  **3 per second**. So it stays on one core for ~⅓ s, and over the whole run the time ends up
  split evenly, ~25 % per P-core. The regular rhythm looks like the scheduler deliberately
  rotating the thread over the cluster, maybe to spread the heat. That's my guess, I haven't
  verified it.
- **The program almost never waited:** 99.8 % of the time it was running. The tool skips
  everything before the first *Running* interval: that's a single ~0.7 s *Blocked* interval
  in every trace (0.68–0.72 s), while `xctrace` holds the process until recording starts,
  so it's Instruments, not my program.
- **`powermetrics` couldn't have shown any of this.** Once per second it only showed the
  P-cluster 100 % busy. The rhythm of the moves only shows up in an event trace.

> **Check it in Instruments.**
>
> 1. Open the trace: `open measurements/part002/100M_david_traces/system-trace.trace`.
> 2. In the list of tracks on the left, expand the `david` process and select its
>    **Main Thread**. Its *Activity* lane shows the thread state over time.
> 3. The bottom panel switches to **Thread States**: one row per interval, with the columns
>    *Start Time*, *Duration*, *State*, **Core**, *Running Time*, *Wait Time* and *Priority*.
> 4. Click into the timeline at different moments and the list jumps there. The *Core*
>    column shows which core the thread was on at that moment.
>
> Expected: only *CPU 6–9 (P Core)* in the Core column, never an E-core, and *Running* almost
> all the time.

![Instruments: Thread States of the 100M run](../images/part002-instruments-thread-states.png)

The list shows a pattern that explains the many *Interrupted* intervals in the table above:
almost exactly **every 1 ms** the thread is interrupted for a fraction of a microsecond
(e.g. at 02.139 697, 02.140 700, 02.141 697, …) and then keeps running on the same core.
That's very likely Instruments itself: its time profiler takes a sample of the call stack
every 1 ms (see [Where the time goes in the code](#where-the-time-goes-in-the-code)), and
taking a sample interrupts the core. So the measuring adds ~12,000 tiny interrupts to a
12-second run, though together they cost only ~0.03 s. In other parts of the run the core
in the list is different (e.g. *CPU 9* at 10.8 s), which is the thread moving between the
P-cores.

**Something to try later: pinning to one core.** On macOS I can only choose the type of core
(and only E, not P), never a specific core. Linux can do that: `taskset -c 3 ./program` or
`sched_setaffinity` in the code keeps a thread on exactly one core. Running the same versions
on a Linux server (for example a cloud machine like the
[official 1BRC setup](../intro.md#the-challenge)) would show what changes when the thread never moves:
whether the jumps between cores cost anything measurable, and how stable the times get. I
may try that in a later part.

## Where the time goes in the code

The System Trace of the 100M run (see [Exact check with Instruments](#exact-check-with-instruments))
also contains a **time profile**: every 1 ms it records the call stack of my thread, i.e.
which function the CPU was in and which functions called it. Counting the samples per
function shows where the 12 seconds go.

To get the numbers from the command line, I export the `time-profile` table and count the
samples with the same tool as above, in its `code_profile` mode (how to see the same thing
in Instruments is right below the table):

```sh
xcrun xctrace export --input measurements/part002/100M_david_traces/system-trace.trace \
  --xpath '/trace-toc/run[@number="1"]/data/table[@schema="time-profile"]' > tmp-time-profile-100M.txt
cargo run --release --bin analyze-xctrace -- tmp-time-profile-100M.txt code_profile
```

The tool counts two things for every function:

- **Self:** samples where the CPU was *in* this function (the first frame of the stack).
- **Inclusive:** samples where the function was *anywhere* on the stack, i.e. the function
  plus everything it called.

It gives exactly the same numbers as Instruments (12,099 samples = 12.10 s of CPU time):

| Function | Self | Inclusive | What it is |
|---|---|---|---|
| `_platform_memcmp` + `DYLD-STUB$$memcmp` | **37.4 %** (3.93 s + 0.59 s) | | comparing two strings: the `BTreeMap` lookups |
| `david::main` | 28.2 % (3.41 s) | 71.7 % (8.67 s) | my loop itself, with the inlined `BTreeMap` code |
| `memchr_aligned` + `CharSearcher::next_match` | **19.6 %** (1.65 s + 0.71 s) | | `line.split(';')`: searching for the `;` |
| `malloc`, `free` and friends | ~9 % | | the `Vec` and `String` allocated for every line |
| `f32::from_str` + `parse_number` | 4.2 % (0.26 s + 0.24 s) | 4.2 % | parsing the temperature |
| `from_utf8` | 0.8 % | | checking the file is valid UTF-8 (`read_to_string`) |

What it says:

- **A third of the time is spent comparing station names.** `memcmp` is the C library
  function that compares two blocks of memory; Rust uses it whenever it compares two
  strings. A `BTreeMap` is a sorted tree, so finding `"Hamburg"` means walking down the
  tree and comparing `"Hamburg"` with the keys on the way, about ten comparisons for 413
  stations. And my code looks up every line **twice** (`contains_key`, then `get_mut`), so on
  100M rows that's ~200 million lookups and billions of `memcmp` calls. `DYLD-STUB$$memcmp`
  is the small jump from my binary into the system library, paid on every one of those
  calls.
- **A fifth is `split(';')`**, which scans every line for the `;` and collects the parts into
  a `Vec`.
- **Parsing the numbers is cheap** (4 %), and so is reading the file: in the *System Calls*
  view of the same trace, `read_to_string` gets the whole 1.38 GB file in just **2** `read`
  calls that take 139 ms together (~9.9 GB/s from RAM), about 1 % of the run.

> **Check it in Instruments.**
>
> 1. Open the trace: `open measurements/part002/100M_david_traces/system-trace.trace`.
> 2. Select the `david` process in the list of tracks on the left.
> 3. In the bottom panel, open the view menu (it shows e.g. *Profile: System Calls*) and pick
>    **Time Profiler → Profile** (⌘7).
> 4. Click the **Self Weight** column to sort by it. *Weight* is the inclusive time, *Self
>    Weight* the self time, both in ms (1 sample = 1 ms). A blue person icon means the code
>    is in my binary (including the parts of Rust's standard library compiled into it), a
>    brown building icon means a system library.
>
> Expected: `_platform_memcmp` 3.93 s (32.5 %), `david::main` 3.41 s self / 8.67 s
> inclusive, `memchr_aligned` 1.65 s, and so on, the same as in the table above.
>
> To see **who called a function**, click its row: the panel on the right shows its
> *Inverted Callers*. For `_platform_memcmp` they go straight to
> `__rust_begin_short_backtrace`, without `david::main` (see the note below).
>
> For the **`read` calls**: pick **System Call Trace → Profile: System Calls** (⌘5) in the
> same menu. `read` should be at the top with ~139 ms.

So the things to fix first aren't the obvious ones. It's not the float parsing or the file
reading, but the double map lookup (a single `get_mut` does it once, see [What I learned #3](../part002-first-naive-version.md#3-looking-up-a-station-contains_key--get_mut-entry-or-just-get_mut)), the tree itself (a `HashMap`
compares the name once instead of ~10 times) and the `split` into a `Vec` (`split_once`).

> **Why `david::main` is only 71.7 % inclusive, not ~100 %.** Almost everything happens
> inside my `main`, yet in many samples it's missing from the stack. The normal (not
> inverted) call tree in Instruments shows it best:
>
> ![Instruments: call tree of the 100M run](../images/part002-instruments-call-tree.png)
>
> Under `__rust_begin_short_backtrace` (12.10 s, 100 %) there is `david::main` with 8.67 s,
> but next to it, as if Rust's runtime had called them directly, also `_platform_memcmp`
> with **2.92 s**, `malloc`, `next_match`, `free` and others. Only 1.01 s of the 3.93 s in
> `memcmp` ended up correctly under `david::main`. These are *leaf* functions (they call
> nothing else), so they don't save a frame pointer on the stack, and walking the stack
> skips their direct caller. Self time is exact; the inclusive time of the function that
> calls such a leaf function comes out too low.

## 1B over time

| | `david` | `jonhoo` |
|---|---|---|
| Memory (`top`) | grows to **13 GB** in ~10 s | **~1.1 MB** the whole time |
| Of that compressed or swapped (`top` CMPRS) | **~7.7–12 GB** | 0 B |
| Compressions / decompressions (`vm_stat`) | 2.50 M / 1.80 M pages | 7,643 / 3,181 pages |
| Swap-outs / swap-ins (`vm_stat`) | **1.53 M / 1.38 M pages** (≈ 25 GB / 23 GB) | 0 / 1,085 pages |
| Data moved through the SSD (`iostat`) | **~61.5 GB** | ~13.5 GB |
| Swap file (`sysctl vm.swapusage`) | 1.2 GB → 1.6 GB used, grew from 2 GB to 3 GB | ~1.2 GB, no change |
| Voluntary context switches (`time -l`) | **754,079** | 134 |
| P-cluster (`powermetrics`) | 39–88 % active, see the phases below | **~100 %** active at ~4.05 GHz |

The SSD numbers add up: `jonhoo` reads the 13.8 GB file once. `david` reads it once too, and
then moves another ~48 GB in and out of swap (1.53 M + 1.38 M pages × 16 KB). Swap-outs and
swap-ins are counted system-wide, but nothing else was running, so they're almost all mine.

**Jon's version** is boring in the best way: the whole run looks like the 100M run, only
10× longer. ~100 % CPU on a P-core, ~1.1 MB of memory, and the SSD reading a steady
~125 MB/s, as fast as the rows get processed. Almost all of it is user time (104.7 s user vs
1.8 s sys): reading the file costs the kernel little, the time goes into our own code (UTF-8
validation, a `String` allocation per line, float parsing and B-tree lookups).

**My version** goes through three phases, clearly visible in `top`, `iostat` and
`powermetrics`:

| Phase | Time | CPU (`top`) | Memory | SSD (`iostat`) | What happens |
|---|---|---|---|---|---|
| 1. Reading | 0–10 s | 10–65 % | 0 → 13 GB, of which up to 12 GB compressed | **~2.2 GB/s** | `read_to_string` reads the file at full SSD speed. macOS compresses and swaps out the memory almost as fast as it fills up. |
| 2. Waiting | ~10–70 s | **mostly 4–12 %** | 13 GB, ~10 GB compressed | ~290 MB/s on average | The process is mostly in the `stuck` state, waiting for swapped-out pages to come back. The P-cluster is only ~40–50 % active. |
| 3. Computing, slowly | ~70–266 s | 40–85 % | 13 GB, ~9.3–9.9 GB compressed | ~140 MB/s on average | The loop over the lines. It computes, but keeps stopping for pages that have to be decompressed or swapped back in. The P-cluster is ~80–88 % active at ~4.0–4.2 GHz. |

My guess for phase 2 is that `read_to_string` checks that the whole 13.8 GB is valid UTF-8
before it returns the `String`. That's a second pass over all the memory, and most of it is
already compressed or on the SSD. I haven't verified this.

So: **yes, it swapped, a lot**, even though `/usr/bin/time -l` reported `swaps 0`. Only
7.2 GB of the 13.8 GB was ever in RAM at once (RSS); the rest was compressed or on the SSD.
The 754,079 voluntary context switches are the process stopping to wait for those pages.
How long such a run takes also depends on how much RAM is free when it starts, so the wall
time of `david` on 1B varies a lot between runs: an earlier run, with less free RAM at the
start, took 365.5 s with the same ~136 s of CPU time. Jon's doesn't vary.

## Which cores did it run on?

`/usr/bin/time -l` doesn't report the clock frequency, but it gives the cycles and the CPU
time. Dividing one by the other gives a rough average frequency for a run (see
[Clock frequency](../notes/my-machine.md#clock-frequency)):

| Run | CPU cycles | CPU time (user + sys) | Average CPU frequency |
|---|---|---|---|
| `david`, 100M | 50,606,727,903 | 12.34 + 0.18 s | **≈ 4.04 GHz** |
| `jonhoo`, 100M | 42,825,290,416 | 10.44 + 0.17 s | **≈ 4.04 GHz** |
| `david`, 1B | 560,077,079,377 | 120.68 + 15.24 s | **≈ 4.12 GHz** |
| `jonhoo`, 1B | 430,103,429,350 | 104.71 + 1.83 s | **≈ 4.04 GHz** |

The CPU cycle count includes the time spent in the kernel, so it has to be divided by user
**plus** sys time. With user time alone, `david` on 1B would come out at 4.64 GHz, which is
more than the chip can do.

~4.1 GHz is far above the ~2.9 GHz of the E-cores, so both programs ran on the **P-cores**
whenever they were computing, and the Air didn't slow down noticeably even over four and a half minutes.
`powermetrics` confirms it second by second. That makes the `david` vs `jonhoo` comparison
fair.

## Compiling for the M4: `target-cpu=native` (2026-10-20)

Normally Rust compiles for a generic CPU of the platform, so the binary runs on any Apple
Silicon Mac. `-C target-cpu=native` tells the compiler to use everything the CPU it's
running on can do. Jon tries this in the video and mentions it won't help much, because the
standard library comes **precompiled** for a generic CPU. With `-Zbuild-std` (only in
nightly Rust) Cargo compiles the standard library again, with the same flags.

I compared three builds of my version on 100M from the RAM disk:

```sh
# A: the normal build (stable Rust 1.97)
cargo build --release

# B: nightly Rust 1.101, compiled for the M4, standard library included
rustup toolchain install nightly
rustup component add rust-src --toolchain nightly-aarch64-apple-darwin
RUSTFLAGS="-C target-cpu=native" cargo +nightly build --release -Zbuild-std --target aarch64-apple-darwin

# C: nightly Rust 1.101 for a generic CPU, to separate the two changes in B
cargo +nightly build --release --target-dir target/nightly-generic
```

`target-cpu` can't be set in `[profile.release]`, only through `RUSTFLAGS` or
`.cargo/config.toml`. `-Zbuild-std` needs `--target`, so build B ends up in
`target/aarch64-apple-darwin/release/` instead of `target/release/`.

**Time** with hyperfine (5 runs, 1 warm-up), A against B:

| | Mean ± σ | Min … max |
|---|---|---|
| A: normal build | 12.582 ± 0.121 s | 12.430 … 12.706 s |
| B: `target-cpu=native` + `-Zbuild-std` | 12.498 ± 0.186 s | 12.329 … 12.784 s |

B is 0.7 % faster on average, but that's less than the spread of either build and the
ranges overlap: **no measurable difference**.

**Instructions and cycles** from one run each with `/usr/bin/time -l` show what the compiler
actually changed:

| | A: stable 1.97 | C: nightly 1.101, generic | B: nightly 1.101 + `target-cpu=native` + `build-std` |
|---|---|---|---|
| Instructions | 200.8 billion | **183.6 billion** | 182.0 billion |
| Instructions per row | ~2,008 | ~1,836 | ~1,820 |
| CPU cycles | 49.6 billion | 48.2 billion | 49.4 billion |
| IPC | 4.05 | 3.81 | 3.69 |
| CPU time (user + sys) | 12.18 s | 11.96 s | 12.11 s |

Split into the two changes:

| Change | Instructions | CPU cycles |
|---|---|---|
| A → C: newer compiler | **−8.6 %** | −2.7 % |
| C → B: `target-cpu=native` + `-Zbuild-std` | **−0.9 %** | +2.4 % (noise of a single run) |

What it shows:

- **Compiling for the M4 does almost nothing here**, as Jon said. On top of the precompiled
  standard library, ~46 % of the time goes into system libraries of macOS (`memcmp`,
  `malloc`, `free`, see [Where the time goes in the code](#where-the-time-goes-in-the-code)),
  which no Rust flag can change. The rest is slowed down by the algorithm (the `BTreeMap`,
  the double lookup), not by the choice of instructions.
- **Fewer instructions isn't the same as less time.** The newer compiler removed 17 billion
  instructions, but the cycles went down by less than 3 %, because the IPC dropped at the same
  time. What counts is `CPU time = CPU cycles / CPU frequency` (see
  [Clock frequency](../notes/my-machine.md#clock-frequency)). Removing instructions only helps if they're the ones
  the CPU is actually waiting on.
- **Change one thing at a time.** Comparing only A and B, I would have credited
  `target-cpu=native` with the whole 9 % drop in instructions. Build C shows that almost all
  of it comes from the newer compiler.
