# Part 001 — The first naive version

**Dates:** 2026-10-15 – 2026-10-20
**Video progress:** stopped at [38:38](https://www.youtube.com/watch?v=tCY7p6dVAGE&t=2318s) when this part was published
**Code:** git tag [`part001`](https://github.com/davidhruby/brrr/tree/part001). To get the code exactly as it was for this part: `git checkout part001`

New here? Start with the [Introduction](intro.md): what this series is about, the challenge,
my goal, how I generate the input files, my machine and how I work.

## My version

`src/david/main.rs`:

```rust
use std::{collections::BTreeMap, env, fs::read_to_string};

struct MeasurementStats {
    min: f32,
    max: f32,
    sum: f32,
    count: u32
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let input_file_path = &args[1];

    let mut location_to_measurement: BTreeMap<String, MeasurementStats> = BTreeMap::new();
    for line in read_to_string(input_file_path).unwrap().lines() {
        let split = line.split(';').collect::<Vec<&str>>();
        if split.len() == 2 {
            let location = split[0];
            let measurement = &split[1].parse::<f32>().unwrap();

            if location_to_measurement.contains_key(location) {
                let measurement_stats = location_to_measurement.get_mut(location).unwrap();
                if measurement_stats.min > *measurement {
                    measurement_stats.min = *measurement;
                }
                if measurement_stats.max < *measurement {
                    measurement_stats.max = *measurement;
                }
                measurement_stats.sum += measurement;
                measurement_stats.count += 1;
            } else {
                location_to_measurement.insert(location.to_string(), MeasurementStats {
                    min: *measurement,
                    max: *measurement,
                    sum: *measurement,
                    count: 1
                });
            }
        }
    }

    for (location, stats) in location_to_measurement.iter() {
        println!("{}={1:.1}/{2:.1}/{3:.1}", location, stats.min, stats.sum / stats.count as f32, stats.max);
    }
}
```

## Jon's version (reference)

`src/jonhoo/main.rs`. This is the very first version Jon wrote on the stream, from his
first commit
[`jonhoo/brrr@9d0c5ab`](https://github.com/jonhoo/brrr/commit/9d0c5ab5559b33528af685411ccf726746b632b1).
He wrote it on purpose as the simplest correct starting point: a baseline that the rest of
the stream then makes faster step by step. It's not meant to be fast yet, and neither is
mine. I use it with two small changes:

- It takes the input file path as the first command-line argument instead of a hardcoded
  `measurements.txt`, so the same binary works with both input files.
- It prints one station per line instead of the official `{a=…, b=…}` format. That way the
  output can be diffed line by line against mine.

```rust
use std::{collections::BTreeMap, env, fs::File, io::{BufRead, BufReader}};

fn main() {
    let args: Vec<String> = env::args().collect();
    let input_file_path = &args[1];

    let f = File::open(input_file_path).unwrap();
    let f = BufReader::new(f);
    let mut stats = BTreeMap::<String, (f64, f64, usize, f64)>::new();
    for line in f.lines() {
        let line = line.unwrap();
        let (station, temperature) = line.split_once(';').unwrap();
        let temperature: f64 = temperature.parse().unwrap();
        let stats = stats
            .entry(station.to_string())
            .or_insert((f64::MAX, 0., 0, f64::MIN));
        stats.0 = stats.0.min(temperature);
        stats.1 += temperature;
        stats.2 += 1;
        stats.3 = stats.3.max(temperature);
    }
    let mut stats = stats.into_iter().peekable();
    while let Some((station, (min, sum, count, max))) = stats.next() {
        println!("{station}={min:.1}/{:.1}/{max:.1}", sum / (count as f64));
    }
}
```

## Measurements

The full measurements, with every table, command and raw file, are on a separate page:
**[Part 001 — Measurements](part001/measurements.md)**. Here's the short version.

All numbers come from my laptop (see [My machine](notes/my-machine.md)). 1M and 100M are read
from RAM, 1B from the SSD, and a small script records time, memory, swap, disk and CPU
usage during every run ([How I measure](part001/measurements.md#how-i-measure)). The outputs
of both versions are identical on all three files.

| | `david` 100M | `jonhoo` 100M | `david` 1B | `jonhoo` 1B |
|---|---|---|---|---|
| Wall time | 12.5 s | 10.6 s | **266 s** | **107 s** |
| Memory | 1.38 GB | 1.9 MB | **13.8 GB** (peak) | 1.9 MB |
| Swapped to the SSD | 0 | 0 | **~25 GB** | 0 |
| Instructions per row | ~2,000 | ~1,800 | ~2,170 | ~1,840 |

What stands out:

- **Mine needs as much memory as the file is big** (`read_to_string`), Jon's stays at ~2 MB
  (streaming). On the 1B file mine doesn't fit into 16 GB of RAM, macOS swaps ~48 GB in and
  out of the SSD, and it ends up 2.5× slower than Jon's, while doing about the same amount
  of computing ([1B over time](part001/measurements.md#1b-over-time)).
- **Both versions are CPU-bound otherwise.** ~4 instructions per CPU cycle is a lot; the CPU
  isn't waiting, it just has ~2,000 instructions to execute per line.
- **A third of the time goes to comparing station names** in the `BTreeMap` (`memcmp`), a
  fifth to `split(';')`, only ~4 % to parsing the numbers and ~1 % to reading the file
  ([Where the time goes in the code](part001/measurements.md#where-the-time-goes-in-the-code)).
- **Both run on the P-cores**, at ~4.05 GHz, moving between the four P-cores about three
  times per second. Forced onto the E-cores, the same code is 4.6× slower
  ([P-cores vs E-cores](part001/measurements.md#p-cores-vs-e-cores)).
- **Compiling for the M4** (`target-cpu=native`) makes no measurable difference
  ([Compiling for the M4](part001/measurements.md#compiling-for-the-m4-target-cpunative-2026-10-20)).

## What I learned

### 1. `read_to_string` loads the whole file into memory

`read_to_string` reads the entire 13.8 GB file into a single `String` before the loop starts.
The 1B file is 13.8 GB and my machine has 16 GB of RAM. The program did finish, but only
after macOS compressed and swapped most of that memory, and it took 2.5× as long as Jon's
(see [1B over time](part001/measurements.md#1b-over-time)). On a machine with less free RAM it
would not finish at all. On the 1M and 100M files the same code works fine, which makes this
easy to miss.

The general rule: **never assume the input fits in memory.** The usual way around it is
**streaming**: read the file in small chunks and process it as you go. That's what Jon's
version does with `BufReader` + `.lines()`. It reads one buffer at a time (8 KB by default),
so memory use stays constant however big the file is:

```rust
let f = BufReader::new(File::open("input/1B_measurements.txt").unwrap());
for line in f.lines() { /* one line at a time */ }
```

How the two compare:

| | `read_to_string` + `str::lines()` (mine) | `BufReader` + `BufRead::lines()` (Jon's) |
|---|---|---|
| Memory | the **whole file** in one `String`: 1.38 GB for 100M, 13.8 GB for 1B | a buffer of 8 KB, **constant** |
| `read()` system calls | a few huge ones: **2** for the whole 100M file | one per 8 KB: ~170,000 for 100M, ~1.7 million for 1B |
| When processing starts | only after the whole file has been read | right away, with the first buffer |
| Each line | a `&str` pointing into the big `String`, **no allocation** | a new `String` per line, **one allocation per line** |
| UTF-8 check | once, over the whole file | once per line |
| When RAM runs out | the `String` gets compressed and swapped: **~48 GB** of swap on the 1B file | nothing happens, the buffer is tiny |
| Input | only something that fits into RAM | any size, also `stdin` or a pipe |
| 100M from RAM | 12.5 s | 10.6 s |
| 1B from the SSD | **266 s** | **107 s** |

Two things in that table are easy to miss:

- **Mine is not worse in every row.** Because the whole file is already in one `String`,
  `str::lines()` hands out slices into it without allocating anything. Jon's `BufRead::lines()`
  has to copy every line out of the buffer into its own `String`, because the buffer gets
  overwritten by the next read. When I switch to `BufReader`, I'll gain constant memory but
  get an allocation per line that I don't have today.
- **The difference on 100M isn't about reading.** Reading the file is ~1 % of the run time
  (see [Where the time goes in the code](part001/measurements.md#where-the-time-goes-in-the-code)).
  Mine is slower there because of the `Vec` per line and the double map lookup. Only on the 1B
  file, where the `String` doesn't fit into RAM, does the way of reading decide the time.

**Next step:** switch my version to `BufReader`, like Jon's (see [Next steps](#next-steps)).
Reading the file with `mmap` is on the [TODO](#todo) list for later.

### 2. `split_once` instead of `split().collect::<Vec<_>>()`

Mine collects the pieces into a `Vec`, which allocates on the heap **for every line**
(1 billion allocations on the 1B file), just to read two items.

`str::split_once` returns `Option<(&str, &str)>`. It doesn't allocate, and pattern matching
replaces the indexing:

```rust
let (station, temperature) = line.split_once(';').unwrap();
```

The two `&str` don't copy anything, they point **into the original line**. A `&str` is a
[wide pointer](https://doc.rust-lang.org/reference/dynamically-sized-types.html): the address
of the first byte plus a length, 8 + 8 = 16 bytes on a 64-bit CPU. For the line
`Hamburg;12.0`:

```text
the line in memory:   H a m b u r g ; 1 2 . 0
                      ▲               ▲
station     = (pointer ┘, length 7)  "Hamburg"
temperature = (pointer ───────────┘, length 4)  "12.0"
```

So the whole result is 2 × 16 = 32 bytes, on the stack or straight in CPU registers. The
`Option` around it costs nothing extra: a pointer is never null, so Rust uses null to mean
`None`.

| | `split_once` | `split().collect::<Vec<_>>()` |
|---|---|---|
| On the stack | 2 × `&str` = 32 bytes | the `Vec` header: pointer + capacity + length = 24 bytes |
| On the **heap** | nothing | an array for the slices: a `malloc` (typically room for 4 × `&str` = 64 bytes) |
| At the end of each line | nothing | a `free` |
| Checking the number of parts | none, it's `Some` or `None` | `split.len() == 2` |
| How far it searches for `;` | stops at the first one | goes through the **whole** line, looking for more |

The time profile of the 100M run shows both costs (see
[Where the time goes in the code](part001/measurements.md#where-the-time-goes-in-the-code)): ~9 % of the time goes
to allocating and freeing memory, ~20 % to searching for the `;`. `split_once` removes the
allocation completely and makes the search shorter.

**Next step:** use `split_once` (see [Next steps](#next-steps)).

### 3. Looking up a station: `contains_key` + `get_mut`, `entry`, or just `get_mut`

Both versions look up the station in the map for every line, but each pays for something
different.

**Mine** searches the map **twice** when the station is already there: once in
`contains_key` and again in `get_mut`. It only allocates a `String` for the key in the `else`
branch, i.e. for a new station:

```rust
if location_to_measurement.contains_key(location) {                 // lookup 1, with a &str
    let measurement_stats = location_to_measurement.get_mut(location).unwrap();   // lookup 2
    // update
} else {
    location_to_measurement.insert(location.to_string(), /* … */);   // String only here
}
```

**Jon's** uses the `entry` API, which needs only one lookup:

```rust
let stats = map
    .entry(station.to_string())
    .or_insert((f64::MAX, 0., 0, f64::MIN));
stats.0 = stats.0.min(temperature);
// …
```

But `entry` wants an owned key, so `station.to_string()` allocates a `String` for **every
line**, even when the station is already in the map and the `String` is thrown away right
after.

It also initializes a new entry twice: first with neutral values (`f64::MAX` for the minimum,
`f64::MIN` for the maximum, `0` for the sum and the count), which the update right below
immediately overwrites. That keeps a single code path for the first and every later
measurement. It doesn't cost anything noticeable: it only happens when a new station is
inserted, and the insert itself (allocating the `String`, adding it to the tree) costs far
more than writing four numbers twice.

`and_modify` reads nicer, with no neutral values and no double initialization. Exactly one of
the two branches runs:

```rust
map.entry(station.to_string())
    .and_modify(|s| {                        // only if the station is already there
        s.0 = s.0.min(temperature);
        s.1 += temperature;
        s.2 += 1;
        s.3 = s.3.max(temperature);
    })
    .or_insert((temperature, temperature, 1, temperature));   // only if it's new
```

(`or_insert` alone with the first measurement wouldn't work: it doesn't tell whether the
entry was just inserted, so the update below it would count the first measurement twice.)

This is the version I'll switch to next (see [Next steps](#next-steps)): one lookup, no
neutral values, and it reads well.

It still allocates a `String` for every line, though. The fastest option with the standard
library is a single `get_mut` with a `&str`, and `insert` only for a new station:

```rust
if let Some(s) = map.get_mut(station) {      // one lookup, no allocation
    // update
} else {
    map.insert(station.to_string(), (temperature, temperature, 1, temperature));
}
```

For a station that's already in the map:

| | Lookups | `String` allocations |
|---|---|---|
| `contains_key` + `get_mut` (mine) | 2 | 0 |
| `entry(to_string()).or_insert(…)` (Jon's) | 1 | 1 |
| `entry(to_string()).and_modify(…).or_insert(…)` | 1 | 1 |
| **`get_mut`, else `insert`** | **1** | **0** |

For a new station it's the other way round: `get_mut` fails and `insert` searches again, so
`entry` is better there. Which case dominates depends on the data, and the 1BRC rules say
implementations "must not rely on specifics of a given data set". What they do guarantee is
that "there is a maximum of 10,000 unique station names"
([1BRC rules](https://github.com/gunnarmorling/1brc#rules-and-limits)). With a billion rows,
at most 10,000 of them can insert a new station, and all others update an existing one. So
`get_mut` + `insert` is the better choice for **every** valid input, not just my file with
its 413 stations. The lesson: optimize for the specification, not for the sample data.

### 4. `f64::min` / `f64::max` instead of manual `if` comparisons

```rust
stats.0 = stats.0.min(temperature);
stats.3 = stats.3.max(temperature);
```

These are shorter and say what they mean. Mine uses `f32`, which has the same methods
(`f32::min` / `f32::max`).

**Next step:** use `.min()` / `.max()` instead of the `if` comparisons (see
[Next steps](#next-steps)).

### 5. `f32`, `f64` or integers?

I used `f32` for the sum, Jon uses `f64`. Both give the same output on my files, but they
behave very differently once you look at what a float can store.

**How a float stores a number.** A float is stored as `1.xxx… × 2^exponent`. The bits after
the point (the *mantissa*) carry the precision, and the leading `1.` isn't stored, it's
always 1:

```text
f32:  [sign 1 bit][exponent  8 bits][mantissa 23 bits]  → 24 bits of precision
f64:  [sign 1 bit][exponent 11 bits][mantissa 52 bits]  → 53 bits of precision
```

So a float can only store a limited set of values, and the gap between two neighbouring
values grows with the number: between 2^e and 2^(e+1) it is 2^(e − 23) for `f32`. Every
result is rounded to the nearest value that can be stored:

| Value | Gap between neighbouring `f32` values | What that means |
|---|---|---|
| ~1,000 (2¹⁰) | ~0.0001 | tenths are fine |
| ~1,000,000 (2²⁰) | **0.125** | tenths can no longer be stored exactly |
| ~16,700,000 (2²⁴) | **1** | not even every integer |
| ~2,150,000,000 (2³¹) | **256** | adding 99.9 changes nothing (see below) |

`f64` has the same pattern, only much later: it stores every integer exactly up to
2⁵³ ≈ 9 × 10¹⁵.

**My file.** With 413 stations, each station gets ~2.4 million measurements on the 1B file,
so its sum grows to tens of millions. Past ~1 million, `f32` already can't store tenths, and
every addition gets rounded. I expected this to break the 1B results, but **it didn't**: the
output is identical to Jon's `f64` version on all 413 stations. The rounding errors are
small and go both up and down, so they mostly cancel out.

**The worst case the rules allow.** But my file isn't the point: the
[1BRC rules](https://github.com/gunnarmorling/1brc#rules-and-limits) say any distribution
must work, and every temperature is between −99.9 and 99.9. The worst valid input is all
10⁹ rows for **one** station, each with 99.9. The exact sum is 10⁹ × 99.9 = 99,900,000,000.
I ran exactly that, a billion additions of `99.9`:

| | Sum | Mean | Rounded mean |
|---|---|---|---|
| exact | 99,900,000,000 | 99.9 | 99.9 |
| `f64` | 99,899,998,835.9 (off by ~1,164) | 99.8999988… | **99.9** ✅ |
| `f32` | **2,147,483,600** | 2.147… | **2.1** ❌ |

- **`f32` gets stuck.** From 2³¹ ≈ 2.15 × 10⁹ on, the gap between neighbouring values is 256.
  Adding 99.9 lands closer to the old sum than to the next value up (99.9 is less than half
  of 256), so it rounds back to the same sum. The sum stops growing after ~21 million rows,
  and the other ~979 million rows are lost.
- **`f64` survives.** Its sum is off by ~1,164, but divided by a billion rows that's an error
  of ~0.000001 in the mean, far below the 0.05 that would change the printed tenth.

**`f64` still isn't exact.** Values like `12.3` have no exact binary representation, and a
mean that lies exactly halfway between two tenths can round either way. On an earlier
100K-row test file, two stations had such a mean (Gangtok 16.25, Monaco 15.55; the 1BRC rules
round half up, so 16.3 and 15.6 are correct). Each version rounded one of them the wrong way:

| Station | Exact mean | Correct | `jonhoo` (`f64`)            | `david` (`f32`)             |
|---------|------------|---------|-----------------------------|-----------------------------|
| Gangtok | 16.25      | 16.3    | 16.250000000000004 → 16.3 ✅ | 16.249998 → 16.2 ❌          |
| Monaco  | 15.55      | 15.6    | 15.549999999999997 → 15.5 ❌ | 15.550000 → 15.6 ✅          |

Which way a tie rounds comes down to a tiny error in the sum. So Jon's output is a useful
reference, but not a perfect oracle.

**The robust fix: integers in tenths.** Every temperature has exactly one decimal place, so
it can be stored as a whole number of tenths: `12.3` becomes `123`, `-5.0` becomes `-50`.
Integer sums are exact, and the mean is rounded once, at the end, from an exact value. Each
value can use the smallest type that fits the worst case:

| Value | Type | Worst case |
|---|---|---|
| sum | `i64` | ±999 × 10⁹ ≈ ±10¹²: needs a sign, and more than `i32` (±2.1 × 10⁹) |
| count | `u32` | 10⁹, fits under 4.29 × 10⁹ |
| min / max | `i16` | ±999 |

Together that's 16 bytes per station instead of the 32 bytes of Jon's
`(f64, f64, usize, f64)`, so more stations fit into the CPU caches.

I'm not doing this yet, Jon will almost certainly get there in the stream. It's on the
[TODO](#todo) list, together with parsing the temperature straight into tenths.

### 6. Struct vs tuple

Jon uses a tuple `(f64, f64, usize, f64)` and accesses it as `stats.0`, `stats.3`, and so on.
I use a named struct, `MeasurementStats`. Jon says himself that it should really be a
struct; the tuple is just the quick way to start.

**Performance is the same.** A tuple and a struct with the same fields take the same memory
(`(f64, f64, usize, f64)` and a struct with those four fields are both 32 bytes), and
`stats.0` and `stats.min` compile to the same instruction: a read at a fixed offset. My
16 bytes vs Jon's 32 come from the types (`f32` and `u32` vs `f64` and `usize`), not from
struct vs tuple.

The difference is in how you work with it:

| | Tuple `(f64, f64, usize, f64)` | Struct `MeasurementStats` |
|---|---|---|
| Access | `stats.0`, `stats.3`: you have to remember which is which | `stats.min`, `stats.max`: reads on its own |
| Mixing up fields | `stats.0` vs `stats.3` (min vs max), the compiler can't tell | `stats.min` can't be confused with `stats.max` by accident |
| Type | anonymous: every `(f64, f64, usize, f64)` is the same type | **its own type**: can't be mixed up with some other four numbers |
| Methods | not possible | `impl MeasurementStats { fn add(&mut self, t) … }` |
| Traits | limited | `#[derive(Debug, Default)]`, a custom `Display` for the output, … |
| Extra code | none | the struct definition, a few lines |

The biggest win is methods: the update logic can live in the struct, for example a `new`
that creates the stats from the first measurement and an `add` that updates them with the
next one. The loop then just calls `s.add(temperature)` or
`MeasurementStats::new(temperature)` and reads like a description of what happens. I'll
write these methods in a later part.

**Next step:** give `MeasurementStats` the methods `new` and `add` (see
[Next steps](#next-steps)).

### 7. Format string details

The two versions print the same line in two different ways.

Mine:

```rust
println!("{}={1:.1}/{2:.1}/{3:.1}", location, stats.min, stats.sum / stats.count as f32, stats.max);
```

Jon's:

```rust
println!("{station}={min:.1}/{:.1}/{max:.1}", sum / (count as f64));
```

There are three ways to refer to a value in a format string. After the colon comes the
format: `{:.1}`, `{1:.1}` and `{min:.1}` all mean "round to one decimal place".

| Syntax | How it works | Example |
|---|---|---|
| `{}` | **implicit position**: takes the arguments in order | `println!("{}={}", a, b)` |
| `{1}` | **explicit position**: the argument with index 1 (counting from 0) | `println!("{1}={0}", a, b)` prints `b=a` |
| `{name}` | **captured variable** (Rust 2021+): uses the variable with that name from the surrounding code | `println!("{station}")` |

Compared:

| | Mine | Jon's |
|---|---|---|
| Station name | `{}`, implicit position 0 | `{station}`, captured variable |
| Min, max | `{1:.1}`, `{3:.1}`, explicit positions | `{min:.1}`, `{max:.1}`, captured variables |
| Mean | `{2:.1}`, explicit position | `{:.1}`, implicit position (the only argument after the string) |
| Readability | you have to count indices and look over to the arguments | the names are right in the string, it reads like the output `Hamburg=…` |
| Styles mixed | `{}` and `{1}` | `{station}` and `{:.1}` |

Why doesn't Jon capture everything? Only a plain variable can be captured, not an expression
like `sum / count` and not a field like `stats.min`. Jon unpacks the tuple into the variables
`station`, `min`, `sum`, `count` and `max` in the `while let`, so he can capture those, and
passes the mean, which is an expression, as an argument. Mine keeps the values in a struct
(`stats.min`), so it can't capture them as they are.

None of these styles is faster: `println!` is a macro, and the format string is taken apart
at compile time. What does cost something is how `println!` writes: in the System Trace of
the 100M run, my version made **413 `write` system calls**, one per line of output. A
`BufWriter` would collect the lines and write them in a few big calls. That's on the
[TODO](#todo) list.

**Next step:** rewrite my `println!` so the names are in the string (see
[Next steps](#next-steps)).

### 8. Silent vs loud failure

Mine skips malformed lines without saying anything (`if split.len() == 2`). Jon's
`.unwrap()` panics on them instead. For a benchmark with a known-good input, failing loudly
is arguably better, because a skipped line would quietly produce a wrong result.

**Next step:** fail loudly like Jon's version (see [Next steps](#next-steps)).

## Next steps

1. **Switch my version to `BufReader`**, like Jon's, so it streams the file instead of
   loading it all with `read_to_string`. On the 1B file the memory should stay small and the
   swapping should disappear.
2. **Use `split_once` instead of `split(';').collect::<Vec<_>>()`**, so there's no heap
   allocation per line and the search for `;` stops at the first one.
3. **Use `entry().and_modify().or_insert()`** instead of `contains_key` + `get_mut` +
   `insert`: one lookup instead of two, and a new station gets its first measurement
   directly, without neutral values. It still allocates a `String` per line for the key;
   `get_mut` + `insert` would avoid that (see
   [What I learned #3](#3-looking-up-a-station-contains_key--get_mut-entry-or-just-get_mut)).
4. **Use `.min()` / `.max()` instead of the manual `if` comparisons** for the minimum and the
   maximum: shorter, and it says what it means.
5. **Give `MeasurementStats` the methods `new` and `add`**, so the loop just calls
   `s.add(temperature)` / `MeasurementStats::new(temperature)` and the update logic lives in
   one place. That also makes the later switch to integers a change inside the struct only.
6. **Rewrite my `println!` with captured variables**, so the format string reads like the
   output (`{location}={min:.1}/{mean:.1}/{max:.1}`) instead of counting positions. That
   means getting the values out of the struct into plain variables first, and computing the
   mean into its own variable.
7. **Fail loudly on a malformed line**, like Jon's version: instead of silently skipping a
   line that doesn't split into two parts, stop with an error. With `split_once` (step 2)
   that's `.unwrap()` on its result (it's `None` when the line has no `;`), and the
   temperature that doesn't parse already fails through `.unwrap()` on `parse`. A broken
   input then can't quietly produce a wrong result.

## TODO

Things I want to try later. I'm not doing them yet, because Jon may get to them in a later
part of the stream, and I don't want to get ahead of him.

- **Read the file with `mmap` instead of `BufReader`.** No copying from the file cache into
  a buffer, no swapping on the 1B file, and an easy way to split the file between threads
  (see [What I learned #1](#1-read_to_string-loads-the-whole-file-into-memory)).
- **Use a `HashMap` instead of a `BTreeMap`** and sort the stations once at the end. One
  hash and ~1 string comparison per lookup instead of ~10 comparisons (see
  [What I learned #3](#3-looking-up-a-station-contains_key--get_mut-entry-or-just-get_mut)).
- **Store the temperatures as integers in tenths** (`12.3` → `123`): `i64` for the sum,
  `u32` for the count and `i16` for min / max. Exact for every valid input, where `f32` gets
  stuck at 2³¹ and `f64` is slightly off, and 16 bytes per station instead of 32 (see
  [What I learned #5](#5-f32-f64-or-integers)).
- **Parse the temperature by hand** into tenths, skipping the dot, instead of
  `parse::<f32>()`: the format is fixed by the rules, so a few steps are enough instead of a
  general float parser (~4 % of the time today, see
  [What I learned #5](#5-f32-f64-or-integers)).
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
