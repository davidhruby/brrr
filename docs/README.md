# Learning journal

A part-by-part log of learning Rust by following Jon Gjengset's
[One Billion Row Challenge stream](https://www.youtube.com/watch?v=tCY7p6dVAGE).

Each part covers one step of the video and can span several days. It records:

- what I built,
- my own code next to Jon's (reference) code,
- measurements (time, memory),
- what I learned.

## Entries

| Part | Dates                     | Topic                                                     | Code tag  |
|------|---------------------------|-----------------------------------------------------------|-----------|
| –    |                           | [Introduction](intro.md): what this is about, the challenge, setup, my machine, approach | |
| 001  | 2026-10-15 – 2026-10-20   | [The first naive version](part001-first-naive-version.md) | [`part001`](https://github.com/davidhruby/brrr/tree/part001) |
| 002  | 2026-10-21 – 2026-10-25   | [Next steps from Part 001](part002-just-poslishments.md) | [`part002`](https://github.com/davidhruby/brrr/tree/part002) |
| 003  | 2026-10-25 – 2026-10-25   | [HashMap, bytes and LTO](part003-hashmap-bytes-and-lto.md) | [`part003`](https://github.com/davidhruby/brrr/tree/part003) |
| 004  | 2026-10-26 – 2026-10-28   | [In progress](part004.md) | [`part004`](https://github.com/davidhruby/brrr/tree/part004) |

Every part has a git tag with the code as it was at the end of that part. To see it:

```sh
git checkout part001
```

To go back to the latest code: `git checkout master`.

## Other pages

Notes mostly for myself, not needed to follow the parts:

- [Setup: generating the input file](notes/setup-generating-the-input-file.md): generating the
  100M and 1B input files.
- [My machine](notes/my-machine.md): the laptop all numbers come from, its CPU, caches, SSD and
  RAM.
- [Rust](notes/rust.md): general notes about Rust, starting with its history.
- [TLB](notes/tlb.md): measuring the TLB sizes of the M4, which Apple doesn't publish.

## Adding a new entry

1. Copy [`TEMPLATE.md`](TEMPLATE.md) to `partNNN-short-topic.md` (e.g. `part002-...md`).
2. Fill it in.
3. Add a row to the table above.
4. Commit, then tag the commit: `git tag -a partNNN -m "Part NNN — Title"` and push the tag:
   `git push origin partNNN`.
