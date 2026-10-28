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

Measurements after this change (raw output from commit
[`400953f`](https://github.com/davidhruby/brrr/tree/400953f/measurements/part004)):

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

### 2. `MADV_WILLNEED` on the whole file

An attempt to make the kernel read ahead: instead of `MADV_SEQUENTIAL`, `madvise` gets
`MADV_WILLNEED`, "I'll need this soon, start reading it now", for the whole mapped file.

Before:

```rust
if libc::madvise(ptr, len, libc::MADV_SEQUENTIAL) != 0 {
```

After:

```rust
if libc::madvise(ptr, len, libc::MADV_WILLNEED) != 0 {
```

Measurements after this change (raw output from commit
[`7f3994e`](https://github.com/davidhruby/brrr/tree/7f3994e/measurements/part004)):

| | `david` 100M | `david` 1B |
|---|---|---|
| Wall time | 3.1 s | 63 s |
| CPU time (user + sys) | 2.9 s | 31.2 s |
| Peak memory footprint | 1.8 MB | 7.9 MB |
| Swapped to the SSD | 0 | 0 |
| Instructions per row | ~466 | ~524 |
| Page faults | 84,214 | 1,671,039 |
| Read from the SSD | | 27.0 GB |

**Slower on 1B: 47 s → 63 s.** macOS takes the advice, and too literally. In the first ~11 s
it reads the file ahead in big requests (300 – 420 KB per request) at **~1.5 GB/s**, so the SSD
can do it. But that's ~14 GB in 11 s, while the program gets through only ~5 GB of it. The
free RAM runs out (529,035 free pages before the run, 4,985 after), and to make room the kernel
drops pages the program hasn't reached yet. When it gets there, they're read again, page by
page at ~250 MB/s as in change #1. Most of the file is read **twice** (27.0 GB instead of
13.8 GB), there are twice as many page faults, and the kernel's extra work shows in `sys`
(1.9 s → 4.6 s).

On 100M (1.38 GB, fits in RAM), the instructions per row are the same and the wall time is
a bit higher (2.8 s → 3.1 s) in a single run.

### 3. `MADV_WILLNEED` in 512 MB chunks, `MADV_DONTNEED` behind

So the advice has to be limited to what fits in RAM. The file is split into 512 MB chunks.
At the start, the program advises the first chunk. Whenever it's halfway through a chunk, it
advises the next one with `MADV_WILLNEED`, so the kernel has the rest of the current chunk
(256 MB) as a head start. At the same moment it tells the kernel with `MADV_DONTNEED` that
the chunk before the current one isn't needed any more. The slice the loop reads still covers
the whole file; the chunks only decide when and where to give the advice, so a chunk boundary
in the middle of a line doesn't matter. `madvise` needs a page-aligned address: the mapping
starts on a page and the chunks are multiples of 16 KB, so every chunk start is aligned.

Before (changes #1 and #2):

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

After (`mmap` now returns only the pointer, without `madvise`):

```rust
let file = File::open(input_file_path).unwrap();
let file_len: usize = file.metadata().unwrap().len() as usize;
let m_advise_size: usize = 512 * 1024 * 1024;
let mut m_adivse_block_index = 0;
let when_to_start_m_advise = m_advise_size / 2;
let ptr = mmap(&file, file_len);

if unsafe { libc::madvise(ptr, m_advise_size as libc::size_t, libc::MADV_WILLNEED) } != 0 {
    panic!("madvise  failed: {:?}", std::io::Error::last_os_error());
}
let map = unsafe { std::slice::from_raw_parts(ptr as *const u8, file_len) };
let map_start_byte = map.as_ptr() as usize;
let mut next_m_advise_byte = map_start_byte + m_advise_size;
let map_end_byte = map_start_byte + file_len;

let mut stats: HashMap<Vec<u8>, MeasurementStats> = HashMap::new();
for line in map.split(|c| *c == b'\n') {
    if line.is_empty() {
        break;
    }

    if (next_m_advise_byte - line.as_ptr() as usize) < when_to_start_m_advise {
        if (next_m_advise_byte + m_advise_size) > map_end_byte {
            if unsafe { libc::madvise(next_m_advise_byte as *mut c_void, (map_end_byte - next_m_advise_byte) as libc::size_t, libc::MADV_WILLNEED) } != 0 {
                panic!("madvise MADV_WILLNEED failed: {:?}", std::io::Error::last_os_error());
            }
            next_m_advise_byte = usize::MAX;
        } else {
            if unsafe { libc::madvise(next_m_advise_byte as *mut c_void, m_advise_size as libc::size_t, libc::MADV_WILLNEED) } != 0 {
                panic!("madvise MADV_WILLNEED failed: {:?}", std::io::Error::last_os_error());
            }
            next_m_advise_byte += m_advise_size;
        };
        if m_adivse_block_index > 0 {
            if unsafe { libc::madvise((map_start_byte + ((m_adivse_block_index - 1) * m_advise_size)) as *mut c_void, m_advise_size as libc::size_t, libc::MADV_DONTNEED) } != 0 {
                panic!("madvise MADV_DONTNEED failed: {:?}", std::io::Error::last_os_error());
            }
        }
        m_adivse_block_index += 1;
    }

    let mut fields = line.rsplitn(2, |c| *c == b';');
```

Measurements after this change (raw output from commit
[`7369f31`](https://github.com/davidhruby/brrr/tree/7369f31/measurements/part004)):

| | `david` 100M | `david` 1B |
|---|---|---|
| Wall time | 2.9 s | 37 s |
| CPU time (user + sys) | 2.8 s | 28.1 s |
| Peak memory footprint | 1.9 MB | 7.9 MB |
| Swapped to the SSD | 0 | 0 |
| Instructions per row | ~471 | ~479 |
| Page faults | 84,214 | 842,022 |
| Read from the SSD | | 13.6 GB |

**1B: 47 s (change #1) → 37 s.** The file is read once again (13.6 GB), in big requests
(~276 KB), and the waiting dropped from ~19 s to ~9 s. I also tried 128 MB and 256 MB chunks,
and without `MADV_DONTNEED`: all of them came out at 38 – 39 s (raw output in the
`measurements/part004-mmap-progressive-madvise*` folders), so the chunk size doesn't matter.

But the SSD isn't the bottleneck any more. Per second, `iostat` shows bursts of ~510 MB/s
and in between seconds where it reads almost nothing (2, 5, 31, 36 MB/s): the kernel reads a
chunk faster than the program gets through it, and then waits for the next advice. Yet the
program still waits ~9 s, and the number of page faults hasn't changed: one for every page,
842,022. So the waiting isn't only for the SSD, but also on the page faults themselves.

The 1M file (~14 MB) crashes with this version: the first `madvise` asks for 512 MB, more than
the file, and `madvise` fails.

### 4. A helper thread that touches the pages ahead

If the waiting is on the page faults, someone else should take them. A second thread gets the
chunks over a channel instead of the main thread advising them itself. For each chunk it calls
`MADV_WILLNEED` on it and `MADV_DONTNEED` on the chunk two before it (the one behind the
main thread), and then reads one byte
of every 16 KB page in it with `read_volatile` (a volatile read the compiler can't drop, even
though the value isn't used). The page faults, and the waiting with them, now happen in the
helper thread. Threads of one process share the page tables, so when the main thread gets to
a page, it's already mapped and there's no fault left for it.

Before (change #3, advising in the main thread):

```rust
if (next_m_advise_byte - line.as_ptr() as usize) < when_to_start_m_advise {
    if (next_m_advise_byte + m_advise_size) > map_end_byte {
        if unsafe { libc::madvise(next_m_advise_byte as *mut c_void, (map_end_byte - next_m_advise_byte) as libc::size_t, libc::MADV_WILLNEED) } != 0 {
            panic!("madvise MADV_WILLNEED failed: {:?}", std::io::Error::last_os_error());
        }
        next_m_advise_byte = usize::MAX;
    } else {
        if unsafe { libc::madvise(next_m_advise_byte as *mut c_void, m_advise_size as libc::size_t, libc::MADV_WILLNEED) } != 0 {
            panic!("madvise MADV_WILLNEED failed: {:?}", std::io::Error::last_os_error());
        }
        next_m_advise_byte += m_advise_size;
    };
    if m_adivse_block_index > 0 {
        if unsafe { libc::madvise((map_start_byte + ((m_adivse_block_index - 1) * m_advise_size)) as *mut c_void, m_advise_size as libc::size_t, libc::MADV_DONTNEED) } != 0 {
            panic!("madvise MADV_DONTNEED failed: {:?}", std::io::Error::last_os_error());
        }
    }
    m_adivse_block_index += 1;
}
```

After (the helper thread, started at the beginning of `main`):

```rust
const M_ADVISE_SIZE: usize = 512 * 1024 * 1024;

let (tx, rx) = mpsc::channel::<(usize, usize, usize)>();

thread::spawn(move || {
    for (ptr, len, m_adivse_block_index) in rx {
        if unsafe { libc::madvise(ptr as *mut c_void, len as libc::size_t, libc::MADV_WILLNEED) } != 0 {
            panic!("madvise MADV_WILLNEED failed: {:?}", std::io::Error::last_os_error());
        }
        if m_adivse_block_index > 0 {
            if unsafe { libc::madvise((ptr - M_ADVISE_SIZE - M_ADVISE_SIZE) as *mut c_void, M_ADVISE_SIZE as libc::size_t, libc::MADV_DONTNEED) } != 0 {
                panic!("madvise MADV_DONTNEED failed: {:?}", std::io::Error::last_os_error());
            }
        }
        let page_count = len / (16 * 1024);
        for page_index in 0..page_count {
            let byte_position = ptr + (page_index * 16 * 1024);
            let y = byte_position as *const u8;
            unsafe { std::ptr::read_volatile(y) };
        }
    }
});
```

and in the loop, the main thread only sends the next chunk:

```rust
if (next_m_advise_byte - line.as_ptr() as usize) < when_to_start_m_advise {
    let m_adivse_block_index = (next_m_advise_byte - (map_start_byte + M_ADVISE_SIZE)) / M_ADVISE_SIZE;
    if (next_m_advise_byte + M_ADVISE_SIZE) > map_end_byte {
        tx.send((next_m_advise_byte, (map_end_byte - next_m_advise_byte), m_adivse_block_index)).unwrap();
        next_m_advise_byte = usize::MAX;
    } else {
        tx.send((next_m_advise_byte, M_ADVISE_SIZE, m_adivse_block_index)).unwrap();
        next_m_advise_byte += M_ADVISE_SIZE;
    };
}
```

Measurements after this change (raw output in
[`measurements/part004/`](../measurements/part004/)):

| | `david` 100M | `david` 1B |
|---|---|---|
| Wall time | 2.8 s | 28 s |
| CPU time (user + sys, both threads) | 2.8 s | 29.4 s |
| Peak memory footprint | 1.9 MB | 7.9 MB |
| Swapped to the SSD | 0 | 0 |
| Instructions per row | ~477 | ~486 |
| Page faults | 84,217 | 842,026 |
| Read from the SSD | | 13.8 GB |

**1B: 37 s → 28 s**, and from Part 003 (48.9 s) it's **−43 %**. The wall time is now about the
CPU time: the main thread doesn't wait any more and the program is limited by computing again.
The number of page faults is the same, they just happen in the helper thread. The SSD reads
evenly at ~510 MB/s, as fast as the main thread gets through the data (13.8 GB / 28 s), and
the idle seconds from change #3 are gone. So the ~9 s of waiting in change #3 were the page
faults themselves, not the SSD. Most likely, on macOS a thread that hits a page fault on a
mapped file waits in the kernel even when the page is already in RAM; I haven't checked that
in Instruments yet.

On the 1M file, the helper thread panics on the first `madvise` (512 MB, more than the file),
but the main thread doesn't depend on it and the output is still correct.

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
