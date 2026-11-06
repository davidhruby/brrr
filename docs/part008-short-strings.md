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

**Jon saw many more dTLB misses** with this version. My first guess was speculative execution:
when the branch predictor guesses "long" for a short key, the CPU runs the `else` path of
`as_ref()` ahead and takes bytes 8 – 15 of the name as a pointer (for `Kuwait City` that's
`ity`, zeros and the tag 12, the "address" `0x0C000000_00797469`). Translating it would miss
the TLB and walk the page tables; the load is thrown away once the CPU sees the wrong guess, so
the program doesn't crash, but the walk has happened and may be counted.

**Measured on Linux** (the AWS Graviton3 instance from Part 005, 100M file, both versions
warmed up first, only my code counted with `:u`, two events per run):

```sh
sudo perf stat -o stat-dtlb-u.txt -e dTLB-load-misses:u,dTLB-loads:u ./target/release/david input/100M_measurements.txt > /dev/null
sudo perf stat -o stat-branches-u.txt -e branch-misses:u,branches:u ./target/release/david input/100M_measurements.txt > /dev/null
sudo perf stat -o stat-cycles-u.txt -e cycles:u,instructions:u ./target/release/david input/100M_measurements.txt > /dev/null
```

| 100M, per row | `Vec<u8>` (Part 007) | `StrVec` | Difference |
|---|---|---|---|
| dTLB misses | 0.759 | 0.730 | −4 % |
| Loads through the dTLB | 39.4 | 43.7 | +4.4 |
| Branches | 25.8 | 28.6 | +2.8 |
| Mispredicted branches | 0.577 | 0.590 | +0.014 |
| Instructions | 182.8 | 200.5 | **+17.7 (+10 %)** |
| Cycles | 50.9 | 57.4 | **+13 %** |
| Time | 1.97 s | 2.22 s | +13 % |

- **On the Graviton, `StrVec` doesn't add dTLB misses**, there are even 4 % fewer (the keys are
  in the table, the heap isn't touched). Jon's observation didn't repeat here.
- **The slowdown is real and the same on both machines:** ~18 instructions, ~4 loads and ~3
  branches more per row, which is `as_ref()`. On the Graviton it costs 13 %, on the M4 3.5 %.
- **The short/long branch almost never mispredicts** (+0.014 per row), so the speculative load
  through a "pointer" made of letters hardly ever happens. My guess doesn't explain anything
  here.
- **Where the ~0.75 dTLB misses per row come from**, with `perf record -e dTLB-load-misses:u`
  and `perf report --sort srcfile,srcline`: in both versions it's the first access to the
  bucket of the `HashMap` (`cmp.rs:21`, the comparison of the `Vec<u8>` key, 40 % of them;
  `main.rs:619`, reading the tag of the `StrVec`, 53 %). The table with 16,384 buckets of
  32 – 40 bytes is 512 – 640 KB, ~130 – 160 pages of 4 KB on Linux, and the 413 stations are
  scattered over it, so most lookups touch a page the first level of the dTLB doesn't hold. On
  the M4 with 16 KB pages, it's only ~40 pages.

Why Jon saw something else I can't tell: another CPU (x86, with other TLB sizes, another
branch predictor, and `dTLB-load-misses` mapped to another hardware event, possibly counting
speculative walks), and other code around it at that moment.

### 2. Comparing short keys as one `u128` (even slower, not used)

The idea of `StrVec` is to compare a short key without following a pointer and without
`memcmp`. But the version from change #1 never does that: the program looks up by `&[u8]`
(`stats.get_mut(station)`), so hashbrown compares `station == key.borrow()`, which goes through
`as_ref()` and `memcmp`. `StrVec`'s own `PartialEq` is never called in a lookup. To use it, the
lookup has to be by a `StrVec` too:

```rust
impl PartialEq for StrVec {
    fn eq(&self, other: &Self) -> bool {
        unsafe {
            if self.inlined[LAST] != 0x00 || other.inlined[LAST] != 0x00 {
                // at least one is short: all 16 bytes decide, a short and a long key never match
                u128::from_ne_bytes(self.inlined) == u128::from_ne_bytes(other.inlined)
            } else {
                // both long: compare the names on the heap
                self.as_ref() == other.as_ref()
            }
        }
    }
}
```

```rust
let stat = if station.len() < INLINE {
    // short name: look up by StrVec, so that StrVec's PartialEq (two u64 / one u128) is used
    let key = StrVec::new(station);
    match stats.get_mut(&key) {
        Some(stat) => stat,
        None => stats.entry(key).or_insert(MeasurementStats::default()),
    }
} else {
    // long name: look up by &[u8] as before, StrVec::new would allocate on every row
    match stats.get_mut(station) {
        Some(stat) => stat,
        None => stats.entry(StrVec::new(station)).or_insert(MeasurementStats::default()),
    }
};
```

- **Two short keys** are equal exactly when their 16 bytes are equal: the zeros after the name
  and `len + 1` in the last byte make sure of that. On the M4, `u128 == u128` is two `u64`
  comparisons, no call.
- **A short and a long key** never match, and the `u128` comparison sees it by itself: the last
  byte is the tag (≠ 0) in one and 0 in the other. So `||` is enough, no extra branch.
- **Two long keys** still go through `as_ref()`, the `u128` would only compare their lengths and
  pointers.
- `get_mut(&key)` works with a `&StrVec` because every type can be borrowed as itself
  (`Borrow<T> for T`); `Hash` stays on `as_ref()`, so a `StrVec` and a `&[u8]` with the same
  name still get the same hash, which the long names looked up by `&[u8]` need.

Measurements (nightly, raw output in
[`measurements/part008-u128/`](../measurements/part008-u128/)):

| 1B | `Vec<u8>` (Part 007) | `StrVec`, through `as_ref` (change #1) | `StrVec`, `u128` (this change) |
|---|---|---|---|
| Wall time | 13.64 s | 14.12 s | **14.81 s** |
| Instructions per row | ~207 | ~222 | **~240** |
| Cycles | 58.4 billion | 60.8 billion | 63.6 billion |

On 100M the same: 1.88 → 1.92 → 1.99 s, ~199 → ~213 → ~231 instructions per row. The output is
identical to Part 007.

**Even slower.** The comparison got cheaper, but every row now does more:

- **`StrVec::new(station)` on every row**: zero 16 bytes, copy the name into them
  (`copy_from_slice` of a variable length) and write the tag. Before, the lookup used the
  `&[u8]` without any copy.
- **Hashing the lookup key goes through `as_ref()`**: a branch and building a slice before the
  hashing starts.
- **The `station.len() < INLINE` branch** in the loop, and **`Drop` of the lookup key** (one
  more check of the tag) at the end of every row.
- **What it saves** is one comparison of a short name per row, on names that are in the L1
  cache anyway.

The copy could go away by loading 16 bytes straight from the mapped file and masking out the
bytes after the name, and the hash could be computed from those 16 bytes. But that's an
`unsafe` read past the end of the name's slice, and hashing short keys differently from
`[u8]` breaks the rule that a key and what it borrows as must hash the same. More work and more
risk, to save at best one cheap comparison per row.

**So the program goes back to the `Vec<u8>` key of Part 007.** Both versions of `StrVec` stay
here as measured attempts: an optimization that makes sense in general (no allocation, no
pointer to follow, the key right in the table) doesn't pay off on these data, with 413 short
names that all fit into the L1 cache, and only measuring shows it.

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
