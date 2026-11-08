# Part 009 — Masked loads and dTLB misses on ARM and x86

**Dates:** 2026-11-07 – 2026-11-08
**Video progress:** stopped at [5:02:58](https://www.youtube.com/watch?v=tCY7p6dVAGE&t=18178s) when this part was published
**Code:** git tag [`part009`](https://github.com/davidhruby/brrr/tree/part009). To get the code exactly as it was for this part: `git checkout part009`

New here? Start with the [Introduction](intro.md).

## What changed

### 1. The question: why is Jon's code 4.6× slower on the Graviton?

The same code, Jon's [`src/jonhoo/main.rs`](../src/jonhoo/main.rs) at this point of the stream,
with the 1B file in RAM (`tmpfs` on Linux, see below):

| | Jon's PC (x86, from the stream) | AWS Graviton3 (`r7g.xlarge`) |
|---|---|---|
| Wall time | 25.05 s | 88.6 s |
| User time | 19.17 s | 87.6 s |
| Sys time | 3.37 s | 1.0 s |

The clock explains only part of it. The Graviton3 runs at ~2.6 GHz; a desktop x86 CPU at about
5 GHz (I don't know Jon's exact CPU, so this is an estimate). Per row that's roughly:

| | Jon's PC (estimated) | Graviton3 (measured) |
|---|---|---|
| Cycles per row | ~95 (19.17 s × ~5 GHz / 1 billion) | ~229 |

So even per cycle, the Graviton needs ~2.4× more. The counters on the Graviton (1B, per row,
`perf stat` with `:u`, see [Part 008](part008-short-strings.md)):

| Per row | Jon's code | My code (Part 007) |
|---|---|---|
| Instructions | ~402 | ~183 |
| Branches | ~124 | ~26 |
| Mispredicted branches | ~2.4 | ~0.6 |
| Loads through the dTLB | ~66 | ~39 |
| Cycles | ~229 | ~51 |

**The suspect: `u8x64::load_or_default` in Jon's `split_semi`.** `perf annotate` on the Graviton
shows what the compiler made of it: a 64-bit mask of which bytes belong to the line, then for
each of the 64 bytes a conditional branch on one bit of the mask and a load of **one byte**:

```text
addp …  /  umov …  /  bfi …  /  orr x8, …   build a 64-bit mask: which bytes belong to the line
tbz   w8, #0, …                              does byte 0 belong? if not, skip
ldr   b0, [x27]                              load ONE byte
tbnz  w8, #1, …                              does byte 1 belong? …
…                                            and so on for all 64 bytes
```

ARM (NEON) has no masked load, "load these 64 bytes, but only the allowed ones". x86 with
AVX-512 has one, a single instruction that loads 64 bytes and zeroes the ones outside the mask.
That would fit the numbers: the many branches and one-byte loads per row on the Graviton, and
the ~2.4 mispredictions per row (the branch at the end of the line depends on its length, ~11
cycles each, ~26 cycles per row). My own code loads whole 64-byte blocks with `from_slice` and
uses `load_or_default` only once, at the end of the file.

That's a hypothesis so far. What I want to find out in this part:

- **Is it really `load_or_default`?** A benchmark of `split_semi` alone on the Graviton and the
  M4: Jon's `load_or_default`, a plain `from_slice` of 64 bytes (when 64 bytes are available),
  and `rsplitn`.
- **What does it compile to on x86?** The same code on an x86 instance with AVX-512, and its
  instructions (`perf annotate`): is it really one masked load there?
- **How fast is Jon's code on x86?** The cycles per row on an x86 instance, against the ~229 on
  the Graviton.

**A RAM disk on Linux.** The macOS RAM disk (`diskutil … ram://…`) has an equivalent on Linux,
`tmpfs`, a file system that lives in memory. Unlike the page cache, the kernel can't drop its
pages, so no run reads from the disk:

```sh
sudo mkdir -p /mnt/ramdisk
sudo mount -t tmpfs -o size=15G tmpfs /mnt/ramdisk
cp /home/ubuntu/brrr/input/1B_measurements.txt /mnt/ramdisk/
# … measure with /mnt/ramdisk/1B_measurements.txt …
sudo umount /mnt/ramdisk
```

(Ubuntu also has a `tmpfs` at `/dev/shm`, half the RAM by default.)

### 2. A benchmark of the load alone

To see whether it's really the load, three ways to split a line at the `;` that differ **only
in how the 64-byte vector is made**, everything else (`simd_eq`, `first_set`, slicing the line)
the same, in [`benches/parse_semicolon_criterion.rs`](../benches/parse_semicolon_criterion.rs):

| Variant | Input | How the 64 bytes are loaded |
|---|---|---|
| **A** (Jon) | only the line | `u8x64::load_or_default(line)` |
| **B** | only the line | copy the line into a zeroed `[0u8; 64]`, then `u8x64::from_array(buf)` |
| **C** | the whole file and where the line starts | `u8x64::from_slice(&data[start..start + 64])`, one load without a mask |

A and B take the same input and give the same result: the line padded with zeros to 64 bytes.
B just does it by hand. C reads past the end of the line, which is fine, because the line goes
on with the next lines in the file, and the first `;` in those 64 bytes is always the one of
this line (the station comes before the temperature). Only for the last lines of the file,
with fewer than 64 bytes left, it falls back to `load_or_default`.

```rust
// B: same input as A (only the line), but the masked load done by hand:
// copy the line into a zeroed 64-byte buffer, then one plain load of the whole buffer
pub fn split_semicolon_simd_copy(line: &[u8]) -> (&[u8], &[u8]) {
    const DELIMITER: u8x64 = u8x64::splat(b';');
    if line.len() > 64 {
        // fallback to normal not optimized approach
        return line.rsplit_once(|c| *c == b';').unwrap();
    }
    let mut buf = [0u8; 64];
    buf[..line.len()].copy_from_slice(line);
    let del_eq = DELIMITER.simd_eq(u8x64::from_array(buf));
    // SAFETY: we are promised there is a ; in every line
    let index_of_delim = unsafe { del_eq.first_set().unwrap_unchecked() };
    (&line[..index_of_delim], &line[index_of_delim + 1..])
}

// C: load 64 bytes from the whole file, starting at the line, without any mask;
// the bytes after the end of the line don't matter, the first ';' is always in the line
pub fn split_semicolon_simd_from_data<'a>(data: &'a [u8], start: usize, line: &'a [u8]) -> (&'a [u8], &'a [u8]) {
    const DELIMITER: u8x64 = u8x64::splat(b';');
    if line.len() > 64 {
        // fallback to normal not optimized approach
        return line.rsplit_once(|c| *c == b';').unwrap();
    }
    let simd_value = if start + 64 <= data.len() {
        u8x64::from_slice(&data[start..start + 64])
    } else {
        // the last lines of the file: fewer than 64 bytes left
        u8x64::load_or_default(line)
    };
    let del_eq = DELIMITER.simd_eq(simd_value);
    // SAFETY: we are promised there is a ; in every line
    let index_of_delim = unsafe { del_eq.first_set().unwrap_unchecked() };
    (&line[..index_of_delim], &line[index_of_delim + 1..])
}
```

Before measuring, the benchmark checks once that all three give the same result as `rsplitn`
for every line of the 1M file. On the M4 and on the AWS Graviton3 (`r7g.xlarge`), both nightly,
per line:

| 1M file, per line | M4 | Graviton3 |
|---|---|---|
| `rsplitn` (no SIMD) | 2.65 ns | 4.94 ns |
| **A:** `load_or_default` (Jon) | **17.28 ns** | **25.78 ns** |
| **B:** copy into `[0u8; 64]`, `from_array` | 7.80 ns | 13.22 ns |
| **C:** `from_slice` from the whole file | **1.29 ns** | **3.13 ns** |
| A / B | 2.2× | 2.0× |
| A / C | 13× | 8.2× |
| `rsplitn` / C | 2.1× | 1.6× |

The same order on both, C < `rsplitn` < B < A, so it isn't something of Apple Silicon, but of
ARM: neither has a masked load. The Graviton is slower overall (a lower clock), but the ratios
are about the same.

- **A against B**, the same input and the same result, only another way of doing it: **2.2×**
  on the M4, 2.0× on the Graviton.
  So on ARM, `load_or_default` really is most of the cost: up to 64 one-byte loads behind
  branches cost more than a copy of a variable length plus one big load.
- **B is still slower than `rsplitn`**: zeroing 64 bytes and the copy of a variable length on
  every line cost something too.
- **C is 13× (M4) and 8× (Graviton) faster than A, and faster than `rsplitn` too**: one
  64-byte load, no mask, no copy, no branches. Only like this does SIMD pay off for finding the
  `;`, and it's what my iterator does.

### 3. The same benchmark on x86, with and without AVX-512

The same benchmark on an AWS x86 instance (Intel Sapphire Rapids), which has AVX-512
(`grep -o avx512bw /proc/cpuinfo` finds it). It ran twice. Rust's default x86-64 target is the
generic one, which must run on any x86-64 CPU, so it doesn't use AVX-512 even when the CPU has
it. Only `-C target-cpu=native` lets the compiler use everything this CPU has:

```sh
cargo bench --bench parse_semicolon_criterion -- --save-baseline generic
RUSTFLAGS="-C target-cpu=native" cargo bench --bench parse_semicolon_criterion -- --baseline generic
```

| 1M file, per line | M4 | Graviton3 | x86, generic | **x86, AVX-512** (`native`) |
|---|---|---|---|---|
| `rsplitn` (no SIMD) | 2.65 ns | 4.94 ns | 4.84 ns | 5.25 ns |
| **A:** `load_or_default` (Jon) | 17.28 ns | 25.78 ns | **32.72 ns** | **1.81 ns** |
| **B:** copy into `[0u8; 64]`, `from_array` | 7.80 ns | 13.22 ns | 9.76 ns | 9.82 ns |
| **C:** `from_slice` from the whole file | 1.29 ns | 3.13 ns | 2.27 ns | 1.76 ns |
| A / C | 13× | 8.2× | 14× | **1.03×** |

- **Generic x86 is as bad as ARM**, even a bit worse (A / C 14×). Without AVX-512 there is no
  masked load on x86 either. So it isn't something of ARM, but of **any CPU without a masked
  load**.
- **With AVX-512, A is 94 % faster** (32.72 → 1.81 ns) and as fast as C. Jon's `split_semi` is
  fast only because his build uses AVX-512.
- **B doesn't change** (9.76 → 9.82 ns). Its cost is the copy of a variable length into the
  buffer, not the comparison, and AVX-512 doesn't help with that.
- **C is 23 % faster**: the comparison of 64 bytes is one 512-bit instruction instead of four
  128-bit ones.
- `rsplitn` is 8 % slower with `native`, other code generated for this CPU; it doesn't change
  the result.

**The same on AMD Zen 5** (`r8a.xlarge`, AMD EPYC 9R45, ~4.5 GHz), which has AVX-512 too:

| 1M file, per line | Intel SPR, generic | Intel SPR, AVX-512 | **Zen 5, generic** | **Zen 5, AVX-512** (`znver5`) |
|---|---|---|---|---|
| `rsplitn` (no SIMD) | 4.84 ns | 5.25 ns | 2.82 ns | 2.78 ns |
| **A:** `load_or_default` (Jon) | 32.72 ns | 1.81 ns | 17.87 ns | **2.21 ns** |
| **B:** copy into `[0u8; 64]`, `from_array` | 9.76 ns | 9.82 ns | 6.71 ns | 6.95 ns |
| **C:** `from_slice` from the whole file | 2.27 ns | 1.76 ns | 1.56 ns | 1.42 ns |
| A / C | 14× | 1.03× | 11.5× | **1.56×** |

The same picture: with the masked load, A is 88 % faster (17.87 → 2.21 ns). On Zen 5 it doesn't
quite get to C, though (1.56× against 1.03× on Intel), so the masked load costs a bit more there
than a plain load.

**Gotcha: `native` and a reused build.** I changed the instance type from Intel to AMD and ran
the same `RUSTFLAGS="-C target-cpu=native" cargo bench …` as before. Cargo built nothing (0.44 s)
and ran the old binary, built for Sapphire Rapids, which crashed with `SIGILL: illegal
instruction` (it used an instruction Zen 5 doesn't have; Sapphire Rapids has some that Zen 5
lacks, AVX-512 FP16 for example). Cargo does
rebuild when `RUSTFLAGS` change, but the text `-C target-cpu=native` was the same; it doesn't
know that "native" means another CPU now. Naming the CPU, `-C target-cpu=znver5`, is another
text, so Cargo rebuilds, and on this CPU it's the same as `native`.

**The instruction.** The disassembly of the `native` benchmark binary, every masked 64-byte load
and the function it's in:

```sh
objdump -d --no-show-raw-insn -C /home/ubuntu/brrr/target/release/build/brrr/35bfd61948148472/out/parse_semicolon_criterion-35bfd61948148472 | awk '/>:$/ {f=$0} /vmovdqu8.*%zmm.*\{%k/ {print f; print "    " $0}'
```

```text
<criterion::bencher::Bencher>::iter::<(), parse_semicolon_criterion::bench_parse::{closure#2}::{closure#0}>
      19ad1d:    vmovdqu8 0x0(%rbp),%zmm2{%k1}{z}
<criterion::bencher::Bencher>::iter::<(), parse_semicolon_criterion::bench_parse::{closure#4}::{closure#0}>
      19b20a:    vmovdqu8 (%r14),%zmm2{%k1}{z}
parse_semicolon_criterion::main
      1a8121:    vmovdqu8 (%rbx),%zmm0{%k1}{z}
parse_semicolon_criterion::main
      1a82e0:    vmovdqu8 (%rbx),%zmm0{%k1}{z}
```

(`objdump` uses the AT&T syntax by default, so registers have a `%` in front: the mask is
`{%k1}`, not `{k1}`.)

| Part | Meaning |
|---|---|
| `vmovdqu8` | load bytes (8 bits each) from an address that doesn't have to be aligned |
| `(%rbx)` | the address: the start of the line |
| `%zmm0` | a 512-bit register, 64 bytes at once |
| `{%k1}` | the mask, 64 bits in the register `k1`: a 1 means "load this byte" |
| `{z}` | zero the bytes outside the mask, the "or default" |

The bytes outside the mask aren't read from memory at all, so a line that ends right before an
unmapped page doesn't crash. That's why the compiler may use it instead of the 64 one-byte loads
behind branches on ARM.

The closures of `bench_parse` are numbered in the order they appear in the code: `#0` is the
`|c| *c == b'\n'` that splits the file into lines, `#1` the `rsplitn` bench, **`#2` variant A**,
`#3` variant B (no masked load) and **`#4` variant C**, its fallback for the end of the file. The
two in `main` are the `assert_eq!` check before measuring: the compiler inlined all of
`bench_parse` into `main`. The functions from `src/david/lib.rs` were inlined into the
benchmark loops even though they're in another crate, which is why they don't show up as
symbols of their own.

So on x86 with AVX-512, `u8x64::load_or_default` is **one instruction**; on ARM and on generic
x86-64 it's up to 64 conditional one-byte loads.

### 4. Jon's whole program on x86

The micro-benchmark answers it for `split_semi` alone. How much is it for the whole program?
Jon's [`src/jonhoo/main.rs`](../src/jonhoo/main.rs) on the same x86 instance, the 1B file
generated there (see [measurements](notes/measurements.md)) and copied to a `tmpfs` RAM disk
(change #1). Two builds into separate directories, so they don't overwrite each other's binary:

```sh
cd /home/ubuntu/brrr
cargo build --release --bin jonhoo --manifest-path /home/ubuntu/brrr/Cargo.toml --target-dir /home/ubuntu/target-generic
RUSTFLAGS="-C target-cpu=native" cargo build --release --bin jonhoo --manifest-path /home/ubuntu/brrr/Cargo.toml --target-dir /home/ubuntu/target-native
```

(The `cd` matters: rustup picks the toolchain by the `rust-toolchain.toml` of the **current
directory**, not by `--manifest-path`. Run from another directory, it takes stable, which
refuses `#![feature(portable_simd)]`.)

First plainly with `time`, the way Jon measured, then with the counters:

```sh
time /home/ubuntu/target-native/release/jonhoo /mnt/ramdisk/1B_measurements.txt > /dev/null
sudo perf stat --all-user -e cycles,instructions,branches,branch-misses /home/ubuntu/target-native/release/jonhoo /mnt/ramdisk/1B_measurements.txt > /home/ubuntu/out-native.txt
```

The outputs of both builds are the same (`cmp` prints nothing). Two runs of the `native` build
differed by 2 ms. Then the same on AMD Zen 5 (`r8a.xlarge`, AMD EPYC 9R45), the `native` build
into its own `--target-dir /home/ubuntu/target-zen5` (see the gotcha in change #3).

| 1B, `tmpfs` | Jon's PC (x86, from the stream) | Graviton3 | Intel SPR, generic | Intel SPR, AVX-512 | Zen 5, generic | **Zen 5, AVX-512** |
|---|---|---|---|---|---|---|
| Wall time | 25.05 s | 88.6 s | 62.29 s | 36.85 s | 40.03 s | **22.31 s** |
| User time | 19.17 s | 87.6 s | 61.81 s | 36.40 s | 39.75 s | **22.04 s** |
| Sys time | 3.37 s | 1.0 s | 0.48 s | 0.45 s | 0.28 s | 0.26 s |

Per row (`perf stat`, `:u`):

| Per row | Graviton3 | Intel SPR, generic | Intel SPR, AVX-512 | Zen 5, generic | **Zen 5, AVX-512** |
|---|---|---|---|---|---|
| Cycles | ~229 | ~209 | ~122 | ~179 | **~99** |
| Instructions | ~402 | ~529 | ~267 | ~529 | ~279 |
| Branches | ~124 | ~127 | ~62 | ~127 | ~62 |
| Mispredicted branches | ~2.4 | ~1.37 | ~1.13 | ~1.44 | ~1.14 |
| Instructions per cycle | ~1.76 | ~2.53 | ~2.19 | ~2.96 | ~2.82 |
| Clock (cycles / user time) | ~2.6 GHz | ~3.36 GHz | ~3.37 GHz | ~4.50 GHz | ~4.49 GHz |

- **One compiler flag makes the whole program 41 % faster** on Intel (62.3 → 36.9 s), 44 % on
  Zen 5 (40.0 → 22.3 s). In the micro-benchmark it was 94 % and 88 %, but `split_semi` is only
  a part of the work per row (Amdahl).
- **The masked load removes ~65 branches and ~260 instructions per row.** That fits the 64
  conditional one-byte loads, one branch for each of the 64 bytes.
- **The Graviton runs the same as generic x86**, ~124 branches per row, with fewer but bigger
  instructions. Both lack a masked load.
- **Jon's build must have used AVX-512.** With the ~209 cycles per row of the generic build, his
  19.17 s of user time would need a clock of ~10.9 GHz; with the ~122 of the `native` build,
  ~6.4 GHz. A desktop CPU at ~5 – 5.7 GHz with a somewhat better core than this server one gets
  close to the second, not to the first.
- **Zen 5 with AVX-512 beats Jon's wall time** (22.31 against 25.05 s), though not his user
  time (22.04 against 19.17 s). At its ~99 cycles per row, his 19.17 s would mean ~5.2 GHz, the
  clock of a desktop Zen 4 or Zen 5. So my estimate of ~95 cycles per row for Jon's PC in
  change #1 was about right.
- **Zen 5 also needs fewer cycles per row than Sapphire Rapids**: ~99 against ~122 with
  AVX-512, ~179 against ~209 without. On top of that, it runs at ~4.5 instead of ~3.4 GHz.

So the 4.6× from change #1 is roughly three things multiplied together:

| Cause | Factor |
|---|---|
| The masked load: generic → `native` on the same x86 instance | ~1.7× |
| Graviton3 → Intel SPR server, both without the masked load (~229 → ~209 cycles per row, and 2.6 → 3.37 GHz) | ~1.4× |
| Intel SPR server → Jon's desktop (a higher clock, maybe a better core) | ~1.9× |

Zen 5 splits the last row: ~1.65× from Intel SPR to Zen 5 (36.40 → 22.04 s user, a better core
and ~4.5 GHz), and only ~1.15× from Zen 5 to Jon's desktop (22.04 → 19.17 s), about the step
from ~4.5 to ~5.2 GHz.

**My code on the same instance.** [`src/david/main.rs`](../src/david/main.rs), built the same
two ways. Its output is the same as Jon's (`cmp` prints nothing):

| 1B, `tmpfs`, wall time | Intel SPR, generic | Intel SPR, AVX-512 | Zen 5, generic | Zen 5, AVX-512 |
|---|---|---|---|---|
| Jon's | 62.29 s | 36.85 s | 40.03 s | 22.31 s |
| **Mine** | 15.51 s | **15.05 s** | **10.00 s** | 10.35 s |

| Mine, per row | Graviton3 | Intel SPR, generic | Intel SPR, AVX-512 | Zen 5, generic | Zen 5, AVX-512 |
|---|---|---|---|---|---|
| Cycles | ~51 | ~52 | ~51 | ~45.6 | ~47.1 |
| Instructions | ~183 | ~183 | ~162 | ~183 | ~167 |
| Branches | ~26 | ~24 | ~21 | ~24 | ~21 |
| Mispredicted branches | ~0.6 | ~0.54 | ~0.53 | ~0.54 | ~0.54 |
| Instructions per cycle | ~3.6 | ~3.50 | ~3.18 | ~4.01 | ~3.55 |

(User time is a bit more than wall time because my program has a helper thread, and user time
adds up all threads.)

- **AVX-512 helps my code only by 3 % on Intel** (15.51 → 15.05 s), against 41 % for Jon's,
  and on Zen 5 the AVX-512 build is even **3 % slower** (10.00 → 10.35 s). I use
  `load_or_default` only once, at the end of the file. What's left is the comparison of 64
  bytes, one 512-bit instruction instead of four 128-bit ones: ~16 – 21 instructions fewer per
  row, but not fewer cycles. Why it's slower on Zen 5, these four counters don't tell.
- **Mine needs the same ~51 cycles per row on the Graviton and on Intel**, and even the same
  ~183 instructions in the generic build. Without the masked load in the way, the two cores are
  about equally good per cycle; the difference in time is the clock. Zen 5 needs ~11 % fewer
  cycles (~45.6) at a ~1.3× higher clock.
- **Mine is ~2.4× faster than Jon's at this point of the stream on Intel with AVX-512** (~2.2×
  on Zen 5), and ~4× faster without it.

### 5. The dTLB misses Jon complained about, on x86

On the stream, Jon complained about many dTLB misses. On the Graviton
([Part 008](part008-short-strings.md)) I saw ~25× more with `StrVec`, but they cost no time.
Jon's machine is x86, though, so the same question on Zen 5 (`r8a.xlarge`), which is likely
closer to his CPU.

**A quick recap of the TLB.** The program works with virtual addresses, and the CPU translates
them to physical ones page by page (4 KB on Linux). The translations are in page tables in
memory, a tree of 4 levels, so a full translation (a *page walk*) is up to 4 more loads. The
TLB is a small cache of finished translations, in two levels: a small, fast L1 dTLB (on the
order of 100 entries) and a bigger L2 TLB (thousands). A miss in the L1 that's found in the L2
costs a few cycles at most; only a miss in both means a page walk.

**The events.** AMD splits the misses exactly the way I needed (`perf list | grep -i tlb`):

| Event | What it counts |
|---|---|
| `ls_l1_d_tlb_miss.all` | every miss in the L1 dTLB (what `dTLB-load-misses` counted on the Graviton) |
| `ls_l1_d_tlb_miss.all_l2_miss` | a miss in the L2 too: a real page walk |
| `ls_l1_d_tlb_miss.tlb_reload_4k_l2_hit` | an L1 miss found in the L2, for a 4 KB page |
| `ls_l1_d_tlb_miss.tlb_reload_coalesced_page_hit` | the same for a "coalesced" page (Zen can keep 4 neighbouring 4 KB pages as one entry) |
| `ls_l1_d_tlb_miss.tlb_reload_2m_l2_hit` | the same for a 2 MB page |

The AWS instance gives the virtual machine only **5** general counters, even with the NMI
watchdog turned off (`sudo sysctl kernel.nmi_watchdog=0`, which normally frees one). With
`cycles` and all five events, two of them shared a counter (`perf` shows a percentage after the
number, like `(1.48%)`), so I measured `cycles` plus four of them at a time.

**Jon's code before and after the change he complained about.** I cloned
[his repo](https://github.com/jonhoo/brrr) and built two commits, besides his current code and
mine, all with `-C target-cpu=znver5`:

- [`4398871`](https://github.com/jonhoo/brrr/commit/4398871) "preallocate map": SipHash,
  `with_capacity(10_000)`, `Vec<u8>` keys;
- [`c31892f`](https://github.com/jonhoo/brrr/commit/c31892f53aff733d71009ca3a062acc88b64e160)
  "hasher": his `FastHasher` and `with_capacity_and_hasher(100_000, …)`, `Vec<u8>` keys;
- `c31892f` with only `100_000` changed back to `10_000`, to separate the capacity from the
  hasher.

```sh
cd /home/ubuntu/jonhoo-brrr
git checkout 4398871
RUSTFLAGS="-C target-cpu=znver5" cargo +nightly build --release --target-dir /home/ubuntu/target-jon-4398871
cd /mnt/ramdisk
sudo perf stat --all-user -e cycles,ls_l1_d_tlb_miss.all,ls_l1_d_tlb_miss.all_l2_miss,ls_l1_d_tlb_miss.tlb_reload_4k_l2_hit,ls_l1_d_tlb_miss.tlb_reload_coalesced_page_hit /home/ubuntu/target-jon-4398871/release/brrr > /dev/null
```

Three gotchas on the way:

- **His repo has no `rust-toolchain.toml`**, so the default stable refused `#![feature]`;
  `cargo +nightly` picks the nightly.
- **At these commits his program opens `"measurements.txt"` in the current directory** and
  ignores its arguments, so it runs from `/mnt/ramdisk`, with the file under that name too.
- **A symbolic link to the file failed with `PermissionDenied`, even for root.** `tmpfs` in
  `/mnt/ramdisk` belongs to root and has the sticky bit (like `/tmp`), and with
  `fs.protected_symlinks` a symlink in such a directory may only be followed by its owner or
  the directory's owner, not by root running a program through `sudo`. A hard link
  (`ln /mnt/ramdisk/1B_measurements.txt /mnt/ramdisk/measurements.txt`) works: just a second
  name for the same file, no extra memory.

| Zen 5, 1B, per row | `4398871` SipHash, 10,000 | `c31892f` Fast, 100,000 | `c31892f` Fast, 10,000 | Jon's current (`StrVec`, 100,000) | **Mine** (16,384 buckets) |
|---|---|---|---|---|---|
| Misses in the L1 dTLB | ~0.52 | **~1.06** | ~0.52 | ~1.02 | ~0.38 |
| of them found in the L2 (4 KB) | ~0.51 | ~1.06 | ~0.51 | ~1.01 | ~0.37 |
| Page walks | ~0.0066 | ~0.0060 | ~0.0062 – 0.0077 | ~0.0063 | ~0.0048 |
| Cycles | ~132 | ~102.0 – 102.6 | ~100.2 – 102.8 | ~100 | ~46.8 |

(Where there are two numbers, the same binary ran twice.)

- **Yes, it happens on x86: about one miss in the L1 dTLB for every row**, a billion in total,
  in Jon's code after `c31892f`.
- **It's the capacity, not the hasher and not `StrVec`.** `c31892f` with 10,000 is back at
  ~0.52, like `4398871`. `StrVec` doesn't add any here (~1.06 → ~1.02); the ~25× of the
  Graviton doesn't repeat.
- **Nearly all of them are found in the L2 TLB.** The page walks are ~0.006 per row in all
  versions, ~6 million on 1 billion rows, about two per 4 KB page of the file (3.4 million
  pages). So they come from reading the file, not from the table.

**Why the capacity matters, although there are always 413 stations.** `with_capacity` allocates
the whole array of buckets at once, however few of them are used, and a key's bucket is
`hash & (buckets - 1)`, a random place in the **whole** array:

| | `with_capacity(10_000)` | `with_capacity(100_000)` |
|---|---|---|
| Buckets | 16,384 | 131,072 |
| Allocated (~48 B per entry + 1 control byte) | ~0.8 MB, ~200 pages | ~6.4 MB, ~1,570 pages |
| Buckets in use | 413 | 413 |
| Data really used | ~20 KB | ~20 KB |
| Different pages touched | ~200 (~2 stations per page) | ~390 (nearly one page per station) + 32 pages of control bytes |

What matters for the TLB isn't how many bytes the program uses, but **on how many different
pages** they lie. ~20 KB would fit in the L1 cache easily; ~390 pages don't fit in an L1 dTLB
of ~100 entries. My rough model (2 pages per lookup, the control bytes and the entry, with
~96 L1 entries; I don't know the exact number for Zen 5) gives ~0.5 and ~1 miss per row, close
to the measurement.

**Do they cost anything?** Halving them (100,000 → 10,000) didn't change the cycles. To be
sure, I removed most of them without changing anything else: **transparent huge pages** (THP).
Linux can back a program's memory with 2 MB pages instead of 4 KB ones, without the program
knowing; one TLB entry then covers 512× more memory. In the default mode, `madvise`, only memory
the program asks for gets them (`madvise(…, MADV_HUGEPAGE)`), and neither Jon's code nor mine
asks. Switched to `always`, the kernel gives them to every big enough allocation on the heap.
The file in `tmpfs` doesn't get them (huge pages are off for `tmpfs` by default), so only the
table changes:

```sh
cat /sys/kernel/mm/transparent_hugepage/enabled
echo always | sudo tee /sys/kernel/mm/transparent_hugepage/enabled
# … measure …
echo madvise | sudo tee /sys/kernel/mm/transparent_hugepage/enabled
```

| Zen 5, 1B, per row (`c31892f`) | 100,000, `madvise` | **100,000, `always`** | 10,000, `madvise` | 10,000, `always` |
|---|---|---|---|---|
| Misses in the L1 dTLB | ~1.06 | **~0.255** | ~0.52 | ~0.52 |
| L1 misses found in the L2 for 2 MB pages | 0 | ~0.00003 | 0 | 0 |
| Cycles | ~102.0 – 102.6 | **~100.4** | ~100.2 – 102.8 | ~102.3 |

- **THP did what I expected.** With 100,000, the misses dropped 4× (the 3 – 4 entries of the
  2 MB pages stay in the L1 dTLB all the time; the ~0.25 left are the edges of the table that
  don't fill a whole 2 MB page, the small allocations of the `Vec<u8>` keys, and the file).
  With 10,000 nothing changed: ~0.8 MB is less than one 2 MB page.
- **The cycles don't follow the misses.** All six runs are between 100 and 103 cycles per row,
  whether there were 0.25 or 1.06 misses per row; the same binary alone varied by ~2.5 %. If an
  L1 miss found in the L2 cost ~7 cycles, 0.8 misses fewer per row would be ~5.6 cycles (~5 %),
  more than the noise. So they cost **at most ~2 – 3 cycles each, more likely about nothing**:
  the CPU does other work out of order while the translation comes from the L2 TLB.
- **The seconds lied a little here.** One run of the 10,000 version took 6 % more user time
  than the 100,000 one, but only 0.7 % more cycles: the CPU ran at ~4.25 instead of ~4.5 GHz in
  that run. On a cloud instance the clock can change between runs, so I compared the cycles.

So Jon's dTLB misses are real on x86, about one per row with the 100,000 capacity, twice as
many as with 10,000, but they don't make the program slower. Like on the Graviton with
`StrVec`, a counter that doubles doesn't have to mean a slower program; what each event costs
decides. I checked it the usual way: change only that one thing (a smaller table, huge
pages), watch the cycles, and run the same binary twice to know the noise.

### 6. Summary

**`load_or_default` on the four CPUs.** It gives the same result everywhere, but compiles to one
instruction where the CPU has a masked load (x86 with AVX-512) and to up to 64 one-byte loads
behind branches where it doesn't (ARM, generic x86-64). On the same Intel CPU it was ~1.8 or
~33 ns per line, depending only on the compiler flag:

| 1M file, `load_or_default`, per line | Without a masked load | With a masked load (AVX-512) |
|---|---|---|
| Apple M4 | 17.28 ns | – |
| AWS Graviton3 | 25.78 ns | – |
| Intel Sapphire Rapids | 32.72 ns | 1.81 ns |
| AMD Zen 5 | 17.87 ns | 2.21 ns |

Loading 64 bytes from the whole mapped file with `from_slice` (variant C), which needs nothing
special from the CPU, was the fastest or close to it on all of them.

**The build.** Rust's default x86-64 target uses no AVX-512 even on a CPU that has it.
`-C target-cpu=native` made Jon's whole program 41 – 44 % faster, and the binary then runs only
on CPUs with the same instructions (`SIGILL` on Zen 5). Jon's 19.17 s of user time fits only an
AVX-512 build (~122 cycles per row), not a generic one (~209 cycles per row, ~10.9 GHz).

**Code, core and clock, separated by cycles and instructions per row:**

- the code: generic → AVX-512 removed ~65 branches and ~250 instructions per row;
- the core: Zen 5 needed ~13 – 19 % fewer cycles than Sapphire Rapids for the same code;
- the clock: ~2.6 GHz on the Graviton3, ~3.4 GHz on Sapphire Rapids, ~4.5 GHz on Zen 5.

My code, which doesn't depend on the masked load, needed about the same ~51 cycles per row on
the Graviton3 and on Sapphire Rapids.

**Micro-benchmark against the whole program.** The 94 % of the load alone became 41 % in Jon's
whole program, `split_semi` being only a part of the work per row. The 3 % that AVX-512 gave my
program on Intel was 3 % slower on Zen 5.

**dTLB misses.** About one per row in Jon's code on x86 with the 100,000 capacity, half that
with 10,000, and no measurable cost in either case.

## What I learned

## Next steps

Back to the stream, from 5:02:58. The question of this part is answered: Jon's code is 4.6×
slower on the Graviton because of `load_or_default`, which has no masked load to compile to on
ARM (~1.7×), plus the lower clock and the core (~1.4×), plus the difference between a cloud
server and Jon's desktop (~1.9×). My own code doesn't depend on it, so it doesn't need to
change because of this part.

From now on, when Jon's numbers from the stream and mine differ a lot, the first things to
check are the CPU and whether a part of his code relies on instructions mine doesn't have, before
looking for a difference in the code itself.

## TODO

Things I want to try later. I'm not doing them yet, because Jon may get to them in a later
part of the stream, and I don't want to get ahead of him.

- **A smaller table on Linux:** with 4 KB pages, the 16,384 buckets of
  `with_capacity(10_000)` spread over ~130 – 160 pages and cause ~0.75 dTLB misses per row.
  Measure `HashMap::new()` (512 buckets, ~20 KB, 5 pages) there; on the M4, the capacity was
  7 % faster. On Zen 5 (change #5), halving the misses changed nothing, so probably not worth
  much; the Graviton may still differ.
- **Why Jon's code with `Vec<u8>` had only 0.004 dTLB misses per row on the Graviton**
  ([Part 008](part008-short-strings.md)), with the same 131,072-bucket table that gives ~1 per
  row on Zen 5, and mine ~0.75 with a table 8× smaller. Either the generic `dTLB-load-misses`
  counts something else on the Graviton, or something there I don't see yet.
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
