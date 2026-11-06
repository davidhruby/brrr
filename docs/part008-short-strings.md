# Part 008 — Short strings inline

**Dates:** 2026-11-06 – 2026-11-06
**Video progress:** stopped at [5:02:58](https://www.youtube.com/watch?v=tCY7p6dVAGE&t=18178s) when this part was published
**Code:** git tag [`part008`](https://github.com/davidhruby/brrr/tree/part008). To get the code exactly as it was for this part: `git checkout part008`

New here? Start with the [Introduction](intro.md).

## What changed

### 1. Station names stored inside the key (`StrVec`), after Jon

With a cheap hash (Part 007), the next cost in the `HashMap` is the key itself. A `Vec<u8>`
key is 24 bytes in the table (pointer, capacity, length), and the name lives somewhere else on
the heap: one allocation per station, and every comparison of a key follows the pointer. Jon's
idea in the stream is the *small string optimization*: a name shorter than 16 bytes is stored
**right in the 16-byte key**, and only longer names go to the heap.

```rust
const INLINE: usize = 16;
const LAST: usize = INLINE - 1;

union StrVec {
    inlined: [u8; INLINE],
    // if length high bit is set, then inlined into pointer then len
    // otherwise, pointer is a pointer to Vec<u8>
    heap: (usize, *mut u8),
}

impl StrVec {
    pub fn new(s: &[u8]) -> Self {
        if s.len() < INLINE {
            let mut combined = [0u8; INLINE];
            combined[..s.len()].copy_from_slice(s);
            combined[LAST] = s.len() as u8 + 1;
            Self { inlined: combined }
        } else {
            let ptr = Box::into_raw(s.to_vec().into_boxed_slice());
            Self {
                heap: (ptr.len().to_be(), ptr as *mut u8),
            }
        }
    }
}

impl Drop for StrVec {
    fn drop(&mut self) {
        if unsafe { self.inlined[LAST] } == 0x00 {
            unsafe {
                let len = usize::from_be(self.heap.0);
                let ptr = self.heap.1;
                let slice_ptr = std::ptr::slice_from_raw_parts_mut(ptr, len);
                let _ = Box::from_raw(slice_ptr);
            }
        }
    }
}

impl AsRef<[u8]> for StrVec {
    fn as_ref(&self) -> &[u8] {
        unsafe {
            if self.inlined[LAST] != 0x00 {
                let len = self.inlined[LAST] as usize - 1;
                // &self.inlined[..len]
                std::slice::from_raw_parts(self.inlined.as_ptr(), len)
            } else {
                let len = usize::from_be(self.heap.0);
                let ptr = self.heap.1;
                std::slice::from_raw_parts(ptr, len)
            }
        }
    }
}

impl PartialEq for StrVec {
    fn eq(&self, other: &Self) -> bool {
        self.as_ref() == other.as_ref()
    }
}

impl Eq for StrVec {}

impl Hash for StrVec {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_ref().hash(state)
    }
}

impl Borrow<[u8]> for StrVec {
    fn borrow(&self) -> &[u8] {
        self.as_ref()
    }
}
```

```rust
let mut stats: HashMap<StrVec, MeasurementStats, IdealHasherBuilder> = HashMap::with_capacity_and_hasher(10_000, IdealHasherBuilder);
// …
let stat = match stats.get_mut(station) {
    Some(stat) => stat,
    None => stats.entry(StrVec::new(station)).or_insert(MeasurementStats::default())
};
```

How the 16 bytes are used (little-endian):

```text
byte:      0  1  2  3  4  5  6  7 │ 8  9 10 11 12 13 14 15
inlined:   the name (up to 15 bytes) ……………………………………… │ len+1     ← short name
heap:      length (big-endian)     │ pointer (LE)    … 00     ← long name
```

- **A short name** (< 16 bytes): its bytes, padded with zeros, and `len + 1` in the last byte,
  so the last byte is 1 – 16, never 0. `Oslo` is `O s l o 0 … 0 5`.
- **A long name**: the length and a pointer to a `Box<[u8]>` on the heap. On little-endian, the
  last byte of the union is the **top byte of the pointer**, which is 0 for addresses on the
  heap. So the last byte is the tag: 0 means heap, anything else means inline. The length is
  stored big-endian (`to_be`), so that it doesn't touch the pointer half. (The comment about
  the "length high bit" is left over from an earlier version of the idea.)
- `Box::into_raw` keeps the heap allocation alive after `new`; `Drop` turns it back into a
  `Box` with `Box::from_raw`, so it's freed. Without that, the pointer would point into freed
  memory, or the memory would leak.
- `PartialEq`, `Hash` and `Borrow<[u8]>` all go through `as_ref()`, which gives the name as a
  `&[u8]` either way. `Hash` of a `StrVec` is therefore the same as `Hash` of the `[u8]`, so the
  map can still be searched with a `&[u8]` (`get_mut(station)`).

Measurements (nightly, raw output in [`measurements/part008/`](../measurements/part008/)):

| | `david` 100M | `david` 1B |
|---|---|---|
| Wall time | 1.9 s | 14.1 s |
| CPU time (user + sys, both threads) | 1.5 s | 15.5 s |
| Peak memory footprint | 2.4 MB | 8.4 MB |
| Instructions per row | ~213 | ~222 |
| CPU cycles | 5.8 billion | 60.8 billion |

**1B: 13.6 s (Part 007) → 14.1 s (+3.5 %), slower.** The output is identical to Part 007 on
both files. The wall time of the same code moves by ~1 – 2 % between runs, but the number of
instructions hardly moves, and it went up by ~15 per row on both files (1B ~207 → ~222).

Why, as far as I can tell from the code:

- **Every comparison of a key now goes through `as_ref()`**: read the tag from byte 15, branch
  on short or long, build the slice (pointer and length), and only then `memcmp`. With
  `Vec<u8>`, the pointer and the length were simply there. A few instructions more on every row
  are 15 billion more on 1 billion rows.
- **A short key isn't compared as two `u64`s**, which is where the idea could save the most: it
  still goes through a slice and `memcmp`.
- **What it saves is tiny here:** 413 allocations in the whole run, and one hop along a pointer
  to names that are in the L1 cache anyway (413 names are a few KB). With many more stations
  (the rules allow 10,000) the names wouldn't fit in L1 any more, and it could look different.

**Jon saw many more dTLB misses** with this version. My guess, not measured yet: speculative
execution. When the branch predictor guesses "long" for a short key, the CPU runs the `else`
path ahead and takes bytes 8 – 15 of the name as a pointer (for `Kuwait City` that's `ity`,
zeros and the tag 12, so the "address" `0x0C000000_00797469`). Translating that address misses
the TLB and walks the page tables; the load is thrown away once the CPU sees that the guess was
wrong, so the program never crashes, but the page walk has already happened and the counter has
counted it. Only ~2 % of the rows have a long name (`Las Palmas de Gran Canaria`,
`Petropavlovsk-Kamchatsky`, `City of San Marino` and a few more of 16+ bytes), so the predictor
should mostly guess "short", and whether this explains "many more" is open. The measurement on
Linux (`perf stat -e dTLB-load-misses` and `perf record` of the dTLB misses, with and without
`StrVec`) is started: the version without `StrVec` is measured, the one with it not yet.

## What I learned

## Next steps

## TODO

Things I want to try later. I'm not doing them yet, because Jon may get to them in a later
part of the stream, and I don't want to get ahead of him.

- **Finish the dTLB measurement on Linux** (change #1): `perf stat` and `perf record` of the
  dTLB misses with `StrVec`, compared with the version without it, to see where the extra misses
  come from (the speculative load in `as_ref`, or something else).
- **Compare short keys as two `u64`s** instead of through `as_ref()` and `memcmp`, and decide
  short or long without a branch, then measure `StrVec` again.
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
