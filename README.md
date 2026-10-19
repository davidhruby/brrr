# brrr

Learning Rust by working through the [One Billion Row Challenge](https://github.com/gunnarmorling/1brc),
following Jon Gjengset's ([@jonhoo](https://github.com/jonhoo)) stream
[*"One Billion Row Challenge"*](https://www.youtube.com/watch?v=tCY7p6dVAGE) and his
[`jonhoo/brrr`](https://github.com/jonhoo/brrr) repository. On top of that I measure how the
code works with the hardware: CPU, caches, memory and the SSD.

The repo holds two binaries side by side: `david` (my own code) and `jonhoo` (Jon's code,
lightly adapted, used as the reference).

📓 **The learning notes live in [`docs/`](docs/README.md).** Start with
[Part 001 — The first naive version](docs/part001-first-naive-version.md):

- [Goal](docs/part001-first-naive-version.md#goal): the challenge and what I want to learn from it
- [Setup](docs/part001-first-naive-version.md#setup-generating-the-input-file): generating the 100M and 1B input files
- [My machine](docs/part001-first-naive-version.md#my-machine): the laptop all numbers come from
- [Measurements](docs/part001-first-naive-version.md#measurements): `david` vs `jonhoo`

## Quick start

The 1M-row input file is committed, so this works right after cloning:

```sh
cargo build --release
./target/release/david  input/1M_measurements.txt
./target/release/jonhoo input/1M_measurements.txt
```
