# The One Billion Row Challenge in Rust — Introduction

*What this series is about, why I do it, and how. The parts themselves start with
[Part 001 — The first naive version](part001-first-naive-version.md).*

## Thank you, Jon

Before anything else, a big thank you to [Jon Gjengset](https://github.com/jonhoo) for making
[this video](https://youtu.be/tCY7p6dVAGE). I'm still near the beginning of it, and it already gives me something no
tutorial does: learning Rust in practice, watching how Jon writes the code and how he thinks
about it as he goes. And I love the positive energy he puts into it: you can see he really
enjoys it.

Nothing in these notes is meant as judging Jon's code against mine. I compare the two only
to learn Rust, and to help others who want to learn it in a similar way.

## A note for comments

I'm happy about every comment, correction or tip: it helps me understand things better, and
everyone else who reads along too. I only ask you to keep it kind. I'm learning, so I'll get
things wrong, and I learn best with positive motivation.

## Goal

### The challenge

The [One Billion Row Challenge](https://github.com/gunnarmorling/1brc) (1BRC): read a text
file with one billion temperature measurements and compute the min, mean and max for every
weather station, as fast as possible.

The input has one measurement per line:

```text
Oklahoma City;-1.0
Charlotte;18.1
Philadelphia;11.3
```

The output is one line per station, sorted alphabetically, as `station=min/mean/max`
rounded to one decimal place:

```text
Abha=-27.7/18.0/61.0
Abidjan=-18.7/26.0/71.0
Abéché=-14.9/29.4/76.3
```

**How the official challenge was measured.** The
[1BRC README](https://github.com/gunnarmorling/1brc#evaluating-results) says:

> *"Results are determined by running the program on a Hetzner AX161 dedicated server
> (32 core AMD EPYC™ 7502P (Zen2), 128 GB RAM)."*
>
> *"Programs are run from a RAM disk (i.o. the IO overhead for loading the file from disk is
> not relevant), using 8 cores of the machine."*

Each entry ran 5 times with `hyperfine`. The fastest and slowest runs were dropped, and the
result is the mean of the other three. The best entries finished in ~1.5 s on 8 cores
(1.535 s) and ~0.3 s on all 32 cores / 64 threads (0.323 s). So the official times measure
**CPU and RAM only**, never the disk.

### My goal

I'm learning Rust, so the challenge is a vehicle, not the target. I follow
[Jon's stream](https://youtu.be/tCY7p6dVAGE), write
my own version next to his, and on top of that I want to **see how the code works with the
hardware**: CPU cores and caches, memory, swap and the SSD. That's why each part measures more
than just the time, and why [My machine](notes/my-machine.md) describes the laptop in detail.

The first part, [Part 001](part001-first-naive-version.md), writes the simplest program that
produces correct output. Speed doesn't matter yet. That version is the **baseline** that
every later optimization gets measured against.

That's also why Part 001 spends more time on the measuring setup than on the code. I'm
building and trying out the tools now (the measuring script, the RAM disk, Instruments
traces and a small tool to analyze them), so that in the later parts I can just run them
and focus on the optimizations.

I work with three input files of growing size:

| File                          | Rows          | Size      | Used for                                       |
|-------------------------------|---------------|-----------|------------------------------------------------|
| `input/1M_measurements.txt`   | 1,000,000     | ~13.8 MB  | Day-to-day iteration: runs in under a second    |
| `input/100M_measurements.txt` | 100,000,000   | ~1.38 GB  | A realistic check that still runs in seconds    |
| `input/1B_measurements.txt`   | 1,000,000,000 | ~13.8 GB  | The real challenge, run at the end of each part |

All three have the same 413 stations. My laptop is not very powerful, so I iterate on the
small files and run the 1B file only once a version is finished.

With the 1B file I can't copy the official setup. The file is 13.8 GB and my laptop has
16 GB of RAM, so it never fits in RAM next to everything else. It always has to be read from
the SSD, and that alone takes ~6 s (see [Storage](notes/my-machine.md#storage-how-fast-can-the-file-be-read)).
That's why I measure two things:

- **1M and 100M from RAM.** These files fit, so before measuring I copy them into RAM.
  The program doesn't touch the SSD at all, which is the closest I get to the official
  setup. It measures how fast the code is. 0.1 s for 100M would be like 1 s for 1B.
- **1B from the SSD.** The real run on my machine, SSD included. It can never go below
  ~6 s.

**Only the 1M file is committed.** GitHub rejects files over 100 MB. The 100M file is
~1.38 GB and the 1B file is ~13.8 GB, and even compressed with `xz` the 1B file would be
roughly 3 GB. To run those two you have to generate them yourself (see
[Setup](notes/setup-generating-the-input-file.md)). The 1M file is already in the repo, so you can
try both versions right away, without Java:

```sh
git clone <this repo>
cd brrr
cargo build --release
./target/release/david  input/1M_measurements.txt
./target/release/jonhoo input/1M_measurements.txt
```

## Setup

The 1M input file is in the repo. The 100M and 1B files are too big for GitHub, so they're
generated locally with the official Java generator from the
[1BRC repo](https://github.com/gunnarmorling/1brc). It needs **Java 21**:

```sh
brew install openjdk@21                    # plus PATH and JAVA_HOME, see the guide below

git clone https://github.com/gunnarmorling/1brc.git
cd 1brc
./mvnw clean verify                        # builds the generator

./create_measurements.sh 1000000000        # 1B rows, ~3 min, 13.8 GB
./create_measurements.sh 100000000         # 100M rows
```

The step-by-step guide, with the Java setup, renaming the files and moving them into
`input/`, is in
[Setup: generating the input file](notes/setup-generating-the-input-file.md), mostly as
notes for myself.

## My machine

A MacBook Air with an Apple M4 (4 performance + 6 efficiency cores) and 16 GB of RAM. The
16 GB matters: the 1B file (13.8 GB) doesn't fit into RAM next to everything else, and the
SSD reads at most ~2.3 GB/s, so reading the 1B file from the SSD alone takes ~6 s.

The details (caches, clock frequency, SSD and RAM speeds, with the measurements behind them)
are in [My machine](notes/my-machine.md), mostly as notes for myself.

## Approach

I wrote my own version first and **then** ran Jon's version. Jon's code is there to produce
a known good output that I can check mine against.

The repo has two binaries, `david` and `jonhoo`. Both take the input file as their first
argument, so the same build works with either input. The results go to `output/<part>/`,
named after the input size and then the binary, so files for the same size sit next to each
other. For Part 001:

```sh
cargo build --release

# 1M
./target/release/david  input/1M_measurements.txt > output/part001/1M_david_output.txt
./target/release/jonhoo input/1M_measurements.txt > output/part001/1M_jonhoo_output.txt
diff output/part001/1M_david_output.txt output/part001/1M_jonhoo_output.txt   # no output = identical

# 100M
./target/release/david  input/100M_measurements.txt > output/part001/100M_david_output.txt
./target/release/jonhoo input/100M_measurements.txt > output/part001/100M_jonhoo_output.txt
diff output/part001/100M_david_output.txt output/part001/100M_jonhoo_output.txt

# 1B
./target/release/david  input/1B_measurements.txt > output/part001/1B_david_output.txt
./target/release/jonhoo input/1B_measurements.txt > output/part001/1B_jonhoo_output.txt
diff output/part001/1B_david_output.txt output/part001/1B_jonhoo_output.txt
```

How I measure time, memory and the rest is in [Measurements](part001/measurements.md#how-i-measure).

The workflow for each part:

1. Write and check the code against the **1M** file. Every run takes under a second.
2. Check it on the **100M** file for more realistic numbers.
3. Run the finished version on the **1B** file once, at the end, to get the real numbers.

The small files have one big blind spot: they fit in RAM easily, so they never show memory
problems. The 1B file is almost as big as my 16 GB of RAM, so any code that loads the whole
file at once gets into trouble there (see [1B over time](part001/measurements.md#1b-over-time)).
Every version has to **stream** the file to work on both inputs.
