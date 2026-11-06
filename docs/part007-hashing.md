# Part 007 — Hashing

**Dates:** 2026-11-05 – 2026-11-05
**Video progress:** stopped at [3:23:49](https://www.youtube.com/watch?v=tCY7p6dVAGE&t=12229s) when this part was published
**Code:** git tag [`part007`](https://github.com/davidhruby/brrr/tree/part007). To get the code exactly as it was for this part: `git checkout part007`

New here? Start with the [Introduction](intro.md).

## What changed

### 1. A benchmark: SipHash vs a "stupidly fast" hash

The `HashMap` with the default hasher (SipHash) is now the biggest cost: over half of the
main thread's time in the profile, and ~62 % of the branch mispredictions came from hashing
the station names (see [Part 005, change #3](part005-parsing-temperature.md#3-parsing-without-branches)).
SipHash is built to resist attacks with crafted keys, not for speed. So the first try is the
opposite extreme, a hasher that just adds up the bytes of the name:

```rust
pub struct FastHasherBuilder;
pub struct FastHasher(u32);

impl BuildHasher for FastHasherBuilder {
    type Hasher = FastHasher;

    fn build_hasher(&self) -> Self::Hasher {
        FastHasher(0)
    }
}

impl Hasher for FastHasher {
    fn finish(&self) -> u64 {
        u64::from(self.0)
    }
    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 = self.0.wrapping_add(u32::from(*byte));
        }
    }
}
```

`BuildHasher` is the factory the `HashMap` keeps and asks for a new hasher for every key;
`Hasher` does the hashing itself. My first version implemented `BuildHasher` for `FastHasher`
instead of `FastHasherBuilder`, and the compiler said `FastHasherBuilder: BuildHasher` is not
satisfied on every `get_mut` and `entry`.

A benchmark in [`benches/hashmap_criterion.rs`](../benches/hashmap_criterion.rs): the lines
of the 1M file are split and their temperatures parsed in advance, and every iteration
inserts all ~1 million of them into a new `HashMap::with_capacity(10_000)`, the same way as
the program (`get_mut`, and `entry` only for a new station):

| 1M file, ~1 million rows (nightly) | Time | Per row |
|---|---|---|
| `HashMap` with SipHash (the default) | 12.15 ms | **12.2 ns** |
| `HashMap` with the sum of the bytes | 25.40 ms | **25.4 ns** |

**The "fast" hash is twice as slow.** Computing it is cheaper, but it's a bad hash, and the
`HashMap` pays for that. My guesses why, not measured yet:

- **Many stations get the same hash.** The sum of the bytes doesn't depend on their order
  (`"ab"` and `"ba"` are the same), and the sums of names of a similar length all fall into a
  narrow range of a few thousand. Stations with the same hash end up in the same place, and
  the `HashMap` has to compare the names one by one until it finds the right one.
- **The top bits of the hash are always 0.** `finish` returns a `u32` widened to `u64`. The
  `HashMap` of the standard library (hashbrown) uses the top 7 bits of the hash as a tag, so
  that one SIMD comparison of a group of slots throws away almost all the wrong ones without
  comparing the keys. With the top bits always 0, every station has the same tag, every slot
  in the group looks like a candidate, and each of them needs a full comparison of the name.
- **It still goes through the name byte by byte**, in a loop as long as the name, like
  SipHash's handling of the rest of the name.

So a fast hash has to be fast **and** spread the keys well, including the top bits.

### 2. A hash with a folded multiply: 8 bytes at a time

The second try, after the hash Jon writes in the stream: take the name 8 bytes at a time,
multiply each block into the state, and fold the 128-bit product back into 64 bits. In the
stream, Jon has the loop over the `remainder` commented out (and `#[inline(never)]` on
`write`, for the profiler); I kept the loop:

```rust
pub struct FastHasherBuilder;
pub struct FastHasher(u64);

impl BuildHasher for FastHasherBuilder {
    type Hasher = FastHasher;

    fn build_hasher(&self) -> Self::Hasher {
        FastHasher(0xcbf29ce484222325)
    }
}

impl Hasher for FastHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        let (chunks,remainder) = bytes.as_chunks::<8>();
        for &chunk in chunks {
            let mixed = self.0 as u128 * (u64::from_ne_bytes(chunk) as u128);
            self.0 = (mixed >> 64) as u64 ^ mixed as u64;
        }
        for b in remainder {
            self.0 = self.0.wrapping_add(u64::from(*b));
        }
    }
}
```

**How a hasher is used.** For every lookup (`get_mut`, `entry`), the `HashMap` asks its
`FastHasherBuilder` for a new `FastHasher`, which starts from the same seed
(`0xcbf29ce484222325`, the offset basis of the FNV-1a hash; it just mustn't be 0). The key
is fed into it with `write`, `finish` reads the result, and the hasher is thrown away. The
same key therefore always gives the same hash. `Hash` for `[u8]` (and for `Vec<u8>`, which
gives the same result, so an inserted `Vec<u8>` key is found with a `&[u8]`) calls `write`
twice: first with the length of the key as 8 bytes, then with the bytes of the name.

**What the pieces do:**

- `as_chunks::<8>()` splits the bytes into whole blocks of 8 and a `remainder` of fewer than 8
  (`Las Palmas de Gran Canaria`: 3 blocks and `ia`).
- `u64::from_ne_bytes(chunk)` makes one number of a block (little-endian on the M4).
- **The multiplication into 128 bits mixes.** Every bit of one factor affects many bits of
  the product, through the carries. But the low bits of the product depend only on the low
  bits of the factors, while the high bits depend on all of them, so the high half must not be
  thrown away: 64 × 64 bits gives up to 128 bits.
- **`(mixed >> 64) as u64 ^ mixed as u64` folds the two halves** into 64 bits with XOR, so the
  result depends on both and no information is lost (*folded multiply*, the trick of fast
  hashes like wyhash and foldhash). `>> 64` moves the high half down, and `as u64` keeps the
  low 64 bits.

**When I copied it, I typed `&` instead of `^`**, and it was 3× slower than SipHash. On a model
with 4-bit numbers (a "byte" of 2 bits, a block of 2 "bytes", written with the first byte on
the left, as at school):

```text
bytes = [01, 10, 01, 01, 01, 10]   →   blocks 0110 and 0101, remainder [01, 10],  state 1011

block 1:  1011 × 0110 = 0100_0010     &: 0100 & 0010 = 0000      ^: 0100 ^ 0010 = 0110
block 2:  &: 0000 × 0101 = 0000_0000 → 0000                       ^: 0110 × 0101 = 0001_1110 → 1111
rest:     &: 0000 + 1 + 2 = 0011                                  ^: 1111 + 1 + 2 → 0010

bytes = [01, 01, 01, 01, 01, 10]   (one byte different)
          &: … → 0011  (the same hash)                            ^: … → 1000
```

AND keeps a 1 only where both halves have one, so the state empties towards 0, and once it's
0, `0 × anything = 0`: all longer keys end up with the hash of their remainder. XOR loses
nothing.

The same benchmark as in change #1:

| 1M file, per row (nightly) | Per row |
|---|---|
| SipHash (the default) | 12.0 ns |
| Folded multiply with `&` | 36.2 ns |
| Folded multiply with `^` | 19.0 ns |

**With `^` it's half the time of `&`, but still slower than SipHash.** My guesses, not
measured yet:

- **Short names are only added up.** A name shorter than 8 bytes (`Abha`, `Oslo`, `Accra`)
  has no whole block, so all of it goes into the `remainder` and is just added to the state.
  For all names of the same length, the hash is the same value (from the length block) plus
  the sum of the bytes: collisions when the sums are equal, and the same top bits for all of
  them, so hashbrown's tag can't tell them apart.
- **The state is multiplied by the data, not by a constant.** A block with many zero bytes
  (like the length, `[4, 0, 0, 0, 0, 0, 0, 0]`) gives a small product that mixes few bits.
- **`finish` doesn't mix.** Whatever the last step didn't mix stays unmixed.

What to try next: put the remainder into one zero-padded `u64` and mix it the same way as a
whole block; mix the block into the state with XOR and multiply by a fixed odd constant
(e.g. `0x9E3779B97F4A7C15`) instead of by the data; and one more folded multiply in `finish`.
And before measuring the time, check the hash itself: for all 413 station names, how many
different hashes (should be 413) and how many different top 7 bits (`hash >> 57`, ideally
close to 128).

### 3. Counting the collisions, and the capacity of the `HashMap`

Before trying the next hash, I wanted to see what's really wrong with the one from change #2,
instead of only guessing. How a `HashMap` (hashbrown) finds a key:

1. it computes the hash,
2. the **low bits** (`hash & (buckets - 1)`, the number of buckets is a power of two) pick
   where to start, and the **top 7 bits** (`hash >> 57`) are the key's *tag*,
3. it loads the control bytes of a **group of 16 buckets** and finds the buckets with the same
   tag with one SIMD comparison,
4. only for those, it compares the whole key (`memcmp`).

So there are three kinds of "collision", and they cost very differently:

| | When | Cost |
|---|---|---|
| **The same hash** | two names have the same 64-bit hash | a name comparison on every lookup, whatever the size of the table |
| **The same bucket and the same tag** | different hashes, but the same low bits and the same top 7 bits | also a name comparison, the SIMD step can't tell them apart |
| **The same bucket, a different tag** | different hashes, the same low bits | almost nothing: the second key just sits in another bucket of the same group, and the SIMD step skips the first one |

**A quick test** on the 1M file (all 413 stations are in it, so the 1B file gives the same):
the number of different names minus the number of different hashes. The hash of change #2
gave **413 − 344 = 69** names that share their hash with another one, in groups of up to 5.
SipHash gives 0.

**A small tool,** [`src/analyze/collision/main.rs`](../src/analyze/collision/main.rs)
(`cargo run --release --bin analyze-collision`), takes the 413 names and counts, for tables
of different sizes, how many buckets got more than one name, and how many names share the
bucket **and** the tag. It uses `hash_one` of the hash builder, so that it hashes exactly like
the `HashMap` (`Hash` for `[u8]` writes the length first and then the bytes; hashing a `&str`
or calling `write` directly gives a different hash). With SipHash, for the two sizes my
program can have:

| Buckets | Buckets with 2+ names | Names with the same bucket and tag |
|---|---|---|
| 512 (`HashMap::new()`, 413 stations) | 98 | 2 |
| 16,384 (`HashMap::with_capacity(10_000)`) | 6 | 0 |

That's what a good hash gives: about n² / 2m pairs in the same bucket for n names in m
buckets (413² / 32,768 ≈ 5 for 16,384), and about 1 in 128 of them with the same tag too.
(`RandomState` draws a new key every run, so the numbers move a little between runs.)

**The capacity.** `with_capacity(10_000)` makes **16,384 buckets**: hashbrown keeps a table at
most 7/8 full (10,000 × 8/7 ≈ 11,429) and rounds up to a power of two. `HashMap::new()`
starts empty and doubles as stations come: 4, 8, … 512 buckets for 413 stations, 7 times,
all within the first few thousand rows. A benchmark of only the `HashMap` with SipHash
(the 100M file split and parsed in advance, 100 million rows per iteration):

| 100M rows, SipHash | Per row |
|---|---|
| `HashMap::new()` (512 buckets) | 15.72 ns |
| `HashMap::with_capacity(10_000)` (16,384 buckets) | 14.60 ns (−7 %) |

I expected the small table to win, because it fits into the L1 cache (~20 KB vs ~670 KB). It
didn't. My explanation: 413 stations in 512 buckets is 80 % full, so a group of 16 buckets
holds ~13 other stations, and with 128 tags, ~13 / 128 ≈ 10 % of the lookups find a wrong
candidate with the same tag and compare a name for nothing. The analysis by the exact bucket
(2 names) misses those, because hashbrown looks at the whole group of 16, not only at one
bucket. In the big table, the groups are almost empty.

**A better `FastHasher`:** instead of adding up the bytes after the last whole block, put them
into one more block, padded with ones (a block of zeros would multiply the state by 0), and
mix it like the others:

```rust
let (chunks,remainder) = bytes.as_chunks::<8>();
let mut last = [1u8; 8];
(last[..remainder.len()].copy_from_slice(remainder));
for &chunk in chunks.iter().chain(iter::once(&last)) {
    let mixed = self.0 as u128 * (u64::from_ne_bytes(chunk) as u128);
    self.0 = (mixed >> 64) as u64 ^ mixed as u64;
}
```

Short names are now really mixed, and there's no branch on the length of the rest any more.
The tool found **one** pair of names with the same bucket and tag that stays at any size of
the table, up to 4 million buckets: a pair with the same whole hash. That's ~0.5 % of the
rows with one name comparison more, a few hundredths of a nanosecond per row on average.

### 4. The collisions and the benchmarks of each hash, side by side

All the numbers from the tool in [`src/analyze/collision/main.rs`](../src/analyze/collision/main.rs)
for the 413 station names ([`input/1B_unique_station_names.txt`](../input/1B_unique_station_names.txt)),
by the number of buckets of the table. *Buckets with 2+ names* counts the buckets that more
than one name lands in; *the same bucket and tag* counts the groups of names that hashbrown
can't tell apart without comparing the names; *tags shared* counts the tags used by more than
one name (with 413 names and only 128 tags, they always repeat, so this number alone means
little).

SipHash with a fixed key (`BuildHasherDefault::<DefaultHasher>::default()`, so that the
numbers don't change between runs), tags shared: 107:

| Buckets | Buckets with 2+ names | The same bucket and tag |
|---|---|---|
| 64 | 64 | 9 |
| 128 | 102 | 3 |
| 256 | 124 | 2 |
| **512** (`HashMap::new()`) | **98** | **2** |
| 1,024 | 67 | 1 |
| 2,048 | 41 | 0 |
| 4,096 | 20 | 0 |
| 8,192 | 9 | 0 |
| **16,384** (`with_capacity(10_000)`) | **6** | **0** |
| 32,768 | 3 | 0 |
| 65,536 | 2 | 0 |
| 131,072 | 1 | 0 |
| 262,144 – 4,194,304 | 0 | 0 |

`FastHasher` with the rest of the name as a padded block (change #3), tags shared: 101:

| Buckets | Buckets with 2+ names | The same bucket and tag |
|---|---|---|
| 64 | 64 | 13 |
| 128 | 114 | 7 |
| 256 | 121 | 3 |
| **512** (`HashMap::new()`) | **98** | **2** |
| 1,024 | 56 | 1 |
| 2,048 | 36 | 1 |
| 4,096 | 23 | 1 |
| 8,192 | 14 | 1 |
| **16,384** (`with_capacity(10_000)`) | **8** | **1** |
| 32,768 | 4 | 1 |
| 65,536 | 4 | 1 |
| 131,072 – 524,288 | 2 | 1 |
| 1,048,576 – 4,194,304 | 1 | 1 |

- **The buckets with 2+ names go down the same way for both**, roughly n² / 2m. So both
  spread the low bits about as well as random numbers.
- **The same bucket and tag goes to 0 for SipHash** from 2,048 buckets on. **For
  `FastHasher` it stays at 1 at any size**, even with 4 million buckets: two of the names have
  the same whole hash. A collision of two random 64-bit hashes among 413 names is extremely
  unlikely (~1 in 10¹⁴), so it's more likely a weakness of the hash than bad luck.

The benchmarks of the `HashMap` alone (the lines split and parsed in advance, so only the
`HashMap` is measured), in [`benches/hashmap_criterion.rs`](../benches/hashmap_criterion.rs).
Every row is from one run; between runs the same code moved by up to ~9 %, so only the
numbers of one run can be compared with each other.

| Per row | 100M rows | 1M rows, run 1 | 1M rows, run 2 |
|---|---|---|---|
| SipHash, `HashMap::new()` | 15.72 ns | 17.33 ns | 16.32 ns |
| SipHash, `with_capacity(10_000)` | 14.60 ns | 15.75 ns | 14.71 ns |
| `FastHasher`, `with_capacity(10_000)` | – | 11.57 ns | 11.65 ns |
| `IdealHasher`, `HashMap::new()` (change #5) | – | – | 11.71 ns |
| `IdealHasher`, `with_capacity(10_000)` (change #5) | – | – | 10.74 ns |

- **The capacity helps in every run**, by 7 – 9 %, for the reasons in change #3.
- **`FastHasher` is ~27 % faster than SipHash** with the same capacity, even with its one
  collision: it costs one name comparison on ~0.5 % of the rows, while the cheaper hashing
  saves ~4 ns on every row.
- The 100M benchmark is a bit slower per row than the 1M one: the prepared rows of the 100M
  file are ~2.4 GB and come from the RAM, not from the cache.

### 5. A hasher for short keys (`IdealHasher`)

The names are mostly 4 – 16 bytes long (the longest are `Las Palmas de Gran Canaria`, 26, and
`Petropavlovsk-Kamchatsky`, 24). A hash just for this list (a *perfect hash*) would be
cheating: the rules allow any names, up to 10,000 of them and 100 bytes long. So the goal is
a hash that is fast on names like these and spreads any keys well. The idea of fast modern
hashes (foldhash, rapidhash):

- **up to 16 bytes without a loop**: the first 8 and the last 8 bytes (for shorter names they
  overlap, `Copenhagen` gives bytes 0 – 7 and 2 – 9), then one folded multiply of both. For
  4 – 7 bytes the same with 4 bytes, for 1 – 3 the first, middle and last byte,
- **a loop only for the middle of names longer than 16 bytes**, which are rare, so the branch
  is almost always predicted right,
- **the length mixed in with one XOR** (`write_usize`, which `Hash` for `[u8]` calls first),
  not as a whole block,
- **multiplying by a constant**, an odd number from the golden ratio, not by the data,
- **one more folded multiply in `finish`**, so that the top bits, hashbrown's tag, depend on
  the whole key.

```rust
pub struct IdealHasherBuilder;
pub struct IdealHasher(u64);

impl BuildHasher for IdealHasherBuilder {
    type Hasher = IdealHasher;

    fn build_hasher(&self) -> Self::Hasher {
        IdealHasher(0x243F_6A88_85A3_08D3)
    }
}

impl IdealHasher {
    const K: u64 = 0x9E37_79B9_7F4A_7C15;

    fn folded_multiply(a: u64, b: u64) -> u64 {
        let m = a as u128 * b as u128;
        (m >> 64) as u64 ^ m as u64
    }

    fn read_u64(bytes: &[u8]) -> u64 {
        u64::from_le_bytes(bytes[..8].try_into().unwrap())
    }
}

impl Hasher for IdealHasher {
    fn finish(&self) -> u64 {
        // final mix, so that the top bits (hashbrown's tag) depend on everything
        Self::folded_multiply(self.0, Self::K)
    }

    // Hash for [u8] writes the length first; mix it in cheaply instead of as a whole block
    fn write_usize(&mut self, len: usize) {
        self.0 ^= len as u64;
    }

    fn write(&mut self, bytes: &[u8]) {
        let len = bytes.len();
        let (a, b) = if len >= 8 {
            // first 8 and last 8 bytes, they overlap for names of 8 – 15 bytes
            let mut a = Self::read_u64(bytes);
            let b = Self::read_u64(&bytes[len - 8..]);
            // the middle of names longer than 16 bytes (rare)
            let mut i = 8;
            while i + 8 < len - 8 {
                a = Self::folded_multiply(a ^ Self::read_u64(&bytes[i..]), Self::K);
                i += 8;
            }
            (a, b)
        } else if len >= 4 {
            // first 4 and last 4 bytes, they overlap for names of 4 – 7 bytes
            let a = u32::from_le_bytes(bytes[..4].try_into().unwrap()) as u64;
            let b = u32::from_le_bytes(bytes[len - 4..].try_into().unwrap()) as u64;
            (a, b)
        } else if len > 0 {
            // 1 – 3 bytes: first, middle and last byte
            let a = bytes[0] as u64 | (bytes[len / 2] as u64) << 8 | (bytes[len - 1] as u64) << 16;
            (a, 0)
        } else {
            (0, 0)
        };
        self.0 = Self::folded_multiply(a ^ self.0, b ^ Self::K);
    }
}
```

A `HashMap` with another hasher has it in its type, so a function that takes the map needs
it too. One generic function works for all of them, and the compiler makes a separate copy
for each hasher:

```rust
pub fn hashmap_upsert<S: BuildHasher>(stats: &mut HashMap<Vec<u8>, MeasurementStats, S>, station: &[u8], temperature: i16) {
```

The benchmark of the `HashMap` alone (1M file, split and parsed in advance), all in one run:

| 1M file, per row | `HashMap::new()` | `with_capacity(10_000)` |
|---|---|---|
| SipHash | 16.3 ns | 14.7 ns |
| `FastHasher` (change #3) | – | 11.6 ns |
| **`IdealHasher`** | 11.7 ns | **10.7 ns** |

−27 % against SipHash, −8 % against `FastHasher`: one multiplication per name instead of one
per 8-byte block, plus one for the rest and one for the length. And the capacity helps again
(−8 %), for the same reason as with SipHash.

Measurements with `IdealHasher` and `with_capacity(10_000)` in the program (nightly, raw
output in [`measurements/part007/`](../measurements/part007/)):

| | `david` 100M | `david` 1B |
|---|---|---|
| Wall time | 1.9 s | 13.6 s |
| CPU time (user + sys, both threads) | 1.4 s | 14.9 s |
| Peak memory footprint | 2.6 MB | 8.6 MB |
| Instructions per row | ~199 | ~207 |
| CPU cycles | 5.6 billion | 58.4 billion |

**1B: 18.5 s (Part 006) → 13.6 s (−26 %).** The output is identical to Part 006 on both files.

- **124 instructions fewer per row** (~331 → ~207, −37 %): the hashing was the biggest part
  of the program.
- **The benchmark predicted the program well this time.** SipHash with `new()` → `IdealHasher`
  with the capacity is ~5.6 ns per row in the benchmark, ~5.6 s on 1 billion rows; the program
  saved 4.8 s of wall time and 4.9 s of CPU time. The hashing is a big part of the program, so
  Amdahl's law doesn't take much away.
- **On 100M the program now waits more:** 1.9 s of wall time, but only 1.4 s of CPU time. The
  faster the computing, the more the waiting shows.
- From Part 001 (266 s) it's now ~19× faster.

## What I learned

## Next steps

## TODO

Things I want to try later. I'm not doing them yet, because Jon may get to them in a later
part of the stream, and I don't want to get ahead of him.

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
