# TLB

*Notes mostly for myself. Apple doesn't publish the TLB sizes of the M4 and `sysctl` doesn't
show them, so I measured them. The measurements on this page were made during
[Part 004](../part004.md), on the machine described in [My machine](my-machine.md).*

## What the TLB is

A program never sees physical RAM, only virtual addresses. The kernel maps them to physical
memory in fixed blocks called **pages**; on Apple Silicon a page is **16 KB**
(`sysctl hw.pagesize` = 16384). The mapping from virtual pages to physical frames is stored in
the **page tables**, in RAM. One entry is 8 bytes: the number of the physical frame plus a few
flags. The number of the virtual page isn't stored, it's the position of the entry in the
table, and the offset inside the page (the lowest 14 bits of the address) stays the same.

Reading the page tables on every memory access would be far too slow, so the CPU keeps the
recent translations in the **TLB** (translation lookaside buffer), a small cache of page table
entries. There are two levels: a small, fast L1 TLB and a bigger L2 TLB. When a translation
isn't in either of them, the CPU walks the page tables in memory (a **page walk**).

The TLB holds translations, not data. One entry covers a whole page, so a TLB with N entries
covers N × 16 KB of memory. Whether the data itself is in the L1 or L2 cache is a separate
question: caches work with 128-byte cache lines, not with pages.

## How I measured it

The program is [`src/analyze/tlb_size/main.rs`](../../src/analyze/tlb_size/main.rs) (run with
`cargo run --release --bin analyze-tlb-size`). It measures how long one memory read takes
depending on how many different pages the reads go over:

1. **N pages, aligned.** A `Vec` of a struct with `#[repr(align(16384))]` and a
   `[usize; 2048]` array: each element is exactly one 16 KB page and starts on a page
   boundary.
2. **One slot per page.** Only one `usize` in each page is used. Its position is shifted by one
   128-byte cache line from page to page ((page mod 128) × 16 in `usize` units), so the slots
   spread over all the sets of the L1 cache instead of fighting for the same one.
3. **A random chain.** The pages are shuffled into a random order, and the slot of each page
   holds the number of the next page in that order; the last one points back to the first.
   That makes one cycle through all N pages, in an order the hardware prefetcher can't guess.
4. **Pointer chasing.** The measured loop only reads the slot of the current page and jumps to
   the page it points to, 100 million times. Each read needs the result of the previous one, so
   the CPU can't overlap them: the time per step is the latency of one read.
5. **N from 8 to 8,000 pages, in steps of 8**, with a warm-up before the measurements.

The time per step includes a few instructions of address computation and a bounds check. They
are the same for every N, so they shift the whole curve up a little but don't change where it
jumps. Cycles are converted from nanoseconds at ~4.4 GHz, the frequency of a P-core, so they
are approximate. Each N was measured once. Raw output in
[`measurements/tlb_stats/`](../../measurements/tlb_stats/).

## Results

| Pages (N) | ns per step | ≈ cycles | |
|---|---|---|---|
| 8 – 128 | 1.41 | ~6 | everything in the L1 TLB and the L1 cache |
| 136 | 1.63 | ~7 | the first L1 TLB misses |
| 160 | 2.14 | ~9 | |
| 192 – 1,024 | 4.0 – 4.6 | ~18 – 20 | L1 TLB misses, translations from the L2 TLB |
| 1,032 | 4.91 | ~22 | the slots don't fit in the L1 cache any more |
| 1,536 – 3,072 | 7.0 – 7.7 | ~31 – 34 | data from the L2 cache |
| 3,088 | 8.16 | ~36 | the first L2 TLB misses |
| 3,200 | 10.90 | ~48 | |
| 3,336 – 8,000 | 13.9 – 14.4 | ~61 – 63 | L2 TLB misses: a page walk on every step |

### L1 data TLB: misses start above 128 pages

Up to 128 pages, every step takes 1.41 ns. From 136 pages on, it gets slower, and from about
192 pages, almost every step misses. The slots of 192 pages take 24 KB, well inside the
128 KB L1 cache, so the jump comes from the TLB alone.

The L1 TLB holds **about 128 entries** without misses, which covers **~2 MB**. An L1 TLB miss
that hits the L2 TLB costs **~11 – 14 cycles**.

### L2 TLB: misses start above 3,072 pages

The latency starts to rise at 3,088 pages and is up to ~14 ns at about 3,336 pages. The slots
of 3,072 pages take 384 KB, well inside the 16 MB L2 cache, so again it's the TLB. The L2 TLB
holds **about 3,072 entries**, which covers **~48 MB**. A page walk costs **~28 cycles** on
top, with the page tables themselves in the cache.

### L1 data cache: exactly 128 KB

The step between 1,024 and 1,032 pages isn't the TLB but the L1 data cache: 1,024 slots ×
128 B = 128 KB, the size of the L1 data cache. Above that, the slots are read from the L2
cache, which costs ~11 – 14 cycles more. I haven't run the control measurement yet (the same
number of slots packed into fewer pages, so that the TLB doesn't change), but the size matches
exactly.

### The jumps are gradual

With a strict LRU replacement (always evict the entry used longest ago) and a cyclic access
pattern like this one, one page above the capacity would make every step a miss: the entry
needed next is always the one that was just evicted. Instead, the latency rises over ~60 pages
at the L1 TLB and over ~250 pages at the L2 TLB. So neither TLB uses strict LRU; more likely
some pseudo-random replacement, where part of the entries survive.

### An unexplained sawtooth with a period of 256 pages

Between 192 and 1,024 pages, the latency rises and falls with peaks exactly at multiples of
256 pages:

| Pages | 256 | 376 | 512 | 640 | 768 | 896 | 1,024 |
|---|---|---|---|---|---|---|---|
| ns per step | **4.60** | 4.02 | **4.60** | 4.24 | **4.60** | 4.34 | **4.60** |

It repeats in every run, so it's not noise. It probably has to do with how one of the TLBs is
split into sets and how the page numbers fall into them for different N, but I don't have an
explanation yet.

## Summary

| | Entries | Covers (16 KB pages) | Cost of a miss |
|---|---|---|---|
| L1 data TLB | ~128 | ~2 MB | ~11 – 14 cycles |
| L2 TLB | ~3,072 | ~48 MB | ~28 cycles (page walk) |
| L1 data cache | 1,024 cache lines | 128 KB | ~11 – 14 cycles (from L2) |

**For 1BRC:** the `HashMap` with 413 stations takes ~30 KB, a few pages: it fits in the L1
TLB and in the L1 cache. Even the worst case of the rules (10,000 stations, 100-byte names)
would be ~1.75 MB, ~110 pages, still inside the L1 TLB. Reading the 13.8 GB file walks through
842,009 pages, but sequentially: one TLB miss for every 16 KB of data.
