# Performance

*Notes mostly for myself: what I learned about finding out why code is slow. The examples
come from [Part 004](../part004-mmap.md) and the work after it.*

## A profiler shows where, not why

A sampling profiler (Instruments Time Profiler, `samply`, `perf` on Linux) stops the program
many times per second and records which function it's in. The result says **where** the
time goes. It doesn't say **why** that code takes so long, and the fix depends on the why.
So when something stands out in the profile, the next step is to find out which of these
it is:

| Why | What it looks like | How to check it |
|---|---|---|
| **Waiting** (SSD, page faults, locks) | The thread isn't running at all: wall time is much longer than CPU time | `real` − (`user` + `sys`) from `time`; thread states in an Instruments System Trace |
| **Branch mispredictions** | The same code is fast on regular input and slow on varied input | Benchmark with a fixed input vs random inputs; branch-miss counter in Instruments CPU Counters |
| **Cache misses** (waiting for memory) | The time grows when the data stops fitting in a cache level | Vary the data size and look for jumps (as in [TLB](tlb.md)); cache-miss counters in CPU Counters |
| **Too many instructions** | The code is simply doing a lot of work | Instructions per row; read what the function really does |

A sampling profiler only sees a thread while it's running, so waiting usually doesn't show
up in it at all: the program just takes longer than the samples add up to.

## Examples from this project

### Waiting: `mmap` on the 1B file

In [Part 004, change #1](../part004-mmap.md#1-mmap-instead-of-bufreader), the CPU time
dropped to 27.7 s but the wall time stayed at 47 s. The ~19 s difference was the main thread
waiting on page faults. Making the code itself faster wouldn't have helped at all; what
helped was a helper thread that takes the page faults ahead of the main thread (47 s → 28 s).

### Branch mispredictions: parsing the temperature

A hand-written parser with a loop over the bytes and a `match` on each byte (`.`, `-` or a
digit), measured with criterion (details in
[Part 005, change #2](../part005-parsing-temperature.md#2-a-benchmark-for-the-temperature-parser)):

| | Fixed input | 10,000 random inputs (per input) |
|---|---|---|
| `parse::<f32>()` | 4.5 – 4.8 ns | 9.0 ns |
| Loop with `match` | 1.3 – 1.9 ns | 2.8 ns |
| Fixed positions, no branches | 0.85 ns | 1.1 ns |

With a fixed input, the branch predictor learns how many times the loop runs and whether a
`-` comes, and the loop is fast. With random temperatures, the length and the sign change
every time, the predictor often guesses wrong, and each wrong guess costs roughly 10 – 15
cycles: the same code is slower than on any fixed input. The version without branches reads the digits from
fixed positions counted from the end and turns the comparisons into numbers (`as i16`, 0 or
1) instead of using them in an `if`. There's nothing left to predict, so random input is
almost as fast as a fixed one.

The lesson: a benchmark with one fixed input can hide the real cost. Always also measure
with input that varies like the real data. And the other way round: uniformly random input
can make it look worse than it is. In the whole program on the real 1B file, the branch
counters showed that the loop parser mispredicted only ~0.1 times per row, and the version
without branches saved ~6 % of the cycles, not 4× (see
[Part 005, change #3](../part005-parsing-temperature.md#3-parsing-without-branches)).

### Too many instructions: hashing

In the Time Profiler of the 100M run, `HashMap::get_mut` took ~28 % of the time, and ~18 %
was only computing the hash. The default hasher of `HashMap` (SipHash) is built to resist
attacks with crafted keys, not for speed, so it simply runs a lot of instructions for every
station name.

## Micro-benchmarks vs the whole program

A micro-benchmark measures one small function, millions of times. It's fast to run and
precise, but its numbers don't carry over to the whole program one to one. How to set up the
two tools I used is in [Measurements](measurements.md#benchmarks-of-single-functions-criterion)
(criterion) and [Measurements](measurements.md#benchmarks-with-hardware-counters-micromeasure)
(micromeasure).

### Two tools, the same parsers

The three temperature parsers, per temperature on 10,000 random inputs, on the Mac:

| | criterion | micromeasure (median) |
|---|---|---|
| `parse::<f32>()` | 9.0 ns | 7.5 ns |
| Loop with `match` | 2.0 ns | 2.5 ns |
| Fixed positions, no branches | 1.17 ns | 1.20 ns |

The same order and the same scale, but the loop and `parse::<f32>()` differ by up to ~20 %
between the tools. Each benchmark file is a separate program, and the measuring loops around
the parser differ slightly (the type of the sum, how it's kept alive with `black_box`), so
the compiler can inline and schedule the code differently. Even moving the loop parser from
`main.rs` to `lib.rs` changed its criterion result from 2.8 ns to 2.0 ns. The version without
branches came out the same everywhere.

The spread shows the same thing. In micromeasure, the loop had its 95th percentile 10 % above
the median and a median absolute deviation of 4.6 %; the version without branches 2.7 % and
1.0 %. Code with data-dependent branches reacts more to its surroundings, probably also to
what the branch predictor still remembers from the previous samples (my guess).

### What a micro-benchmark promises, and what the program gets

The version without branches was 2.5× faster than the loop in the benchmark (2.8 → 1.1 ns),
but the whole program on 1B rows got only from 25.4 s to 24.7 s. That looks like the
benchmark was wrong, but it wasn't:

- **The absolute numbers fit.** 1.7 ns saved per temperature × 1 billion rows ≈ 1.7 s. The
  program saved 6.4 cycles per row (CPU counters), ~1.5 s of CPU time at ~4.4 GHz, and the Time
  Profiler showed 1.05 s less in the main thread. The same order.
- **The parser is only a small part of the program** (~7.5 % of the main thread's time).
  Even an infinitely fast parser couldn't make the program more than ~7.5 % faster
  (*Amdahl's law*). A big speedup of a small part is a small speedup of the whole.
- **Uniformly random input made the loop look worse than it is.** On the real data the loop
  mispredicted only ~0.1 times per row, much less than on the benchmark's random
  temperatures, so the real gain was a bit smaller than the benchmark's.
- **The wall time on 1B** also depends on the disk and the helper thread, so it shows even
  less of the gain than the CPU time.

### What the counters of the micro-benchmark show

On Linux, micromeasure adds the CPU counters to every benchmark (per temperature, on an AWS
Graviton3, details in [Measurements](measurements.md#with-the-counters-on-linux-aws-graviton3)):

| | `parse::<f32>()` | Loop with `match` | No branches |
|---|---|---|---|
| Cycles | 44.4 | 19.2 | 8.7 |
| Instructions per cycle | 3.02 | 2.98 | 4.14 |
| Mispredicted branches | 0.72 | 0.42 | 0.0001 |
| Frontend stall | 6.6 % | 16.8 % | 0.02 % |
| Backend stall | 22.5 % | 6.1 % | 44.7 % |

A CPU core is a pipeline with two halves. The **frontend** prepares the instructions: it
predicts where the program goes, fetches the instructions and decodes them. The **backend**
executes them: it waits until their inputs are ready (results of earlier instructions, data
from memory), computes them and retires them. A *frontend stall* means the backend could
work but gets no instructions, typically after a mispredicted branch (everything fetched on
the wrong path is thrown away) or an instruction cache miss. A *backend stall* means the
instructions are there but can't run yet: waiting for data, for the result of the
instruction before, or for a free execution unit.

- **The version without branches really has no mispredictions**, ~1 per 10,000
  temperatures. Its 4 branches per temperature are bounds checks and the benchmark's loop,
  which are always predicted right.
- **The loop mispredicts 0.42 times per temperature on the random input**, against ~0.1 per
  row on the real data in the whole program: direct proof that uniformly random input
  exaggerates the mispredictions. At ~12 cycles each that's ~5 of its 19.2 cycles, and it
  shows as the 16.8 % frontend stall.
- **Without branches, the bottleneck moved to the backend** (44.7 % backend stall, the
  frontend now delivers smoothly). Not cache misses, there are almost none. My guess: with
  the inputs as a `Vec<Vec<u8>>`, the pointer to each temperature has to be loaded first and
  only then its bytes, two dependent loads of ~4 cycles each, and in a function of only ~36
  instructions that waiting is a big share. With the temperatures in one continuous buffer, as
  in the real file, it would be different. I haven't verified that.

### Lessons

- Before optimizing a function, look at **its share of the whole program** in a profile. A
  2.5× faster function at 7.5 % of the time saves at most ~4.5 %.
- Feed the benchmark **input that looks like the real data**, ideally a sample of the real
  file, not uniformly random values. Fixed input hides mispredictions, uniform random input
  exaggerates them.
- Compare the **absolute time saved** (ns per call × number of calls) with the program, not
  the speedup factor.
- Keep the wrapper around every measured function **the same** (`black_box` placement, type of
  the sum), and don't trust differences of 10 – 20 % between different benchmark programs.
- `#[inline(never)]` (handy for the profiler) changes the result: the loop parser came out at
  4.7 ns instead of 2.8 ns in the benchmark, and the whole program on 1B at 25.55 s instead of
  24.70 s. Remove it before measuring.
- In the end, **the whole program decides.**

## Counting branch mispredictions in Instruments

`/usr/bin/time -l` reports instructions and cycles, but not branches. Instruments can count
them with the **CPU Counters** instrument (on Linux, `perf stat -e branches,branch-misses`
does the same):

1. New trace from the **CPU Counters** template. In its recording options, switch
   *Configuration* from *Guided* to **Manual**, keep *Sample By: Time*.
2. With `+` under *Events and Formulas*, add:
   - **Instructions (FIXED_INSTRUCTIONS)** and **Cycles (FIXED_CYCLES)**,
   - **`INST_BRANCH`**: all executed branches, **`INST_BRANCH_COND`**: the conditional ones
     (`if`, `match`, the end of a loop),
   - **`BRANCH_MISPRED_NONSPEC`**: all mispredicted branches,
     **`BRANCH_COND_MISPRED_NONSPEC`**: the conditional ones.

   `NONSPEC` means only branches that really executed are counted, not the ones on a
   speculative path the CPU threw away. A green dot next to each event means the chip can
   count all of them at once (the M4 has only a few programmable counters).
3. **File → Save As Template…**, for example as "Branches". Then it works from the command
   line too:

   ```sh
   sudo purge
   xctrace record --template 'Branches' --output measurements/part005/branches-v2-1B.trace --launch -- target/release/david input/1B_measurements.txt
   ```

4. In the trace, the table at the bottom has a column per counter. Select the **Main Thread**,
   so a helper thread doesn't mix in, and widen the columns, otherwise the numbers are cut off.
   Divided by the number of rows, they give branches and mispredictions per row.

To compare with an older version without touching the current code, build it in a second
working tree: `git worktree add ../brrr-9f77420 9f77420` and
`cargo build --release --manifest-path ../brrr-9f77420/Cargo.toml`.

### Where the mispredictions happen: not with Instruments

The counts above are totals. To see in which functions the mispredictions happen, the
counters have to be sampled by the event (*Sample By: Events*, for example every 100,000
`BRANCH_MISPRED_NONSPEC`), so that every sample carries the call stack. On my Mac (M4,
macOS 26.6) this doesn't work: with any counter in *Events and Formulas* the recording fails
with `samplers differ`, and without any counter the samples have no call stacks (details in
[Part 005, change #3](../part005-parsing-temperature.md#3-parsing-without-branches)).
Splitting a time-sampled trace by function doesn't help either: it only copies the share of
time. On Linux, `perf record -e branch-misses` with `perf report` / `perf annotate` does
exactly this, and on an AWS Graviton instance it showed that ~62 % of the mispredictions come
from SipHash hashing the station names (branches on the length of the name) and most of the
rest from the end of the byte-by-byte search for the `;` (see
[Part 005, change #3](../part005-parsing-temperature.md#3-parsing-without-branches)).

A sample taken at a mispredicted branch doesn't always point at the branch itself: it often
lands a few instructions later (*skid*). In my case, ~19 % of the samples were on two
instructions of the parser that can't mispredict at all, right after the loop searching for
the `;`. `perf annotate` shows the instructions around, so it's easy to see which branch
comes just before.

## Browsing a `perf` profile with call stacks (Linux)

`perf report` without `--stdio` opens an interactive browser in the terminal, like in Jon's
stream. To see who calls what in it, the samples need call stacks:

```sh
sudo perf record -e branch-misses:u -c 100000 --call-graph dwarf -o perf.data ./target/release/david input/1B_measurements.txt > /dev/null
sudo perf report -i perf.data
```

- **`--call-graph dwarf`, not just `-g`.** Plain `-g` follows frame pointers, which a Rust
  release build doesn't keep, so the stacks come out cut off. With `dwarf`, every sample
  saves a piece of the stack, and `perf` unwinds it later with the debug info
  (`debug = "full"`). That also shows the inlined functions, marked `(inlined)`. The file is
  bigger: ~8 KB per sample, 112 MB for ~13,800 samples.
- **To sample by time instead of an event**, leave out `-e … -c …` and use `-F 999`
  (samples per second). The default ~4,000 per second makes huge files with `dwarf`.
- **Two columns:** *Children* is the function with everything it calls, *Self* only the
  function itself. `--no-children` sorts by *Self*; `-g caller` shows the tree from the root
  down.

Keys in the browser:

| Key | What it does |
|---|---|
| ↑ ↓ | move between rows |
| `+` or Enter | expand / collapse the call tree of a row |
| `E` / `C` | expand / collapse everything |
| `a` | annotate: the instructions of the function, with the share of samples |
| `/` | filter by name |
| `?` | help |
| `q` | back / quit |

In the annotate view (`a`), *Percent: local period* is the share of the samples inside this
function only. `s` shows the source lines between the instructions, Enter on a jump follows
it, `o` shows the addresses, `h` the help. Arrows next to a jump show its direction (↑ back,
like the end of a loop; ↓ forward).

> **Gotcha: hex addresses instead of names.** Two kinds showed up on the Graviton:
>
> - **Above `main`** (who called it), addresses like `0x20af14a9dea0af` or `0x77e1f3c6fd305b`
>   with an odd first byte. Most likely return addresses signed with *pointer
>   authentication* (PAC): Graviton3 supports it and Ubuntu's glibc uses it, and `perf`
>   didn't strip the signature. A bigger piece of the stack (`--call-graph dwarf,65528`, the
>   maximum) and `sudo apt install libc6-dbg` can help, but for finding where the time goes
>   it doesn't matter.
> - **Inside `main`**, addresses like `0xaf14a9df7ec8`: single instructions in inlined code
>   that `perf` couldn't name. Their last digits match the offsets in `perf annotate`
>   (`…7ec8` was the `ldrb` right after the loop searching for the `;`).

> **Gotcha: `perf stat -d -d` fails on the Graviton** with `Failure to read '#slots'Cannot
> find metric or group 'Default'`. `perf` tries to compute its default derived metrics and
> can't. Listing the events by hand works (`-e cycles,instructions,branches,branch-misses,
> L1-dcache-loads,L1-dcache-load-misses,…`, the full list is in
> [Part 005, change #3](../part005-parsing-temperature.md#3-parsing-without-branches)).

> **Gotcha: more events than counters.** A small instance has only a few counters. With more
> events, `perf` takes turns between them and prints how much of the run each one counted
> (e.g. `(14.21%)`); the numbers are scaled up from that, so they're estimates, and ratios of
> two events counted at different times can come out odd (Jon's run showed iTLB misses at
> 223 % of all iTLB accesses). For exact numbers, measure two or three events at a time.

## Reading a profile of Rust code on macOS

- Instruments doesn't demangle Rust's v0 symbol names (they start with `_R`). On stable Rust
  the old format can't be turned on; a nightly build with
  `-Z unstable-options -C symbol-mangling-version=legacy` gives names Instruments can read.
  Or use `samply`, which demangles them itself.
- With `debug = "full"` in `[profile.release]`, the profile shows inlined functions too, so
  the call tree matches the source even though the calls are gone from the machine code.
- **Invert Call Tree** puts the functions with the most *self* time at the top: where the
  time is actually spent, not the chain of calls leading there.
- The time per source line is approximate: samples belong to instructions, and instructions
  map to lines only roughly.
- `#[inline(never)]` keeps a small function visible in the profile as its own entry, but
  remove it before measuring speed, because it stops the compiler from inlining it.

## A stable clock on Linux (for bare metal)

Linux changes the CPU clock with the load, by a rule called the *governor*. `powersave` or
`schedutil` keep the clock low when idle and raise it step by step under load, so the first
milliseconds of a run are slower. `performance` keeps it at the maximum all the time:

```sh
cat /sys/devices/system/cpu/cpu0/cpufreq/scaling_governor
cpupower frequency-info
sudo cpupower frequency-set -g performance
```

- **It matters on a physical machine,** for short runs and micro-benchmarks most. A run of 10 –
  60 s on 1B hardly notices the ramp-up, which takes milliseconds.
- **On a normal AWS instance there's nothing to set.** The hypervisor controls the clock; the
  virtual machine usually has no cpufreq driver (`scaling_governor` doesn't exist, `cpupower`
  reports no or an unknown driver). AWS allows it only on some bigger instance types and on
  `metal`. The clocks I measured there (from `cycles` / user time) were already at the
  maximum: ~3.37 GHz on Sapphire Rapids, ~4.5 GHz on Zen 5. A run at ~4.25 GHz on Zen 5 came
  from the host, which the governor inside the VM can't change.
- **Turbo makes the clock move too**: it depends on the temperature and on how many cores are
  busy. For comparable numbers on bare metal, it's often turned off, so the clock stays at the
  base frequency. How depends on the driver: `intel_pstate` on Intel
  (`echo 1 | sudo tee /sys/devices/system/cpu/intel_pstate/no_turbo`), the `boost` file with
  `acpi-cpufreq` or `amd-pstate` (`echo 0 | sudo tee /sys/devices/system/cpu/cpufreq/boost`).
- **Either way, compare cycles, not seconds.** Instructions per row don't depend on the clock
  at all, cycles per row much less than the seconds (only the waiting for memory, which takes
  the same nanoseconds at any clock, turns into more cycles at a higher one).
- On macOS none of this can be set; the system controls the clock.
