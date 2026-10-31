# Part 005

**Dates:** 2026-10-30 – 2026-10-31
**Video progress:** stopped at [57:56](https://www.youtube.com/watch?v=tCY7p6dVAGE&t=3476s) when this part was published
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

Measurements after this change (raw output in
[`measurements/part005/`](../measurements/part005/)):

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
(2,741 instead of 1,580). RustRover was starting at the same time as this run, so the 100M
number needs to be measured again.

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
