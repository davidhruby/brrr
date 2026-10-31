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
with input that varies like the real data.

### Too many instructions: hashing

In the Time Profiler of the 100M run, `HashMap::get_mut` took ~28 % of the time, and ~18 %
was only computing the hash. The default hasher of `HashMap` (SipHash) is built to resist
attacks with crafted keys, not for speed, so it simply runs a lot of instructions for every
station name.

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
