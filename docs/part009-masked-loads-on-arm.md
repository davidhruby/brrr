# Part 009 — Masked loads on ARM

**Dates:** 2026-11-07 – 2026-11-07
**Video progress:** stopped at [5:02:58](https://www.youtube.com/watch?v=tCY7p6dVAGE&t=18178s) when this part was published
**Code:** git tag [`part009`](https://github.com/davidhruby/brrr/tree/part009). To get the code exactly as it was for this part: `git checkout part009`

New here? Start with the [Introduction](intro.md).

## What changed

### 1. The question: why is Jon's code 4.6× slower on the Graviton?

The same code, Jon's [`src/jonhoo/main.rs`](../src/jonhoo/main.rs) at this point of the stream,
with the 1B file in RAM (`tmpfs` on Linux, see below):

| | Jon's PC (x86, from the stream) | AWS Graviton3 (`r7g.xlarge`) |
|---|---|---|
| Wall time | 25.05 s | 88.6 s |
| User time | 19.17 s | 87.6 s |
| Sys time | 3.37 s | 1.0 s |

The clock explains only part of it. The Graviton3 runs at ~2.6 GHz; a desktop x86 CPU at about
5 GHz (I don't know Jon's exact CPU, so this is an estimate). Per row that's roughly:

| | Jon's PC (estimated) | Graviton3 (measured) |
|---|---|---|
| Cycles per row | ~95 (19.17 s × ~5 GHz / 1 billion) | ~229 |

So even per cycle, the Graviton needs ~2.4× more. The counters on the Graviton (1B, per row,
`perf stat` with `:u`, see [Part 008](part008-short-strings.md)):

| Per row | Jon's code | My code (Part 007) |
|---|---|---|
| Instructions | ~402 | ~183 |
| Branches | ~124 | ~26 |
| Mispredicted branches | ~2.4 | ~0.6 |
| Loads through the dTLB | ~66 | ~39 |
| Cycles | ~229 | ~51 |

**The suspect: `u8x64::load_or_default` in Jon's `split_semi`.** `perf annotate` on the Graviton
shows what the compiler made of it: a 64-bit mask of which bytes belong to the line, then for
each of the 64 bytes a conditional branch on one bit of the mask and a load of **one byte**:

```text
addp …  /  umov …  /  bfi …  /  orr x8, …   build a 64-bit mask: which bytes belong to the line
tbz   w8, #0, …                              does byte 0 belong? if not, skip
ldr   b0, [x27]                              load ONE byte
tbnz  w8, #1, …                              does byte 1 belong? …
…                                            and so on for all 64 bytes
```

ARM (NEON) has no masked load, "load these 64 bytes, but only the allowed ones". x86 with
AVX-512 has one, a single instruction that loads 64 bytes and zeroes the ones outside the mask.
That would fit the numbers: the many branches and one-byte loads per row on the Graviton, and
the ~2.4 mispredictions per row (the branch at the end of the line depends on its length, ~11
cycles each, ~26 cycles per row). My own code loads whole 64-byte blocks with `from_slice` and
uses `load_or_default` only once, at the end of the file.

That's a hypothesis so far. What I want to find out in this part:

- **Is it really `load_or_default`?** A benchmark of `split_semi` alone on the Graviton and the
  M4: Jon's `load_or_default`, a plain `from_slice` of 64 bytes (when 64 bytes are available),
  and `rsplitn`.
- **What does it compile to on x86?** The same code on an x86 instance with AVX-512, and its
  instructions (`perf annotate`): is it really one masked load there?
- **How fast is Jon's code on x86?** The cycles per row on an x86 instance, against the ~229 on
  the Graviton.

**A RAM disk on Linux.** The macOS RAM disk (`diskutil … ram://…`) has an equivalent on Linux,
`tmpfs`, a file system that lives in memory. Unlike the page cache, the kernel can't drop its
pages, so no run reads from the disk:

```sh
sudo mkdir -p /mnt/ramdisk
sudo mount -t tmpfs -o size=15G tmpfs /mnt/ramdisk
cp /home/ubuntu/brrr/input/1B_measurements.txt /mnt/ramdisk/
# … measure with /mnt/ramdisk/1B_measurements.txt …
sudo umount /mnt/ramdisk
```

(Ubuntu also has a `tmpfs` at `/dev/shm`, half the RAM by default.)

### 2. A benchmark of the load alone

To see whether it's really the load, three ways to split a line at the `;` that differ **only
in how the 64-byte vector is made**, everything else (`simd_eq`, `first_set`, slicing the line)
the same, in [`benches/parse_semicolon_criterion.rs`](../benches/parse_semicolon_criterion.rs):

| Variant | Input | How the 64 bytes are loaded |
|---|---|---|
| **A** (Jon) | only the line | `u8x64::load_or_default(line)` |
| **B** | only the line | copy the line into a zeroed `[0u8; 64]`, then `u8x64::from_array(buf)` |
| **C** | the whole file and where the line starts | `u8x64::from_slice(&data[start..start + 64])`, one load without a mask |

A and B take the same input and give the same result: the line padded with zeros to 64 bytes.
B just does it by hand. C reads past the end of the line, which is fine, because the line goes
on with the next lines in the file, and the first `;` in those 64 bytes is always the one of
this line (the station comes before the temperature). Only for the last lines of the file,
with fewer than 64 bytes left, it falls back to `load_or_default`.

```rust
// B: same input as A (only the line), but the masked load done by hand:
// copy the line into a zeroed 64-byte buffer, then one plain load of the whole buffer
pub fn split_semicolon_simd_copy(line: &[u8]) -> (&[u8], &[u8]) {
    const DELIMITER: u8x64 = u8x64::splat(b';');
    if line.len() > 64 {
        // fallback to normal not optimized approach
        return line.rsplit_once(|c| *c == b';').unwrap();
    }
    let mut buf = [0u8; 64];
    buf[..line.len()].copy_from_slice(line);
    let del_eq = DELIMITER.simd_eq(u8x64::from_array(buf));
    // SAFETY: we are promised there is a ; in every line
    let index_of_delim = unsafe { del_eq.first_set().unwrap_unchecked() };
    (&line[..index_of_delim], &line[index_of_delim + 1..])
}

// C: load 64 bytes from the whole file, starting at the line, without any mask;
// the bytes after the end of the line don't matter, the first ';' is always in the line
pub fn split_semicolon_simd_from_data<'a>(data: &'a [u8], start: usize, line: &'a [u8]) -> (&'a [u8], &'a [u8]) {
    const DELIMITER: u8x64 = u8x64::splat(b';');
    if line.len() > 64 {
        // fallback to normal not optimized approach
        return line.rsplit_once(|c| *c == b';').unwrap();
    }
    let simd_value = if start + 64 <= data.len() {
        u8x64::from_slice(&data[start..start + 64])
    } else {
        // the last lines of the file: fewer than 64 bytes left
        u8x64::load_or_default(line)
    };
    let del_eq = DELIMITER.simd_eq(simd_value);
    // SAFETY: we are promised there is a ; in every line
    let index_of_delim = unsafe { del_eq.first_set().unwrap_unchecked() };
    (&line[..index_of_delim], &line[index_of_delim + 1..])
}
```

Before measuring, the benchmark checks once that all three give the same result as `rsplitn`
for every line of the 1M file. On the M4 (nightly), per line:

| 1M file, per line | Time | Against A |
|---|---|---|
| `rsplitn` (no SIMD) | 2.65 ns | 6.5× faster |
| **A:** `load_or_default` (Jon) | **17.28 ns** | – |
| **B:** copy into `[0u8; 64]`, `from_array` | 7.80 ns | 2.2× faster |
| **C:** `from_slice` from the whole file | **1.29 ns** | **13× faster** |

- **A against B**, the same input and the same result, only another way of doing it: **2.2×**.
  So on ARM, `load_or_default` really is most of the cost: up to 64 one-byte loads behind
  branches cost more than a copy of a variable length plus one big load.
- **B is still slower than `rsplitn`**: zeroing 64 bytes and the copy of a variable length on
  every line cost something too.
- **C is 13× faster than A and 2× faster than `rsplitn`**: one 64-byte load, no mask, no copy,
  no branches. Only like this does SIMD pay off for finding the `;`, and it's what my iterator
  does.

## What I learned

## Next steps

## TODO

Things I want to try later. I'm not doing them yet, because Jon may get to them in a later
part of the stream, and I don't want to get ahead of him.

- **A smaller table on Linux:** with 4 KB pages, the 16,384 buckets of
  `with_capacity(10_000)` spread over ~130 – 160 pages and cause ~0.75 dTLB misses per row.
  Measure `HashMap::new()` (512 buckets, ~20 KB, 5 pages) there; on the M4, the capacity was
  7 % faster.
- **`StrVec` with many more stations.** With 413 names everything is in the L1 cache, so keeping
  the names inside the keys saves nothing (changes #1 and #2). With 10,000 stations (the most
  the rules allow), the names on the heap wouldn't fit in L1 any more, and it might pay off.
- **Handle lines longer than 64 bytes in the SIMD iterator** ([Part 006, change #4](part006-finding-the-end-of-a-line.md#4-finding-the--and-the-n-of-several-lines-at-once-simd-iterator)): when a freshly
  loaded block has no `\n`, fall back to `next_line_swar` + `split_semicolon` for that line,
  and add a test with such a line. The rules allow station names of up to 100 bytes.
- **Try [samply](https://github.com/mstange/samply)**, a sampling profiler written in Rust
  that opens the profile in the Firefox Profiler (call tree, inverted call tree, flame graph).
  It demangles Rust's v0 symbol names itself, so it would work without the nightly build that
  Instruments needs for readable names.
- **Try [hotpath](https://hotpath.rs/)**, a Rust profiler used from the code: functions are
  marked with `#[hotpath::measure]` (and `main` with `#[hotpath::main]`), and it reports their
  time and allocations, and whether they're CPU- or I/O-bound.
- **Find out what the program waits for on 100M** since Part 005 (~0.6 – 0.8 s of wall time
  without CPU time, see [Part 005](part005-parsing-temperature.md#3-parsing-without-branches) and [Part 006](part006-finding-the-end-of-a-line.md#4-finding-the--and-the-n-of-several-lines-at-once-simd-iterator)), for example with the thread states in a System Trace. One
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
