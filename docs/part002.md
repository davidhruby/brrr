# Part 002 — Next steps from Part 001

**Dates:** 2026-10-21 – 2026-10-21
**Video progress:** stopped at [38:38](https://www.youtube.com/watch?v=tCY7p6dVAGE&t=2318s) when this part was published
**Code:** git tag [`part002`](https://github.com/davidhruby/brrr/tree/part002). To get the code exactly as it was for this part: `git checkout part002`

New here? Start with the [Introduction](intro.md).

## What changed

### 1. Streaming with `BufReader`

Step 1 of the [Next steps from Part 001](part001-first-naive-version.md#next-steps): my version
streams the file with `BufReader`, like Jon's, instead of loading it all with
`read_to_string`.

Before:

```rust
for line in read_to_string(input_file_path).unwrap().lines() {
```

After:

```rust
let file = File::open(input_file_path).unwrap();
let reader = BufReader::new(file);

for line in reader.lines() {
    let line = line.unwrap();
```

Measurements after this change (raw output from commit
[`9b1c02f`](https://github.com/davidhruby/brrr/tree/9b1c02f/measurements/part002)):

| | `david` 100M | `jonhoo` 100M | `david` 1B | `jonhoo` 1B |
|---|---|---|---|---|
| Wall time | 15.3 s | 10.6 s | 146 s | 107 s |
| Memory | 1.9 MB | 1.9 MB | 1.9 MB | 1.9 MB |
| Swapped to the SSD | 0 | 0 | 0 | 0 |
| Instructions per row | ~2,530 | ~1,800 | ~2,560 | ~1,840 |

**The swapping on 1B is gone** (266 s → 146 s, 13.8 GB → 1.9 MB), but every row got more
expensive: `BufRead::lines()` allocates a new `String` for each line, while `str::lines()` on
the big `String` didn't. That's why 100M, which never swapped, got slower than in Part 001
(12.5 s → 15.3 s; part of it is waiting for a RAM disk that was partly in swap).

### 2. `split_once` instead of `split(';').collect::<Vec<_>>()`

Step 2 of the [Next steps from Part 001](part001-first-naive-version.md#next-steps): no `Vec`
per line any more, and the search for `;` stops at the first one. With `.unwrap()` on the
result, a line without `;` now stops the program instead of being skipped silently, like
Jon's version (step 7).

Before:

```rust
let split = line.split(';').collect::<Vec<&str>>();
if split.len() == 2 {
    let location = split[0];
    let measurement = &split[1].parse::<f32>().unwrap();
```

After:

```rust
let (location, measurement_str) = line.split_once(';').unwrap();
let measurement = measurement_str.parse::<f32>().unwrap();
```

Measurements after this change (raw output from commit
[`c1b3808`](https://github.com/davidhruby/brrr/tree/c1b3808/measurements/part002)):

| | `david` 100M | `jonhoo` 100M | `david` 1B | `jonhoo` 1B |
|---|---|---|---|---|
| Wall time | 12.3 s | 10.6 s | 126 s | 107 s |
| Memory | 1.8 MB | 1.9 MB | 1.8 MB | 1.9 MB |
| Swapped to the SSD | 0 | 0 | 0 | 0 |
| Instructions per row | ~1,980 | ~1,800 | ~2,010 | ~1,840 |

`split_once` took back almost exactly what `BufReader` added: ~540 instructions per row less
(−22 %) and 15 % fewer CPU cycles, by dropping the `Vec` allocation per line and the search
for a second `;`. 1B got another 14 % faster.

### 3. `entry().and_modify().or_insert()` and `.min()` / `.max()`

Steps 3 and 4 of the [Next steps from Part 001](part001-first-naive-version.md#next-steps): one
lookup in the map instead of two, a new station gets its first measurement directly, and
`.min()` / `.max()` instead of the `if` comparisons.

Before:

```rust
if location_to_measurement.contains_key(location) {
    let measurement_stats = location_to_measurement.get_mut(location).unwrap();
    if measurement_stats.min > measurement {
        measurement_stats.min = measurement;
    }
    if measurement_stats.max < measurement {
        measurement_stats.max = measurement;
    }
    measurement_stats.sum += measurement;
    measurement_stats.count += 1;
} else {
    location_to_measurement.insert(location.to_string(), MeasurementStats {
        min: measurement,
        max: measurement,
        sum: measurement,
        count: 1
    });
}
```

After:

```rust
location_to_measurement.entry(location.to_string())
    .and_modify(| ltm| {
        ltm.min = ltm.min.min(measurement);
        ltm.max = ltm.max.max(measurement);
        ltm.sum += measurement;
        ltm.count += 1;
    }).or_insert(MeasurementStats {
    min: measurement,
    max: measurement,
    sum: measurement,
    count: 1
});
```

Measurements after this change (raw output from commit
[`f0d4dfd`](https://github.com/davidhruby/brrr/tree/f0d4dfd/measurements/part002)):

| | `david` 100M | `jonhoo` 100M | `david` 1B | `jonhoo` 1B |
|---|---|---|---|---|
| Wall time | 10.7 s | 10.6 s | 109 s | 107 s |
| Memory | 1.9 MB | 1.9 MB | 1.9 MB | 1.9 MB |
| Swapped to the SSD | 0 | 0 | 0 | 0 |
| Instructions per row | ~1,840 | ~1,800 | ~1,860 | ~1,840 |

**My version is now almost as fast as Jon's**, about 1–2 % slower (100M 12.3 s → 10.7 s,
1B 126 s → 109 s): ~140
instructions per row less and 11 % fewer CPU cycles. `entry(location.to_string())` allocates
a `String` for every line, even for a station that's already in the map.

### 4. Methods `new` and `update` on `MeasurementStats`

Step 5 of the [Next steps from Part 001](part001-first-naive-version.md#next-steps): the update
logic moved into the struct, and the loop just calls it. `or_insert_with` only creates a new
`MeasurementStats` for a new station (see [What I learned #1](#1-arguments-are-evaluated-before-the-call-or_insert-vs-or_insert_with)).

Before:

```rust
location_to_measurement.entry(location.to_string())
    .and_modify(| ltm| {
        ltm.min = ltm.min.min(measurement);
        ltm.max = ltm.max.max(measurement);
        ltm.sum += measurement;
        ltm.count += 1;
    }).or_insert(MeasurementStats {
    min: measurement,
    max: measurement,
    sum: measurement,
    count: 1
});
```

After:

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

    fn update(&mut self, measurement: f32) {
        self.min = self.min.min(measurement);
        self.max = self.max.max(measurement);
        self.sum += measurement;
        self.count += 1;
    }
}

// in the loop:
location_to_measurement.entry(location.to_string())
    .and_modify(|m| m.update(measurement))
    .or_insert_with(|| MeasurementStats::new(measurement));
```

Measurements after this change (raw output in
[`measurements/part002/`](../measurements/part002/)):

| | `david` 100M | `jonhoo` 100M | `david` 1B | `jonhoo` 1B |
|---|---|---|---|---|
| Wall time | 10.7 s | 10.6 s | 108 s | 107 s |
| Memory | 1.9 MB | 1.9 MB | 1.8 MB | 1.9 MB |
| Swapped to the SSD | 0 | 0 | 0 | 0 |
| Instructions per row | ~1,840 | ~1,800 | ~1,860 | ~1,840 |

**No change in speed**, same instructions per row as before: the compiler inlines the small
methods, so moving the code into the struct costs nothing.

## What I learned

### 1. Arguments are evaluated before the call: `or_insert` vs `or_insert_with`

In a chain like this, **every method is always called**. `and_modify` doesn't skip anything;
it hands the same `Entry` on, and each method checks for itself whether the key exists:

```rust
location_to_measurement.entry(location.to_string())
    .and_modify(|m| m.update(measurement))
    .or_insert(MeasurementStats::new(measurement));
```

And Rust evaluates a method's arguments before calling it. So `MeasurementStats::new(measurement)`
runs on every line, and `or_insert` throws the result away when the station is already in the
map. Only the insert itself happens just for a new station.

`or_insert_with` takes a closure instead of a value, a recipe for making the value, and only
calls it when the key is new:

| | Station already in the map | New station |
|---|---|---|
| `and_modify(...)` | called, updates the value | called, does nothing |
| `or_insert(new(...))` | called, `new` **runs** and the result is thrown away | called, `new` runs and the result is inserted |
| `or_insert_with(\|\| new(...))` | called, the closure is **not** called | called, the closure runs and the result is inserted |

**So I expected a saving, and got none.** Filling in the four fields of `MeasurementStats` is
roughly 4 instructions, so switching to `or_insert_with` should save ~4 instructions per line,
~4 billion on the 1B file. I measured both versions with `/usr/bin/time -l` (on a variant of my
code with a `HashMap`): the difference was only 0.98 billion instructions (0.064 %). That's
noise: the same binary already varies by tens of millions of instructions between runs.

The assembly explains why. I generated it for both versions and compared them:

```sh
# with or_insert
cargo asm --bin david david::main --rust > or_insert.s
# with or_insert_with
cargo asm --bin david david::main --rust > or_insert_with.s

diff or_insert.s or_insert_with.s    # no output: identical
```

**The compiler produced exactly the same machine code for both.** In it, the new
`MeasurementStats` is only written in the block for a missing key, which the program only
reaches after the hash table lookup hits an empty slot:

```asm
	stp  w24, w24, [x9, #-16]    // min = measurement, max = measurement
	mov  w22, #1
	stp  w24, w22, [x9, #-8]     // sum = measurement, count = 1
```

On the common path, a station that's already in the map, it's never built. Building it has no
side effects, so the compiler moved it into that branch on its own: the eager `or_insert`
already behaves like `or_insert_with`. Jon's `.or_insert((f64::MAX, 0., 0, f64::MIN))`, four
constants, is free for the same reason. `or_insert_with` is still the better way to write it,
because it says what I mean and doesn't rely on the optimizer; with an expensive default (an
allocation, I/O) the difference would be real.

That's exactly the case one step earlier: `location.to_string()`, the argument of `entry`, is
evaluated on every line too, and that one **allocates a `String`** each time, even for a
station that's already in the map. An allocation is a side effect (a call to the allocator),
so the compiler can't skip it. In the assembly it shows up as `__rust_alloc` + `memcpy` before
the lookup and `__rust_dealloc` right after the key is found.

The same value / closure pairs show up all over Rust: `unwrap_or` / `unwrap_or_else`,
`ok_or` / `ok_or_else`. `cargo clippy` warns about the eager version (`or_fun_call`) when a
function call is passed where a closure would do.

## Next steps

## TODO

Things I want to try later. I'm not doing them yet, because Jon may get to them in a later
part of the stream, and I don't want to get ahead of him.

- **Read the file with `mmap` instead of `BufReader`.** No copying from the file cache into
  a buffer, no swapping on the 1B file, and an easy way to split the file between threads
  (see [What I learned #1](part001-first-naive-version.md#1-read_to_string-loads-the-whole-file-into-memory)).
- **Use a `HashMap` instead of a `BTreeMap`** and sort the stations once at the end. One
  hash and ~1 string comparison per lookup instead of ~10 comparisons (see
  [What I learned #3](part001-first-naive-version.md#3-looking-up-a-station-contains_key--get_mut-entry-or-just-get_mut)).
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
