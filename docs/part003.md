# Part 003

**Dates:** 2026-10-25 – 2026-10-25
**Video progress:** stopped at [43:19](https://www.youtube.com/watch?v=tCY7p6dVAGE&t=2599s) when this part was published
**Code:** git tag [`part003`](https://github.com/davidhruby/brrr/tree/part003). To get the code exactly as it was for this part: `git checkout part003`

New here? Start with the [Introduction](intro.md).

## Goal

My version is now close to Jon's, so I don't compare it with his any more, only with the
previous step.

## What changed

### 1. Renaming to Jon's names

No change in behavior: the variables now have the same names as in Jon's version, so when he
talks about `stats`, `station` or `temperature` in the stream, it means the same thing in my
code.

Before:

```rust
let mut location_to_measurement: BTreeMap<String, MeasurementStats> = BTreeMap::new();
for line in reader.lines() {
    let line = line.unwrap();
    let (location, measurement_str) = line.split_once(';').unwrap();
    let measurement = measurement_str.parse::<f32>().unwrap();

    location_to_measurement.entry(location.to_string())
        .and_modify(|m| m.update(measurement))
        .or_insert_with(|| MeasurementStats::new(measurement));
}
```

After:

```rust
let mut stats: BTreeMap<String, MeasurementStats> = BTreeMap::new();
for line in reader.lines() {
    let line = line.unwrap();
    let (station, temperature) = line.split_once(';').unwrap();
    let temperature = temperature.parse::<f32>().unwrap();

    stats.entry(station.to_string())
        .and_modify(|m| m.update(temperature))
        .or_insert_with(|| MeasurementStats::new(temperature));
}
```

Measurements after this change (raw output from commit
[`9fc2957`](https://github.com/davidhruby/brrr/tree/9fc2957/measurements/part003)):

| | `david` 100M | `david` 1B |
|---|---|---|
| Wall time | 10.7 s | 108 s |
| Memory | 1.9 MB | 1.9 MB |
| Swapped to the SSD | 0 | 0 |
| Instructions per row | ~1,840 | ~1,860 |

**No change in speed.**

### 2. `HashMap` instead of `BTreeMap`, sorted only for the output

The loop now looks stations up in a `HashMap`: one hash and usually one string comparison,
instead of walking a B-tree with ~10 or more string comparisons. A `HashMap` has no order, so
before the output its 413 entries are moved into a `BTreeMap` once, which sorts them by name.

Before:

```rust
let mut stats: BTreeMap<String, MeasurementStats> = BTreeMap::new();
// ...
for (location, MeasurementStats { min, max, sum, count}) in stats.iter() {
```

After:

```rust
let mut stats: HashMap<String, MeasurementStats> = HashMap::new();
// ...
let stats = BTreeMap::from_iter(stats);
for (location, MeasurementStats { min, max, sum, count}) in stats.iter() {
```

Measurements after this change (raw output from commit
[`1396548`](https://github.com/davidhruby/brrr/tree/1396548/measurements/part003)):

| | `david` 100M | `david` 1B |
|---|---|---|
| Wall time | 7.6 s | 77 s |
| Memory | 1.9 MB | 1.9 MB |
| Swapped to the SSD | 0 | 0 |
| Instructions per row | ~1,560 | ~1,560 |

**28–29 % faster** (100M 10.7 s → 7.6 s, 1B 108 s → 77 s), but only ~15 % fewer instructions
(~280 per row less). The CPU also runs the remaining instructions faster: 5.0 instructions per
cycle instead of 4.3 (why is in the [TODO](#todo)).

### 3. `get_mut` first: no new `String` for every line

`entry(station.to_string())` allocated a new `String` on every line, only to throw it away
when the station was already in the map (see
[Part 002, What I learned #1](part002-just-poslishments.md#1-arguments-are-evaluated-before-the-call-or_insert-vs-or_insert_with)).
Now the loop first looks the station up with `get_mut`, which takes the `&str` from the line
as it is. Only for a new station, 413 times in total, it creates the `String` and inserts it.
A new station starts from neutral values (`Default`), and the same `update` then runs for
every line.

Before:

```rust
impl MeasurementStats {
    fn new(measurement: f32) -> Self {
        Self {
            min: measurement,
            max: measurement,
            sum: measurement,
            count: 1
        }
    }
    // ...
}

// in the loop:
stats.entry(station.to_string())
    .and_modify(|m| m.update(temperature))
    .or_insert_with(|| MeasurementStats::new(temperature));
```

After:

```rust
impl Default for MeasurementStats {
    fn default() -> Self {
        MeasurementStats {
            min: f32::MAX,
            max: f32::MIN,
            sum: 0f32,
            count: 0u32
        }
    }
}

// in the loop:
let stat = match stats.get_mut(station) {
    Some(stat) => stat,
    None => stats.entry(station.to_string()).or_insert(MeasurementStats::default())
};
stat.update(temperature);
```

Measurements after this change (raw output from commit
[`2f17eee`](https://github.com/davidhruby/brrr/tree/2f17eee/measurements/part003)):

| | `david` 100M | `david` 1B |
|---|---|---|
| Wall time | 6.4 s | 66 s |
| Memory | 1.9 MB | 1.9 MB |
| Swapped to the SSD | 0 | 0 |
| Instructions per row | ~1,200 | ~1,200 |

**14–16 % faster** (100M 7.6 s → 6.4 s, 1B 77 s → 66 s) and ~360 instructions per row less
(−23 %): the allocation, the copy of the name and the deallocation on every line are gone.

### 4. Bytes instead of strings: `Vec<u8>` keys

The loop now works with raw bytes (`&[u8]`) instead of `&str`, so it doesn't check that every
line is valid UTF-8. `reader.split(b'\n')` gives each line as a `Vec<u8>` (still a new
allocation per line, like `lines()`), and `rsplitn(2, ...)` looks for the `;` from the end of
the line, where the temperature is at most 5 bytes long, so it's found after a few bytes
instead of after the whole station name. The first item from the right is the temperature,
the second the station. Only the temperature is checked as UTF-8, because `parse` needs a
`&str`. The keys of the map are `Vec<u8>`, and they become `String`s only at the end, for
the 413 stations of the sorted output.

Before:

```rust
let mut stats: HashMap<String, MeasurementStats> = HashMap::new();
for line in reader.lines() {
    let line = line.unwrap();
    let (station, temperature) = line.split_once(';').unwrap();
    let temperature = temperature.parse::<f32>().unwrap();

    let stat = match stats.get_mut(station) {
        Some(stat) => stat,
        None => stats.entry(station.to_string()).or_insert(MeasurementStats::default())
    };
    stat.update(temperature);
}

let stats = BTreeMap::from_iter(stats);
```

After:

```rust
let mut stats: HashMap<Vec<u8>, MeasurementStats> = HashMap::new();
for line in reader.split(b'\n') {
    let line = line.unwrap();
    let mut fields = line.rsplitn(2, |c| *c == b';');
    let temperature = str::from_utf8(fields.next().unwrap()).unwrap().parse::<f32>().unwrap();
    let station = fields.next().unwrap();

    let stat = match stats.get_mut(station) {
        Some(stat) => stat,
        None => stats.entry(station.to_vec()).or_insert(MeasurementStats::default())
    };
    stat.update(temperature);
}

let stats = BTreeMap::from_iter(stats.into_iter().map(|(k, v)| (String::from_utf8(k).unwrap(), v)));
```

Measurements after this change (raw output in
[`measurements/part003/`](../measurements/part003/)):

| | `david` 100M | `david` 1B |
|---|---|---|
| Wall time | 5.5 s | 53 s |
| Memory | 1.9 MB | 1.8 MB |
| Swapped to the SSD | 0 | 0 |
| Instructions per row | ~1,100 | ~1,110 |

**15–20 % faster** (100M 6.4 s → 5.5 s, 1B 66 s → 53 s), but only ~8 % fewer instructions
(~95 per row less). Most of the gain is in the CPU cycles (−17 % on 100M, −20 % on 1B): 5.1
instructions per cycle instead of 4.5.

### 5. `from_utf8_unchecked`: no UTF-8 check at all

The temperature and, at the end, the station names are turned into `str` / `String` without
checking that they are valid UTF-8. `from_utf8_unchecked` is `unsafe`: if the bytes weren't
valid UTF-8, the program would have undefined behavior instead of stopping with an error. The
rules of the challenge guarantee a UTF-8 input, and a temperature is only ASCII digits, `-`
and `.`, so here it's safe.

Before:

```rust
let temperature = str::from_utf8(fields.next().unwrap()).unwrap().parse::<f32>().unwrap();
// ...
let stats = BTreeMap::from_iter(stats.into_iter().map(|(k, v)| (String::from_utf8(k).unwrap(), v)));
```

After:

```rust
let temperature = unsafe { str::from_utf8_unchecked(fields.next().unwrap()).parse::<f32>().unwrap() };
// ...
let stats = BTreeMap::from_iter(stats.into_iter().map(|(k, v)| (unsafe { String::from_utf8_unchecked(k) }, v)));
```

Measurements after this change (raw output in
[`measurements/part003/`](../measurements/part003/)):

| | `david` 100M | `david` 1B |
|---|---|---|
| Wall time | 5.1 s | 52 s |
| Memory | 1.9 MB | 1.8 MB |
| Swapped to the SSD | 0 | 0 |
| Instructions per row | ~1,050 | ~1,050 |

**3–7 % faster** (100M 5.5 s → 5.1 s, 1B 53 s → 52 s) and ~50 instructions per row less
(−5 %): checking the ≤ 5 bytes of the temperature cost that much on every line. The check of
the station names at the end runs only 413 times, so it doesn't matter.




## What I learned

## Next steps

## TODO

Things I want to try later. I'm not doing them yet, because Jon may get to them in a later
part of the stream, and I don't want to get ahead of him.

- **Read the file with `mmap` instead of `BufReader`.** No copying from the file cache into
  a buffer, no swapping on the 1B file, and an easy way to split the file between threads
  (see [What I learned #1](part001-first-naive-version.md#1-read_to_string-loads-the-whole-file-into-memory)).
- **Store the temperatures as integers in tenths** (`12.3` → `123`): `i64` for the sum,
  `u32` for the count and `i16` for min / max. Exact for every valid input, where `f32` gets
  stuck at 2³¹ and `f64` is slightly off, and 16 bytes per station instead of 32 (see
  [What I learned #5](part001-first-naive-version.md#5-f32-f64-or-integers)).
- **Parse the temperature by hand** into tenths, skipping the dot, instead of
  `parse::<f32>()`: the format is fixed by the rules, so a few steps are enough instead of a
  general float parser (~4 % of the time today, see
  [What I learned #5](part001-first-naive-version.md#5-f32-f64-or-integers)).
- **Write the output through a `BufWriter`** instead of calling `println!` for every
  station. In the System Trace of the 100M run, my version made **413 `write` system calls**,
  one per line of output, because `println!` writes each line out on its own. A `BufWriter`
  collects the lines in a buffer and writes them in a few big calls. With 413 lines it
  doesn't matter yet, but with a bigger output (up to 10,000 stations by the rules) or a
  faster program it will.
- **Look at the release profile.** All my measurements use `cargo build --release` with the
  default settings. Cargo lets the `[profile.release]` section in `Cargo.toml` change how the
  code is compiled, e.g. `opt-level`, and further options such as `lto` (link-time
  optimization) or `codegen-units`. See
  [Customizing Builds with Release Profiles](https://doc.rust-lang.org/book/ch14-01-release-profiles.html)
  in the Rust book, and measure whether any of it makes a difference.
- **Pin the program to one core on Linux** (`taskset`) and see whether the jumps between
  cores cost anything measurable (see [Something to try later](part001/measurements.md#exact-check-with-instruments)).
- **Find out why the `HashMap` version runs more instructions per cycle** (5.0 instead of 4.3,
  see [change #2](#2-hashmap-instead-of-btreemap-sorted-only-for-the-output)). My guess: the
  linear search in a B-tree node ends at a different key for every station, which is hard for
  the CPU to predict, while the `HashMap` lookup mostly goes the same way every time. Measure
  the branch mispredictions of both versions, and compare the `BTreeMap` used for sorting the
  output with another way of sorting the stations.
