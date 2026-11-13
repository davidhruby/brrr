# brrr

Learning Rust by working through the [One Billion Row Challenge](https://github.com/gunnarmorling/1brc),
following Jon Gjengset's ([@jonhoo](https://github.com/jonhoo)) stream
[*"One Billion Row Challenge"*](https://www.youtube.com/watch?v=tCY7p6dVAGE) and his
[`jonhoo/brrr`](https://github.com/jonhoo/brrr) repository. On top of that I measure how the
code works with the hardware: CPU, caches, memory and the SSD.

The repo holds two binaries side by side: `david` (my own code) and `jonhoo` (Jon's code,
lightly adapted, used as the reference).

📓 **The learning notes live in [`docs/`](docs/README.md).** Start with the
[Introduction](docs/intro.md), then [Part 001 — The first naive version](docs/part001-first-naive-version.md):

- [Goal](docs/intro.md#goal): the challenge and what I want to learn from it
- [Setup](docs/notes/setup-generating-the-input-file.md): generating the 100M and 1B input files
- [My machine](docs/notes/my-machine.md): the laptop all numbers come from
- [Measurements](docs/part001-first-naive-version.md#measurements): `david` vs `jonhoo`

## The parts

Wall time of `david` on the 1B file at the end of each part. Parts 001 – 011 are one thread on
my MacBook (Apple M4, 16 GB RAM), with the file read from the SSD. From Part 012 on, the 1B file
doesn't fit in the RAM of the laptop next to everything else, so the numbers are from an AWS
`r8a.2xlarge` (AMD Zen 5, 8 cores, 64 GB RAM) with the file in `tmpfs`, the way the original
challenge measured.

| Part | Topic | What's inside | 1B |
|---|---|---|---|
| [001](docs/part001-first-naive-version.md) | The first naive version | `BufReader`, `String` per line, `BTreeMap`, `f64`; 13.8 GB of memory and swapping on 1B | 266 s (Jon's: 107 s) |
| [002](docs/part002-just-poslishments.md) | Next steps from Part 001 | No more `String` per line, `or_insert` vs `or_insert_with`, the swapping gone | 108 s |
| [003](docs/part003-hashmap-bytes-and-lto.md) | HashMap, bytes and LTO | `HashMap` instead of `BTreeMap`, `get_mut` first, `Vec<u8>` keys, `from_utf8_unchecked`, `lto = "fat"` | 49 s |
| [004](docs/part004-mmap.md) | `mmap` | The file mapped instead of read, `MADV_WILLNEED` / `MADV_DONTNEED` on macOS, a helper thread that touches the pages ahead | 28 s |
| [005](docs/part005-parsing-temperature.md) | Parsing the temperature | Integers instead of `f64`, a parser without branches | 24.7 s |
| [006](docs/part006-finding-the-end-of-a-line.md) | Finding the end of a line | `memchr`, SWAR (8 bytes at a time), a SIMD iterator that finds the `;` and `\n` of several lines at once | 18.5 s |
| [007](docs/part007-hashing.md) | Hashing | SipHash vs my own hashers, counting collisions, the capacity of the `HashMap` | 13.6 s |
| [008](docs/part008-short-strings.md) | Short strings inline | Jon's `StrVec` (names inside the key) and a `u128` key, both slower, not kept | 13.6 s |
| [009](docs/part009-masked-loads-and-dtlb-misses.md) | Masked loads and dTLB misses | Why Jon's code is 4.6× slower on a Graviton: `load_or_default` without a masked load on ARM, AVX-512 on x86, dTLB misses on Zen 5 | (no change; on AWS Graviton3, Intel SPR, AMD Zen 5) |
| [010](docs/part010-build-settings-and-simd-name-comparison.md) | Build settings and SIMD name comparison | `codegen-units = 1`, `panic = "abort"`, the cleanup at the exit, comparing station names with SIMD instead of `memcmp` | 9.93 s |
| [011](docs/part011-ideas-from-other-solutions.md) | Ideas from other solutions | SWAR temperature parser (not kept), the exit in a child process (not kept), branches for min/max | 9.88 s |
| [012](docs/part012-threads-and-4kb-pages.md) | Threads, and what 4 KB pages cost them | 8 threads on equal chunks, scaling 1 – 10 threads, page walks and the exit with 4 KB pages, huge pages for `tmpfs`, how a few `eprintln!` changed hashbrown's inlining by 20 % | 1.36 s (8 threads, AWS) |

Every part has a git tag with the code as it was at the end of that part, for example
`git checkout part007`.

## Quick start

The 1M-row input file is committed, so this works right after cloning:

```sh
cargo build --release
./target/release/david  input/1M_measurements.txt 8
./target/release/jonhoo input/1M_measurements.txt
```

The second argument of `david` is the number of threads.
