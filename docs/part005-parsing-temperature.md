# Part 005

**Dates:** 2026-10-30 – 2026-10-31
**Video progress:** stopped at [1:49:41](https://www.youtube.com/watch?v=tCY7p6dVAGE&t=6581s) when this part was published
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

The interesting part is where the gain comes from. The instructions per row went down only a
little (1B ~410 → ~404, −1.5 %), but the cycles went down more (100M 10.7 → 9.9 billion,
−8 %; 1B 106.3 → 103.7 billion, −2.4 %). So the new parser doesn't do much less work.
Most likely the CPU just doesn't stall on it any more, because there are no branches left to
mispredict, as in the benchmark. Why the gain on 1B is smaller than on 100M, I don't know yet.

On 100M, the program waited again: 3.1 s of wall time, but only 2.5 s of CPU time.

## What I learned

## Next steps

## TODO

Things I want to try later. I'm not doing them yet, because Jon may get to them in a later
part of the stream, and I don't want to get ahead of him.

- **Check the waiting in Instruments.** The `thread-state` table of a System Trace shows how
  long the thread was running and how long it was blocked, which would confirm the waiting
  directly instead of from `real` − `user` − `sys`.
- **Write the output through a `BufWriter`** instead of calling `println!` for every
  station. In the System Trace of the 100M run, my version made **413 `write` system calls**,
  one per line of output, because `println!` writes each line out on its own. A `BufWriter`
  collects the lines in a buffer and writes them in a few big calls. With 413 lines it
  doesn't matter yet, but with a bigger output (up to 10,000 stations by the rules) or a
  faster program it will.
- **Pin the program to one core on Linux** (`taskset`) and see whether the jumps between
  cores cost anything measurable (see [Something to try later](notes/measurements.md#exact-check-with-instruments)).
- **Find out why the `HashMap` version runs more instructions per cycle** (5.0 instead of 4.3,
  see [Part 003, change #2](part003-hashmap-bytes-and-lto.md#2-hashmap-instead-of-btreemap-sorted-only-for-the-output)).
  My guess: the linear search in a B-tree node ends at a different key for every station,
  which is hard for the CPU to predict, while the `HashMap` lookup mostly goes the same way
  every time. Measure the branch mispredictions of both versions, and compare the `BTreeMap`
  used for sorting the output with another way of sorting the stations.
