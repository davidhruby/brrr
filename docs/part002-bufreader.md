# Part 002 — Streaming with `BufReader`

**Dates:** 2026-10-21 – 2026-10-21
**Video progress:** stopped at [38:38](https://www.youtube.com/watch?v=tCY7p6dVAGE&t=2318s) when this part was published
**Code:** git tag [`part002`](https://github.com/davidhruby/brrr/tree/part002). To get the code exactly as it was for this part: `git checkout part002`

New here? Start with the [Introduction](intro.md).

## What changed

Step 1 of the [Next steps from Part 001](part001-first-naive-version.md#next-steps): my version
now streams the file with `BufReader`, like Jon's, instead of loading it all with
`read_to_string`. Nothing else changed.

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

## Measurements

Measured with [`scripts/measure.sh`](../scripts/measure.sh), the same way as in Part 001
([How I measure](part001/measurements.md#how-i-measure)). Raw output in
[`measurements/part002/`](../measurements/part002/). The output is identical to Jon's on all
three files.

| | `david` 100M | `jonhoo` 100M | `david` 1B | `jonhoo` 1B |
|---|---|---|---|---|
| Wall time | 15.3 s | 10.6 s | **146 s** | **107 s** |
| Memory | 1.9 MB | 1.9 MB | 1.9 MB | 1.9 MB |
| Swapped to the SSD | 0 | 0 | 0 | 0 |
| Instructions per row | ~2,500 | ~1,800 | ~2,560 | ~1,840 |

Jon's numbers are from Part 001 (same code, same machine).

Compared with [Part 001](part001-first-naive-version.md#measurements), my 1B run went from
**266 s to 146 s** (1.8× faster) and from 13.8 GB of memory to 1.9 MB: the swapping is gone.
Every row costs more, though (~2,000 → ~2,500 instructions): `BufRead::lines()` allocates a
new `String` for each line, while `str::lines()` on the big `String` didn't. That's why 100M,
which never swapped, got slower (12.5 s → 15.3 s, part of it waiting for a RAM disk that was
partly in swap).

## What I learned

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
