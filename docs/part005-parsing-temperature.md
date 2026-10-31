# Part 005

**Dates:** 2026-10-30 – 2026-10-31
**Video progress:** stopped at [1:57:26](https://www.youtube.com/watch?v=tCY7p6dVAGE&t=7046s) when this part was published
**Code:** git tag [`part005`](https://github.com/davidhruby/brrr/tree/part005). To get the code exactly as it was for this part: `git checkout part005`

New here? Start with the [Introduction](intro.md).

## What changed

### 1. Temperatures as integers in tenths, parsed by hand

The temperature isn't parsed with `parse::<f32>()` any more. A small function walks the bytes
from the end and builds the value in tenths (`12.3` → `123`, `-0.5` → `-5`): it skips the
dot, stops at a `-` and flips the sign, and adds every digit (`num - b'0'`) times 1, 10 or 100.
The rules fix the format (an optional `-`, one or two digits, a dot and exactly one decimal),
so a general float parser isn't needed. The statistics are integers now too: `i16` for min /
max, `i64` for the sum and `u32` for the count. Only the output divides by 10.

Before:

```rust
struct MeasurementStats {
    min: f32,
    max: f32,
    sum: f32,
    count: u32
}
```

```rust
let temperature = unsafe { str::from_utf8_unchecked(fields.next().unwrap()).parse::<f32>().unwrap() };
let station = fields.next().unwrap();
```

```rust
let mean = sum / (*count as f32);
println!("{location}={min:.1}/{mean:.1}/{max:.1}");
```

After:

```rust
struct MeasurementStats {
    min: i16,
    max: i16,
    sum: i64,
    count: u32
}
```

```rust
let (Some(temperature), Some(station)) = (fields.next(), fields.next()) else {
    panic!("unexpected line: {:?}", String::from_utf8_lossy(fields.next().unwrap()));
};
let t = parse_temperature(temperature);
```

```rust
pub fn parse_temperature(temperature: &[u8]) -> i16 {
    let mut t: i16 = 0;
    let mut mul = 1;
    for &d in temperature.iter().rev() {
        match d {
            b'.' => continue,
            b'-' => {
                t *= -1;
                break;
            },
            num => {
                t = t + (num - b'0') as i16 * mul;
                mul *= 10;
            }
        }
    }
    t
}
```

```rust
println!(
    "{location}={:.1}/{:.1}/{:.1}",
    (*min as f64) / 10.,
    (*sum as f64) / 10. / (*count as f64),
    (*max as f64) / 10.
);
```

Measurements after this change (raw output from commit
[`9f77420`](https://github.com/davidhruby/brrr/tree/9f77420/measurements/part005)):

| | `david` 100M | `david` 1B |
|---|---|---|
| Wall time | 3.4 s | 25.4 s |
| CPU time (user + sys, both threads) | 2.7 s | 26.7 s |
| Peak memory footprint | 1.9 MB | 8.0 MB |
| Swapped to the SSD | 0 | 0 |
| Instructions per row | ~402 | ~410 |
| Page faults | 84,204 | 842,025 |

**1B: 27.6 s → 25.4 s (−8 %).** About 75 fewer instructions per row (1B ~484 → ~410). The
output is identical to Part 004 on both files.

On 100M, the CPU time dropped too (2.8 s → 2.7 s), but the wall time went up (2.8 s → 3.4 s):
the program waited ~0.8 s, and there were almost twice as many involuntary context switches
(2,741 instead of 1,580). RustRover was starting at the same time as this run, but the
waiting showed up again in the 100M run of change #3, so it may not be the reason. I haven't
found out yet what the program waits for.

### 2. A benchmark for the temperature parser

To compare parsers without running the whole 1B file, I added a benchmark with
[criterion](https://docs.rs/criterion) (`cargo bench --bench parse_temperature`, in
[`benches/parse_temperature.rs`](../benches/parse_temperature.rs)). A benchmark can only
call code from a library, and the rules want all the code of the program in one file, so
`src/david/lib.rs` includes `main.rs` as a module instead of moving code out of it:

```rust
#[allow(dead_code)]
#[path = "main.rs"]
pub mod app;
```

Three parsers are compared.

`parse::<f32>()`, as in Part 004:

```rust
pub fn parse_f32(temperature: &[u8]) -> f32 {
    unsafe { str::from_utf8_unchecked(temperature).parse().unwrap() }
}
```

`parse_temperature` from change #1, the loop over the bytes with a `match`:

```rust
pub fn parse_temperature(temperature: &[u8]) -> i16 {
    let mut t: i16 = 0;
    let mut mul = 1;
    for &d in temperature.iter().rev() {
        match d {
            b'.' => continue,
            b'-' => {
                t *= -1;
                break;
            },
            num => {
                t = t + (num - b'0') as i16 * mul;
                mul *= 10;
            }
        }
    }
    t
}
```

`parse_temperature_v2` (only in `lib.rs` for now), with no loop and no `if`. It reads the
digits from fixed positions counted from the end: the tenths are always last, the ones
always two bytes before them, and the tens are at the start only when there are two digits.
The comparisons are turned into numbers (`as i16`, 0 or 1) instead of being used in an `if`:

```rust
pub fn parse_temperature_v2(temperature: &[u8]) -> i16 {
    let is_negative = (temperature[0] == b'-') as usize;
    let digits = &temperature[is_negative..];
    let len = digits.len();

    let tenths = (digits[len - 1] - b'0') as i16;
    let ones = (digits[len - 3] - b'0') as i16;
    let tens = (digits[0] - b'0') as i16 * (len == 4) as i16;

    let value = tens * 100 + ones * 10 + tenths;
    // is_negative has 0 or 1 value therefore 1 - (2 * 0) = 1, 1 - (2 * 1) = -1
    value * (1 - (2 * is_negative as i16))
}
```

Each one runs on five fixed inputs (`0.0`, `9.9`, `-9.9`, `99.9`, `-99.9`), and on 10,000
random temperatures from −99.9 to 99.9:

```rust
group.bench_function("random_10k_parse_temperature", |b| {
    b.iter(|| {
        let mut sum: i32 = 0;
        for t in &inputs {
            sum += parse_temperature(black_box(t)) as i32;
        }
        sum
    })
});
```

`black_box` hides the input from the compiler, so it can't compute the result in advance,
and returning `sum` keeps it from dropping the loop.

| | `0.0` | `9.9` | `-9.9` | `99.9` | `-99.9` | 10,000 random (per input) |
|---|---|---|---|---|---|---|
| `parse::<f32>()` | 4.49 ns | 4.47 ns | 4.47 ns | 4.75 ns | 4.71 ns | 8.96 ns |
| `parse_temperature` (loop) | 1.26 ns | 1.25 ns | 1.50 ns | 1.66 ns | 1.89 ns | 2.79 ns |
| `parse_temperature_v2` (no branches) | 0.85 ns | 0.85 ns | 0.85 ns | 0.85 ns | 0.85 ns | 1.12 ns |

**On random input, the loop is 3.2× faster than `parse::<f32>()`, and the version without
branches 8×.** The loop gets slower with every byte (1.25 ns for `9.9`, 1.89 ns for
`-99.9`), and on random input it's slower than on any fixed one (2.79 ns): with a fixed
input, the branch predictor learns how many times the loop runs and whether a `-` comes;
with random input, the length and the sign change every time and it often guesses wrong.
The version without branches takes the same 0.85 ns on every input, so there's nothing to
predict, and random input costs only a little more (1.12 ns), mostly for reading 10,000
separate `Vec`s.

In an earlier run of the benchmark, the loop came out much slower (2.0 – 2.3 ns on fixed
input, 4.7 ns on random). Most likely because `parse_temperature` still had
`#[inline(never)]` from profiling, which stops the compiler from inlining it into the
benchmark loop. A benchmark measures the function together with how it's called.

### 3. Parsing without branches

The parser without branches from the benchmark in change #2 replaces the loop in `main.rs`.
The loop version stays in `lib.rs` (as `parse_temperature_jon_version`), so the benchmark
can still compare them.

Before: the loop over the bytes with a `match` (change #1).

After:

```rust
pub fn parse_temperature(temperature: &[u8]) -> i16 {
    let is_negative = (temperature[0] == b'-') as usize;
    let digits = &temperature[is_negative..];
    let len = digits.len();

    let tenths = (digits[len - 1] - b'0') as i16;
    let ones = (digits[len - 3] - b'0') as i16;
    let tens = (digits[0] - b'0') as i16 * (len == 4) as i16;

    let value = tens * 100 + ones * 10 + tenths;
    // is_negative has 0 or 1 value therefore 1 - (2 * 0) = 1, 1 - (2 * 1) = -1
    value * (1 - (2 * is_negative as i16))
}
```

Measurements after this change (raw output in
[`measurements/part005/`](../measurements/part005/)):

| | `david` 100M | `david` 1B |
|---|---|---|
| Wall time | 3.1 s | 24.7 s |
| CPU time (user + sys, both threads) | 2.5 s | 26.0 s |
| Peak memory footprint | 1.9 MB | 8.0 MB |
| Swapped to the SSD | 0 | 0 |
| Instructions per row | ~394 | ~404 |
| CPU cycles | 9.9 billion | 103.7 billion |
| Page faults | 84,203 | 842,026 |

**1B: 25.4 s → 24.7 s (−3 %).** The output is identical to Part 004 on both files.

The instructions per row went down only a little (1B ~410 → ~404), but the cycles went
down more (100M 10.7 → 9.9 billion, −8 %; 1B 106.3 → 103.7 billion, −2.4 %). Why the gain on
1B is smaller than on 100M, I don't know yet.

**Branch mispredictions, counted.** To see whether the gain really comes from fewer
mispredicted branches, I counted them with the CPU Counters instrument in Instruments (how,
see [Performance](notes/performance.md#counting-branch-mispredictions-in-instruments)), one
run of each version on 1B, only the main thread:

| Per row (main thread, 1B) | Loop (change #1) | No branches (change #3) | Difference |
|---|---|---|---|
| Instructions | 388.20 | 383.57 | −4.62 (−1.2 %) |
| Cycles | 101.64 | 95.20 | **−6.44 (−6.3 %)** |
| Instructions per cycle | 3.82 | 4.03 | +5 % |
| Branches (`INST_BRANCH`) | 86.79 | 76.23 | **−10.56 (−12 %)** |
| of them conditional (`INST_BRANCH_COND`) | 75.05 | 67.13 | −7.92 |
| Mispredicted branches (`BRANCH_MISPRED_NONSPEC`) | 1.415 | 1.312 | **−0.103 (−7.3 %)** |
| Mispredicted of all branches | 1.63 % | 1.72 % | |

What it shows:

- **The new parser removed ~10.6 branches per row, but only ~0.1 mispredicted one.** On the
  real data, the branch predictor guessed the parser's loop right most of the time and was
  wrong only about once every ten rows. Much less than the benchmark with uniformly random
  temperatures suggested.
- **It does almost the same amount of work.** Only 4.6 instructions per row fewer: the
  branches were replaced by arithmetic (multiplying by `(len == 4)`, the sign).
- **The fewer mispredictions don't explain the whole gain.** ~0.1 misprediction × ~13 cycles
  is ~1.3 cycles, and 4.6 fewer instructions at ~4 per cycle are ~1.2 cycles: together ~2.5
  of the 6.4 cycles saved. My guess for the rest is a shorter chain of dependencies: in the
  loop, every digit needs the `t` and `mul` from the step before, while the new version reads
  the digits independently, so the CPU can work on them at the same time (instructions per
  cycle went up from 3.82 to 4.03). I haven't verified that.
- **~1.3 mispredictions per row are still left**, outside the parser. At ~13 cycles each
  that's ~17 cycles per row, ~18 % of the main thread's time. Where they come from is
  measured below: mostly hashing the station name, then the search for the `;`.

**Where are they? Not with Instruments.** I first tried to find out with Instruments which
functions the remaining mispredictions come from, and didn't succeed:

- The counters in the trace above are split by function too, but that split is useless:
  Instruments adds the whole change of a counter since the previous sample (~1 ms, thousands
  of rows) to the function the program happens to be in at the sample. So the mispredictions
  per function only copy the share of time (`split` 45.9 % of the samples and 46.0 % of the
  mispredictions, the `HashMap` 35.1 % and 35.0 %, …). Even the parser without branches got
  6.4 % of them.
- The right way is to sample by the event instead of by time (*Sample By: Events*,
  `BRANCH_MISPRED_NONSPEC` every 100,000): every sample then means 100,000 mispredictions
  and carries the call stack. On my Mac (M4, macOS 26.6) it doesn't work, neither from
  `xctrace` (also with `sudo`) nor from the Instruments app. As soon as there's any counter
  in *Events and Formulas*, even only *Cycles*, the recording fails with
  `Failed binding 'kdebug-counters-with-pmi-sample' table: samplers differ`. Without any
  counter it records the samples (100,000 mispredictions each), but without the call stacks,
  so the profile stays empty. Others report the same problem on the Apple Developer Forums
  ([sampling on PMI events stopped working](https://developer.apple.com/forums/thread/736381),
  since Xcode 14.3, also on an M2).

**With `perf` on Linux.** So I measured it with `perf` on an AWS instance: `c7g.large`
(Graviton3, ARM like the M4), Ubuntu 24.04, the same code, and a 1B file generated there with
the same generator (different random rows, same format). A virtual machine on the Mac
(including Docker) wouldn't do, because it doesn't get the CPU counters.

```sh
sudo perf stat -e branches,branch-misses ./target/release/david input/1B_measurements.txt > /dev/null
sudo perf record -e branch-misses:u -c 100000 -o perf-branch-misses-1B.data ./target/release/david input/1B_measurements.txt > /dev/null
sudo perf report -i perf-branch-misses-1B.data --no-children --sort srcfile --full-source-path --stdio
sudo perf annotate -i perf-branch-misses-1B.data --stdio -l
```

`perf stat` with only two events, so that they don't have to share the few counters of a
small instance and both count the whole time. `perf record -c 100000` takes a sample at
every 100,000th mispredicted branch (13,810 samples), `:u` only in my code, not in the
kernel. No call stacks: almost everything is inlined into `main`, so the samples are matched
to source lines through the debug info instead.

The Graviton3 mispredicts about as often as the M4:

| Per row, 1B | Graviton3 (`perf stat`, both threads) | M4 (Instruments, main thread) |
|---|---|---|
| Branches | 72.7 | 76.2 |
| Mispredicted branches | 1.38 | 1.31 |
| Mispredicted of all branches | 1.9 % | 1.7 % |

Where the mispredictions happen, by source file:

| Source file | Share | What it is |
|---|---|---|
| `core/src/hash/sip.rs` | **50.8 %** | SipHash, the default hasher of `HashMap` |
| `core/src/ptr/mod.rs` | 4.6 % | reading the bytes of the key inside SipHash |
| `core/src/hash/mod.rs`, `std/src/hash/random.rs` | 7.0 % | calling the hasher (`Hash` for `[u8]`, `RandomState`) |
| **hashing the station name, together** | **~62 %** | ~0.86 mispredictions per row |
| `hashbrown` (`raw.rs`, `map.rs`, …), `core/src/slice/cmp.rs` | ~6.7 % | looking the name up in the table and comparing it |
| `src/david/main.rs` | 15.7 % | in fact mostly the end of the search for the `;`, see below |
| `core/src/slice/iter/macros.rs` | 4.5 % | the loops of `split` and `rsplitn` |

- **Hashing the station name causes most of them.** SipHash takes the key 8 bytes at a time
  and then handles the rest (length % 8) in steps of 4, 2 and 1 bytes. `perf annotate` shows
  the branches for that rest in `DefaultHasher::write`:

  ```text
  cmp  x9, #0x4     // sip.rs:126   at least 4 bytes left?
  b.cs …
  cmp  x12, x9      // sip.rs:132   at least 2 left?
  b.cs …
  cmp  x11, x9      // sip.rs:137   1 left?
  b.cc …
  ```

  Which of them are taken depends on the length of the station name, which changes from row
  to row, so the predictor often guesses wrong. Profiling on the M4 already showed that
  hashing costs ~18 % of the time; it's also the biggest source of mispredictions.
- **The 15.7 % in `main.rs` are mostly the search for the `;`.** `rsplitn` looks for the `;`
  from the end of the line, one byte at a time:

  ```text
  17e64: cbz   x26, …          // no bytes left?
  17e74: ldurb w10, [x9, #-1]  // load the byte before the position
  17e78: cmp   w10, #0x3b      // is it ';'?
  17e7c: b.ne  17e64           // no → next byte
  ```

  It goes through 3, 4 or 5 bytes depending on the length of the temperature, so the `b.ne`
  at the end is hard to predict. The samples land a few instructions later (*skid*), in the
  parser that comes right after it: 11.75 % on `ldrb w11, [x9]` (`main.rs:336`) and 7.33 % on
  `cmp x1, #0x2` (`main.rs:335`), instructions that can't mispredict.
- **The parser really has no branches**, and the compiler went even further than my code.
  The annotated instructions of `david::main` (`perf report`, then `a`):

  ```text
  ldrb  w8, [x9]            first byte
  cmp   w8, #0x2d           is it '-'?
  cset  w10, eq             is_negative = 0 or 1, no jump
  …
  mul   w10, w10, w12       ones * 10
  mul   w11, w11, w12       tens * 100
  csel  w10, w11, wzr, eq   tens * (len == 4)
  …
  cneg  w8, w9, eq          value * (1 - (2 * is_negative))
  ```

  It didn't multiply by `(len == 4)`: it replaced that with `csel` (*conditional select*:
  take `tens` or zero). And it didn't multiply by `1 - (2 * is_negative)` either: it replaced
  that with `cneg` (*conditional negate*: flip the sign if the first byte was a `-`). Both use
  the flags of a comparison instead of a jump, so there's nothing to predict. The only
  branches left in the parser are the bounds checks, which always pass.

So of the ~1.38 mispredictions per row, ~0.86 come from hashing the name (its length),
~0.25 – 0.3 from the end of the search for the `;` (the length of the temperature), and the
rest from the table lookup and the other loops.

**A detailed `perf stat`.** Jon uses `perf stat -d -d`, which adds the counters of the L1
caches and the TLBs. On the instance it failed (`Failure to read '#slots'Cannot find metric
or group 'Default'`: `perf` couldn't compute its default derived metrics), so I listed the
same events by hand:

```sh
sudo perf stat --all-user -e task-clock,context-switches,cpu-migrations,page-faults,cycles,instructions,stalled-cycles-frontend,stalled-cycles-backend,branches,branch-misses,L1-dcache-loads,L1-dcache-load-misses,L1-icache-loads,L1-icache-load-misses,dTLB-loads,dTLB-load-misses,iTLB-loads,iTLB-load-misses ./target/release/david input/1B_measurements.txt > /dev/null
```

`--all-user` counts only my code, not the kernel (like `:u`). 18 events don't fit on the few
counters of a `c7g.large`, so `perf` takes turns between them: each one counted only 14 – 22 %
of the run, and `perf` scales the result up. So these are estimates. Per row:

| Per row, 1B | Graviton3 | M4 (Instruments, main thread) |
|---|---|---|
| Instructions | 379 | 384 |
| Cycles | 102 | 95 |
| Instructions per cycle | 3.72 | 4.03 |
| Branches / mispredicted | 70.9 / 1.43 (2.0 %) | 76.2 / 1.31 (1.7 %) |
| Loads from the L1 data cache | 85.6 | |
| of them missed | 0.07 (0.08 %) | |
| L1 instruction cache misses | 0.006 | |
| dTLB / iTLB misses | 0.002 / 0.001 | |
| Cycles the backend stalled | 32.1 (31.5 % of the cycles) | |
| Cycles the frontend stalled | 6.1 (6 %) | |

- **Almost the same instructions as on the M4** (379 vs 384 per row): the same architecture
  (ARM64) and the same compiler. The M4 just gets more of them done per cycle.
- **Memory isn't the problem.** Only 0.08 % of the 85.6 loads per row miss the L1 cache: the
  file is read in order, so the hardware prefetcher loads it ahead, and the `HashMap` with
  413 stations fits in L1 (~30 KB, see [TLB](notes/tlb.md#summary)).
- **The TLB misses are negligible too.** The TLB translates whole pages, and the file is read
  in order, so a new translation is only needed at the next page. A 4 KB page holds ~300
  rows, and 13.8 GB are ~3.37 million pages, so about one dTLB miss per page means ~3.4
  million misses. The 2.3 million measured (2,263,749 / 1 billion rows ≈ 0.002 per row) are
  the same order; the counter only ran 14 % of the time, and which hardware event the generic
  `dTLB-load-misses` maps to depends on the CPU. The iTLB translates the addresses of the code,
  and the program is small and stays in one loop: 940,930 misses for the whole run.
- **The mispredictions cost ~15 % of the cycles** (1.43 per row × ~11 cycles ≈ 16 of 102),
  about the same as on the M4.
- **The backend stalls in 31.5 % of the cycles**, the frontend only in 6 %. Not because of
  cache misses, there are almost none. My guess is long chains of dependent instructions,
  each waiting for the result of the one before, as in the rounds of SipHash (a chain of
  `add`, `rotate` and `xor`). I haven't verified that.
- **160,266 page faults instead of ~3.4 million.** Linux on ARM uses 4 KB pages, but it has
  *fault-around*: on a page fault in a mapped file it also maps the pages around it that are
  already in the page cache, 16 pages (64 KB) by default. 3.37 million / 16 ≈ 210,000, the
  same order. macOS on the M4 had 842,026, one for every 16 KB page.
- 104 s of wall time, but only 41.7 s user: the instance waited for its disk (EBS gp3, ~125 MB/s).

On 100M, the program waited again: 3.1 s of wall time, but only 2.5 s of CPU time.

## What I learned

## Next steps

## TODO

Things I want to try later. I'm not doing them yet, because Jon may get to them in a later
part of the stream, and I don't want to get ahead of him.

- **A hasher without branches on the length of the key.** ~62 % of the branch mispredictions
  come from SipHash handling the rest of the station name (length % 8) with branches (see
  change #3). A hasher that treats every name the same way, whatever its length, would
  remove most of them.
- **Find the `;` without a byte-by-byte loop.** The end of the search for the `;` is the
  second source of mispredictions (~0.25 – 0.3 per row). The temperature is only 3 – 5
  bytes, so its position could be found with a fixed number of steps instead of a loop.
- **Find out what the program waits for on 100M** since change #1 (~0.6 – 0.8 s of wall time
  without CPU time, see change #3), for example with the thread states in a System Trace. One
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
