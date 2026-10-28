# Part 004

**Dates:** 2026-10-26 – 2026-10-28
**Video progress:** stopped at [57:56](https://www.youtube.com/watch?v=tCY7p6dVAGE&t=3476s) when this part was published
**Code:** git tag [`part004`](https://github.com/davidhruby/brrr/tree/part004). To get the code exactly as it was for this part: `git checkout part004`

New here? Start with the [Introduction](intro.md).

## What changed

### 1. `mmap` instead of `BufReader`

The file isn't read through a buffer any more. `mmap` maps the whole file into the address
space of the program, and the loop reads the bytes right where they are, in the file cache.
`split` on the mapped bytes gives each line as a `&[u8]` into that memory: no copy into a
buffer, no new `Vec` for every line. `madvise` with `MADV_SEQUENTIAL` tells the kernel the
file is read from start to end.

Before:

```rust
let file = File::open(input_file_path).unwrap();
let reader = BufReader::new(file);

let mut stats: HashMap<Vec<u8>, MeasurementStats> = HashMap::new();
for line in reader.split(b'\n') {
    let line = line.unwrap();
    let mut fields = line.rsplitn(2, |c| *c == b';');
```

After:

```rust
let file = File::open(input_file_path).unwrap();
let map = mmap(&file);

let mut stats: HashMap<Vec<u8>, MeasurementStats> = HashMap::new();
for line in map.split(|c| *c == b'\n') {
    if line.is_empty() {
        break;
    }
    let mut fields = line.rsplitn(2, |c| *c == b';');
```

```rust
fn mmap(file: &File) -> &'static [u8] {
    let len = file.metadata().unwrap().len() as usize;
    unsafe {
        let ptr = libc::mmap(
                std::ptr::null_mut(),
                len as libc::size_t,
                libc::PROT_READ,
                libc::MAP_SHARED,
                file.as_raw_fd(),
                0,
            );
        if ptr == libc::MAP_FAILED {
            panic!("mmap failed: {:?}", std::io::Error::last_os_error());
        } else {
            if libc::madvise(ptr, len, libc::MADV_SEQUENTIAL) != 0 {
                panic!("madvise  failed: {:?}", std::io::Error::last_os_error());
            } else {
                std::slice::from_raw_parts(ptr as *const u8, len)
            }
        }
    }
}
```

Measurements after this change (raw output in
[`measurements/part004/`](../measurements/part004/)):

| | `david` 100M | `david` 1B |
|---|---|---|
| Wall time | 2.8 s | 47 s |
| CPU time (user + sys) | 2.7 s | 27.7 s |
| Peak memory footprint | 1.9 MB | 8.0 MB |
| Swapped to the SSD | 0 | 0 |
| Instructions per row | ~465 | ~477 |
| Page faults | 84,214 | 842,023 |

**Half the instructions per row** (1B ~940 → ~477), and the CPU time on 1B dropped from
48 s to 28 s. But the wall time on 1B stayed the same (49 s → 47 s): the program now spends
~19 s **waiting for the SSD**. On 100M, read from the RAM disk, there's almost nothing to
wait for, and the wall time dropped from 5.1 s (Part 003, change #5, before LTO) to 2.8 s.

There's one page fault for every 16 KB page of the file (13.8 GB / 16 KB = 842,009). On each
one, the program stops until the page is in RAM. With `BufReader`, the kernel read ahead in
big requests (~247 KB per request in `iostat`) while the program was working. With `mmap`,
macOS reads only ~53 KB per request, one after another, so the SSD delivers only ~260 MB/s
instead of the ~2.3 GB/s it can do. The CPU used to be the bottleneck and hid this; now the
SSD is.

## What I learned

## Next steps

## TODO

Things I want to try later. I'm not doing them yet, because Jon may get to them in a later
part of the stream, and I don't want to get ahead of him.

- **Read ahead of the program on 1B.** Tell the kernel early which part of the file comes
  next, so it reads from the SSD while the program is working: `madvise` with
  `MADV_WILLNEED` (on the whole file, or on a window ahead of the current position), or the
  macOS-specific `fcntl` with `F_RDADVISE`. Or a second thread that touches one byte of every
  page ahead of the main thread, so the page faults and the waiting happen there. If it works,
  the page faults in `time.txt` turn into page reclaims (the page is already in RAM).
- **Check the waiting in Instruments.** The `thread-state` table of a System Trace shows how
  long the thread was running and how long it was blocked, which would confirm the 19 s
  directly instead of from `real` − `user` − `sys`.
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
- **Pin the program to one core on Linux** (`taskset`) and see whether the jumps between
  cores cost anything measurable (see [Something to try later](notes/measurements.md#exact-check-with-instruments)).
- **Find out why the `HashMap` version runs more instructions per cycle** (5.0 instead of 4.3,
  see [Part 003, change #2](part003-hashmap-bytes-and-lto.md#2-hashmap-instead-of-btreemap-sorted-only-for-the-output)).
  My guess: the linear search in a B-tree node ends at a different key for every station,
  which is hard for the CPU to predict, while the `HashMap` lookup mostly goes the same way
  every time. Measure the branch mispredictions of both versions, and compare the `BTreeMap`
  used for sorting the output with another way of sorting the stations.
