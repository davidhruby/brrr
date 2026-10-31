# My machine

*Notes mostly for myself. They describe the laptop all the numbers in this journal come
from, and how fast it can get a file to the program. You don't need them to follow the
parts; they're here for anyone who wants to check where a number comes from. The
measurements on this page were made during [Part 001](../part001-first-naive-version.md).*

| | |
|---|---|
| Model | MacBook Air (Mac16,12) |
| Chip | Apple M4, 10 cores (4 performance + 6 efficiency) |
| RAM | 16 GB |
| Storage | 256 GB NVMe SSD (`APPLE SSD AP0256Z`), reads ~2.3 GB/s |
| OS | macOS 26.6 |
| Rust | `rustc 1.97.1` |

## CPU and cache layout

Cores of the same type are grouped into a **cluster**. Each core has its own L1 cache,
and all cores in a cluster share one L2 cache (and one clock frequency).

| | Performance cores (P) | Efficiency cores (E) |
|---|---|---|
| Cores | 4 | 6 |
| L1 instruction cache (per core) | 192 KB | 128 KB |
| L1 data cache (per core) | 128 KB | 64 KB |
| L2 cache (shared per cluster) | 16 MB | 4 MB |
| Cores sharing one L2 | 4 (whole P-cluster) | 6 (whole E-cluster) |

```text
Apple M4
├── P-cluster
│   ├── P0: own L1 ┐
│   ├── P1: own L1 ├── shared L2 (16 MB)
│   ├── P2: own L1 │
│   └── P3: own L1 ┘
├── E-cluster
│   └── E0…E5: each own L1 ── shared L2 (4 MB)
│
├── SLC (System Level Cache): shared by the whole chip (CPU, GPU, …)
└── RAM 16 GB (unified memory)
```

The **SLC (System Level Cache)** is the last cache level before RAM. Unlike L1 and L2 it
doesn't belong to the CPU but to the whole chip: both CPU clusters, the GPU, the Neural
Engine and the rest share it:

```text
P-cluster (L2)   E-cluster (L2)   GPU   Neural Engine   …
       \               |           |          /
        └──────────────┴─── SLC ───┴─────────┘
                             │
                     RAM (unified memory)
```

Apple doesn't publish its size (reports say ~8 MB for the base M4), and `sysctl` doesn't
show it.

Apple Silicon has no hyperthreading (`logicalcpu` = `physicalcpu`), so one core runs one
thread at a time. macOS decides which core a thread runs on and can move it between
P and E cores while it runs.

### Clock frequency

Apple doesn't publish the clock speeds, and `sysctl` doesn't show them on Apple Silicon.
Reviews report roughly **4.4 GHz for the P-cores** and **2.9 GHz for the E-cores**. The real
frequency changes with load and temperature, and the MacBook Air has no fan, so it may slow
down under a long load.

Frequency matters because it ties the counters from `/usr/bin/time -l` to time:

```text
CPU time      = CPU cycles / CPU frequency
IPC           = instructions / CPU cycles
```

- **CPU cycles** (`cycles elapsed`): how many clock ticks the CPU spent on the program, in
  total for the whole run.
- **CPU frequency**: how many ticks the CPU does per second, e.g. 4 GHz = 4 billion.
- **CPU time** (`user` + `sys`): how long the program was actually running on a core. Not
  the wall time: while the program waits (e.g. for swap), the CPU isn't ticking for it.
- **Instructions** (`instructions retired`): how many machine instructions the program
  executed in total. One line of Rust can be one instruction or thousands.
- **IPC** (instructions per cycle): how much work gets done per tick.

A single-threaded program that lands on an E-core runs slower even with the same code, so
the frequency also tells me whether a comparison between two runs is fair. The frequency
of each cluster, second by second, is shown by:

```sh
sudo powermetrics --samplers cpu_power -i 1000
```

What frequency my runs actually got is in
[Measurements](../part001/measurements.md#which-cores-did-it-run-on).

<details>
<summary>Raw output of <code>sysctl hw.perflevel0 hw.perflevel1</code></summary>

```text
hw.perflevel0.physicalcpu: 4
hw.perflevel0.physicalcpu_max: 4
hw.perflevel0.logicalcpu: 4
hw.perflevel0.logicalcpu_max: 4
hw.perflevel0.l1icachesize: 196608
hw.perflevel0.l1dcachesize: 131072
hw.perflevel0.l2cachesize: 16777216
hw.perflevel0.cpusperl2: 4
hw.perflevel0.name: Performance
hw.perflevel1.physicalcpu: 6
hw.perflevel1.physicalcpu_max: 6
hw.perflevel1.logicalcpu: 6
hw.perflevel1.logicalcpu_max: 6
hw.perflevel1.l1icachesize: 131072
hw.perflevel1.l1dcachesize: 65536
hw.perflevel1.l2cachesize: 4194304
hw.perflevel1.cpusperl2: 6
hw.perflevel1.name: Efficiency
```

</details>

## Virtual memory: from a virtual to a physical address

A program never sees physical RAM, only virtual addresses. This section is how the CPU
turns one into the other. The TLB sizes I measured are in [TLB](tlb.md).

### Three different things called L1 / L2 / L3

The same names show up in three places that have nothing to do with each other:

| Name | What it is | Where | Who manages it | What it holds |
|---|---|---|---|---|
| L1 / L2 **cache** | copies of data and code from RAM | in the CPU (L1 per core, L2 per cluster) | the hardware | 128-byte cache lines |
| L1 / L2 **TLB** | a cache of finished address translations | in the CPU, separate from the data caches | the hardware fills it; the kernel only throws entries out (`TLBI`) when it changes a mapping | virtual page → physical frame + permissions |
| L1 / L2 / L3 **page tables** | the levels of the tree that describes the translation | in RAM, one tree per process | the kernel | 8-byte descriptors |

The page table descriptors are entries of the page tables, not file descriptors. ARM calls
the page table levels "level 0–3 lookup" (on macOS a user address starts at level 1, so
only L1–L3 are used), Intel calls them PML4/PDPT/PD/PT. The only place where the three
meet: on a TLB miss, the page walker reads the descriptors through the normal data caches.

### TLB layout (one core)

```text
┌──────────┐   ┌──────────┐
│ L1 iTLB  │   │ L1 dTLB  │   small and fastest
│  (code)  │   │  (data)  │   L1 dTLB: ~128 entries (~2 MB)
└────┬─────┘   └────┬─────┘
     └──────┬───────┘
     miss   │
     ┌──────▼──────┐
     │   L2 TLB    │   bigger, shared by code and data
     │             │   ~3,072 entries (~48 MB)
     └──────┬──────┘
     miss   │
     ┌──────▼──────┐
     │ page walker │   reads descriptors from the page tables
     └─────────────┘   through the data caches / RAM
```

- **iTLB** translates the addresses of instructions (where the CPU fetches code from),
  **dTLB** the addresses of the data that loads and stores touch. One `ldr x0, [x1]` can
  need both: the iTLB for the address of the instruction, the dTLB for `x1`. They're
  separate because code fetch and data accesses happen in the same cycle in different
  parts of the core. The iTLB also checks that the page may be executed, the dTLB that it
  may be read or written.
- **Lookup order:** L1 TLB → L2 TLB → page walk. The result of a page walk is stored in
  both levels, so the next access to the same page is a hit.
- **No extra instructions.** The whole translation, including the page walk, is done by
  the hardware. A miss only makes the one instruction take longer: ~11–14 cycles for an
  L2 TLB hit, ~28 more for a page walk with the page tables in the cache (measured in
  [TLB](tlb.md)), hundreds if the descriptors have to come from RAM. Kernel code runs only
  on a **page fault**, when a descriptor is invalid.
- **A TLB hit gives a translation, not data.** With the physical address, the data is
  then read through the caches as usual: L1 → L2 → SLC → RAM. A TLB hit and a cache hit
  are two independent things.

### Virtual address space

- A page has 16 KB = 2¹⁴ bytes (`sysctl hw.pagesize` = 16384), so the lowest **14 bits**
  of an address are the **offset** inside the page.
- A user address on macOS has **47 bits**, so **33 bits** are left for the number of the
  page: 2³³ ≈ **8.6 billion** virtual pages.
- 8.6 billion pages × 16 KB = 2⁴⁷ bytes = **128 TB** of virtual address space per process.
  That's only the range of addresses, not memory: the machine has 16 GB of RAM.
- A flat table with one 8-byte entry per page would take 8.6 billion × 8 B = **64 GB per
  process**, which wouldn't fit into RAM. A process really uses a tiny part of the address
  space (code, heap, stack, libraries), so the page tables are a **tree** instead: where a
  descriptor in L1 or L2 is invalid, the whole region below it has no tables at all.
  A small process needs a few 16 KB tables, not 64 GB.

**Why three levels:** one table is one page: 16 KB / 8 B = **2,048 descriptors**, so the
index into one table has **11 bits** (2¹¹ = 2048). The 33 bits of the page number are
exactly 3 × 11. For comparison, x86-64 with 4 KB pages: 48 − 12 = 36 bits, 512 entries
per table = 9 bits per level, so 4 levels. The bigger pages save one memory read per page
walk, and one TLB entry covers 4× more memory.

How much one descriptor covers:

| Level | One descriptor covers |
|---|---|
| L3 (a page) | 16 KB |
| L2 | 32 MB (2,048 × 16 KB) |
| L1 | 64 GB (2,048 × 32 MB) |

The offset (14 bits) and the index (11 bits) both come from the same 16 KB, but the offset
counts bytes, the index counts 8-byte descriptors: 14 = 11 + 3, because 2³ = 8.

### Splitting an address

The virtual address itself carries the index for every level:

```text
bit:  46        36 35        25 24        14 13            0
     [ L1 index   | L2 index   | L3 index   |   offset     ]
        11 b         11 b         11 b          14 b
```

Example: `0x1_2345_6789`. Its 9 hex digits are only 36 bits, because the top bits are zero
and aren't written (like 42 instead of 00042). Padded with 11 zeros to 47 bits:

```text
VA = 00000000000 | 00010010001 | 10100010101 | 10011110001001
         L1            L2            L3           offset
          0           145          1301        0x2789 (10,121)
```

Each field is taken out with a shift and a mask:

- `>> n` drops the lowest n bits, so the field moves to the right end,
- `& mask` keeps only the lowest bits: `0x3FFF` = 14 ones, `0x7FF` = 11 ones.

| Field | Shift (bits of the fields below it) | Mask | Result |
|---|---|---|---|
| offset | 0 | `0x3FFF` | `0x2789` = 10,121 |
| L3 | 14 | `0x7FF` | `0x48D15 & 0x7FF` = `0x515` = 1,301 |
| L2 | 14 + 11 = 25 | `0x7FF` | `0x91` = 145 |
| L1 | 14 + 11 + 11 = 36 | `0x7FF` | 0 |

The L3 row in binary:

```text
  VA >> 14 = 00000000000 | 00010010001 | 10100010101     ← the offset is gone
& 0x7FF    = 00000000000 | 00000000000 | 11111111111
           = 00000000000 | 00000000000 | 10100010101     = 0x515 = 1301
```

In the CPU this costs nothing: the wires from those bits of the address simply lead to
the right part of the circuit.

### Page walk, step by step

On an L2 TLB miss, the page walker repeats the same step for each level:

```text
descriptor address = table address + index × 8   →   read 8 bytes   →   take bits 47:14
```

Bits 47:14 of a descriptor are the physical address of the next table (in L1 and L2) or of
the page (in L3); the mask for them is `0x0000_FFFF_FFFF_C000`. The physical address of the
L1 table comes from the register `TTBR0_EL1`, which the kernel switches together with the
process. The descriptor values below are made up for the example; the real ones are written
by the kernel when it maps memory, the CPU only reads them.

| Step | Descriptor address | Read value | Meaning |
|---|---|---|---|
| L1 | `0x8_0000_0000` (TTBR0) + 0 × 8 = `0x8_0000_0000` | `0x0000_0008_0000_4003` | bits 1:0 = `11`: table → L2 table at `0x8_0000_4000` |
| L2 | `0x8_0000_4000` + 145 × 8 (`0x488`) = `0x8_0000_4488` | `0x0000_0008_0000_8003` | table → L3 table at `0x8_0000_8000` |
| L3 | `0x8_0000_8000` + 1301 × 8 (`0x28A8`) = `0x8_0000_A8A8` | `0x0060_0002_048D_0F43` | page at `0x2_048D_0000` |

The flags of the L3 descriptor:

| Bits | Value | Meaning |
|---|---|---|
| 1:0 | `11` | valid page |
| 4:2 | `000` | AttrIndx: index into `MAIR_EL1`, normal memory |
| 7:6 | `01` | AP: user may read and write |
| 9:8 | `11` | SH: shared between cores |
| 10 | `1` | AF: the page has been accessed |
| 11 | `1` | nG: belongs to one process (its ASID) |
| 47:14 | `0x2_048D_0000` | physical address of the page |
| 53, 54 | `1`, `1` | PXN, UXN: code may not run from this page |

The final physical address is the page plus the offset:
`0x2_048D_0000 | 0x2789` = `0x2_048D_2789`. The translation is stored in the TLB, the
permissions are checked, and the data is read from the caches or RAM.

What can change the walk:

- **An invalid descriptor** (bit 0 = 0) at any level is a translation fault. The kernel
  either maps the page (allocates it, decompresses it, reads it back from swap) and the
  instruction runs again, or the process gets `SIGSEGV`.
- **A block descriptor** in L2 (bits 1:0 = `01`) ends the walk one level early: a 32 MB
  page, with a 25-bit offset.
- **The page walk cache** remembers the upper levels ("the L3 table for this region is
  at …"), so often only the L3 descriptor has to be read.

The virtual addresses never change. What the kernel changes is the content of the tables
(when memory is mapped, unmapped, swapped or its permissions change), and then it throws
the old translation out of the TLB. The L3 descriptors change most often, the L1 ones
almost never.

To check the numbers, in `python3`:

```python
va = 0x1_2345_6789
print((va >> 36) & 0x7FF, (va >> 25) & 0x7FF, (va >> 14) & 0x7FF, hex(va & 0x3FFF))
desc = 0x0060_0002_048D_0F43
page = desc & 0x0000_FFFF_FFFF_C000
print(hex(page), hex(page | (va & 0x3FFF)))
```

## Storage: how fast can the file be read?

| | |
|---|---|
| SSD | `APPLE SSD AP0256Z`, internal NVMe (`disk0`), 251 GB raw |
| Main volume | `Macintosh HD`, APFS, 245 GB |
| TRIM | Yes |

(From `system_profiler SPNVMeDataType`.)

**Why the SSD matters here, but not in the official challenge.** The official 1BRC runs read
the file from RAM on a server with 128 GB (see [The challenge](../intro.md#the-challenge)), so the speed
of the disk plays no role there. On my laptop it does, for two reasons:

1. **The 1B file is always read from the SSD.** 13.8 GB doesn't fit into my 16 GB of RAM next
   to everything else, so every 1B run has to read the whole file from the SSD, even a
   version that uses almost no memory. This sets a floor for every version of the program.
2. **A version that holds too much in memory also swaps.** If the program keeps more data
   in memory than fits (like my `read_to_string` version, which keeps the whole 13.8 GB
   file), macOS moves memory out to the SSD and back again. Then the SSD is busy with swap
   on top of reading the file (see [1B over time](../part001/measurements.md#1b-over-time)). A version that streams the
   file doesn't need to swap, so this part can be avoided by better code. The first one
   can't.

Before any parsing, the program has to get the bytes from the SSD into memory. To find that
ceiling I read the input files with `cat` into `/dev/null`, which reads and does nothing
else.

macOS keeps a copy of recently read files in free RAM, so that reading them again doesn't
have to go to the SSD. This part of RAM is called the **file cache** (in Activity Monitor
it's *Cached Files*). `sudo purge` empties it, so the first read below really comes from
the SSD ("cold"). The second read follows right after it ("warm") and can come from RAM.

| File | Size | Cold read (after `sudo purge`) | Second read right after |
|---|---|---|---|
| 100M | 1.38 GB | 0.62 s ≈ **2.2 GB/s** (SSD) | 0.11 s ≈ **12.5 GB/s** (from RAM, the file cache) |
| 1B | 13.8 GB | 6.06 s ≈ **2.3 GB/s** (SSD) | 5.96 s ≈ **2.3 GB/s** (still SSD!) |

What this shows:

- **My SSD reads about 2.2–2.3 GB/s.** Bigger reads don't help: `dd` with 16 MB blocks
  gives the same speed (`dd if=input/1B_measurements.txt of=/dev/null bs=16m` →
  13,795,468,189 bytes in 5.93 s = **2.32 GB/s**). Reading in parallel doesn't help either:
  [`fio`](https://github.com/axboe/fio) with 4 jobs, each reading its own 3 GB part of the
  file with 8 requests in flight and the cache bypassed, reads 12 GiB in 6.01 s =
  **2.14 GB/s** (see the raw output below). Apple doesn't publish SSD speeds, so ~2.3 GB/s is
  the measured ceiling of this SSD. No program can process the 1B file faster than
  ~6 s on this machine unless the file is already in RAM.
- **The 100M file stays in RAM after the first read, the 1B file doesn't.** After one read,
  macOS keeps the 1.38 GB file in the file cache, and the second read is ~5.6× faster. The
  13.8 GB file doesn't fit into RAM next to everything else in 16 GB. Reading it from start
  to end pushes its own beginning out of RAM before it reaches the end, so the next read
  starts from the SSD again.
- **So 100M benchmarks are "warm" unless I purge, and 1B benchmarks are always mostly
  "cold".** To compare runs fairly, I run `sudo purge` before each one (my measuring script
  does that, see [How I measure](../part001/measurements.md#how-i-measure)).
- **Today the SSD isn't the bottleneck.** Jon's version reads the 1B file at only
  13.8 GB / 107 s ≈ 129 MB/s, ~18× below what the SSD can deliver. The time goes to the CPU.
  This changes once the code gets faster.
- **Gotcha:** `/usr/bin/time -l` reports `block input operations 0` even for the cold
  reads that clearly came from the SSD. Like `swaps`, this counter isn't useful on macOS.

<details>
<summary>Raw output of the <code>cat</code> runs</summary>

```text
❯ sudo purge
❯ /usr/bin/time -l cat input/100M_measurements.txt > /dev/null
        0.62 real         0.00 user         0.13 sys
             1409024  maximum resident set size
                   6  page faults
                   0  block input operations
                   9  voluntary context switches
                2634  involuntary context switches
           927445036  instructions retired
           545254834  cycles elapsed

❯ /usr/bin/time -l cat input/100M_measurements.txt > /dev/null
        0.11 real         0.00 user         0.11 sys
             1409024  maximum resident set size
                   1  page faults
                   0  block input operations
                   0  voluntary context switches
                   5  involuntary context switches
           479119606  instructions retired
           372562795  cycles elapsed

❯ sudo purge
❯ /usr/bin/time -l cat input/1B_measurements.txt > /dev/null
        6.06 real         0.02 user         1.21 sys
             1409024  maximum resident set size
                   6  page faults
                   0  block input operations
                 131  voluntary context switches
               28071  involuntary context switches
          9450900425  instructions retired
          5381312692  cycles elapsed

❯ /usr/bin/time -l cat input/1B_measurements.txt > /dev/null
        5.96 real         0.02 user         1.23 sys
             1409024  maximum resident set size
                   5  page faults
                   0  block input operations
                   3  voluntary context switches
               30225  involuntary context switches
          9519953551  instructions retired
          5445595767  cycles elapsed
```

Lines that were zero in every run are left out.

</details>

<details>
<summary>Raw output of the parallel <code>fio</code> read</summary>

```text
❯ sudo purge
❯ fio --name=par4 --filename=input/1B_measurements.txt --readonly \
    --rw=read --bs=1m --ioengine=posixaio --iodepth=8 --numjobs=4 \
    --size=3g --offset_increment=3g --direct=1 --group_reporting
fio-3.43
Starting 4 processes
par4: (groupid=0, jobs=4): err= 0: pid=8766: Sat Oct 17 21:43:01 2026
  read: IOPS=2044, BW=2045MiB/s (2144MB/s)(12.0GiB/6010msec)
    clat (usec): min=1342, max=31423, avg=15587.22, stdev=1023.07
   bw (  MiB/s): min= 1970, max= 2118, per=100.00%, avg=2049.38, stdev=10.46, samples=44
  cpu          : usr=0.09%, sys=1.28%, ctx=51194, majf=0, minf=2086
  IO depths    : 1=0.1%, 2=0.1%, 4=0.1%, 8=99.8%, 16=0.0%, 32=0.0%, >=64=0.0%

Run status group 0 (all jobs):
   READ: bw=2045MiB/s (2144MB/s), 2045MiB/s-2045MiB/s (2144MB/s-2144MB/s), io=12.0GiB (12.9GB), run=6010-6010msec
```

Shortened to the main lines. `--readonly` makes sure fio never writes to the file, and
`--direct=1` bypasses the file cache.

</details>

### Reading the file from RAM

To see how fast a file can be read once it's in memory, I put the 100M file on a RAM disk
(a disk image that lives in memory) and read it with `dd`:

```sh
diskutil erasevolume APFS RAMDisk $(hdiutil attach -nomount ram://4194304)   # 2 GB RAM disk
cp input/100M_measurements.txt /Volumes/RAMDisk/
dd if=/Volumes/RAMDisk/100M_measurements.txt of=/dev/null bs=16m
hdiutil detach /Volumes/RAMDisk                                               # when done
```

| Read | Time | Speed |
|---|---|---|
| 1st `dd` from the RAM disk | 0.183 s | **7.5 GB/s** |
| 2nd and 3rd `dd` (now also in the file cache) | 0.144 s | **9.6 GB/s** |

So with the file already in RAM, one thread can read it **~4× faster than from the SSD**
(~2.3 GB/s). For the 1B file that would be ~1.4 s instead of ~6 s. On my machine that only
works with the 100M file, because 13.8 GB doesn't fit in RAM twice.

This is the speed of **reading a file** from RAM, not the speed of the RAM itself (the M4's
memory bandwidth is up to 120 GB/s according to Apple). `dd` and `cat` use the `read()`
system call, and for every call the kernel **copies** the data from the file cache into the
program's buffer, on a single thread. Every program that reads the file with `read()`
(including `BufReader`) is limited by this too. Memory mapping (`mmap`) avoids the copy.

Today's code is CPU-bound, so where the bytes come from barely matters yet. I still run the
1M and 100M measurements from the RAM disk: it guarantees the file is in RAM for every run,
and it will matter once the code gets faster.
