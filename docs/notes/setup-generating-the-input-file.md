# Setup: generating the input file

*Notes mostly for myself. You only need this page if you want to run the 100M and 1B input
files yourself. It belongs to [Part 001](../part001-first-naive-version.md).*

You only need this for the 100M and 1B files; the 1M file is already committed. They are
generated locally with the official Java generator from
[`gunnarmorling/1brc`](https://github.com/gunnarmorling/1brc).

## 1. Install Java 21

The generator needs JDK 21.

```sh
# Install JDK 21
brew install openjdk@21

# Register it with macOS so the system Java tools can find it (asks for your password)
sudo ln -sfn /opt/homebrew/opt/openjdk@21/libexec/openjdk.jdk \
  /Library/Java/JavaVirtualMachines/openjdk-21.jdk

# Add it to PATH and set JAVA_HOME permanently
echo 'export PATH="/opt/homebrew/opt/openjdk@21/bin:$PATH"' >> ~/.zshrc
echo 'export JAVA_HOME="/opt/homebrew/opt/openjdk@21"' >> ~/.zshrc
source ~/.zshrc
```

Verify:

```console
$ java --version
openjdk 21.0.12.1 2026-08-18
OpenJDK Runtime Environment Homebrew (build 21.0.12.1)
OpenJDK 64-Bit Server VM Homebrew (build 21.0.12.1, mixed mode, sharing)
```

## 2. Clone and build the 1BRC repo

```sh
git clone https://github.com/gunnarmorling/1brc.git
cd 1brc
./mvnw clean verify
```

This builds `target/average-1.0.0-SNAPSHOT.jar`, which contains the generator.

## 3. Generate the measurements

The argument is the number of rows. The generator always writes `measurements.txt` in the
root of the `1brc` repo, so rename the file after each run.

The 1 billion row file:

```console
$ ./create_measurements.sh 1000000000
Wrote 50,000,000 measurements in 8045 ms
Wrote 100,000,000 measurements in 18001 ms
...
Wrote 950,000,000 measurements in 188166 ms
Created file with 1,000,000,000 measurements in 198260 ms
```

It took **~3 min 18 s** and wrote **13,795,468,189 bytes ≈ 13.8 GB**. Make sure there is
enough free disk space first.

Verify the result:

```console
$ ls -lh measurements.txt
-rw-r--r--@ 1 davidhruby  staff    13G Oct 16 00:01 measurements.txt

$ wc -l measurements.txt
 1000000000 measurements.txt
```

`ls -h` shows `13G` because it counts in binary units: 13,795,468,189 B ÷ 1024³ ≈ 12.85 GiB,
rounded up. `wc -l` reading the whole file took ~11 s. That gives a rough reference for how
long it takes just to read the file and scan it for newlines, before any parsing.

Rename it so the next run doesn't overwrite it:

```sh
mv measurements.txt 1B_measurements.txt
```

The smaller files work the same way:

```sh
./create_measurements.sh 1000000
mv measurements.txt 1M_measurements.txt

./create_measurements.sh 100000000
mv measurements.txt 100M_measurements.txt
```

`create_measurements.sh` is a thin wrapper around:

```sh
java --class-path target/average-1.0.0-SNAPSHOT.jar dev.morling.onebrc.CreateMeasurements <rows>
```

The rows are random, so each generated file has different content. When you compare
outputs between implementations, always run both on the **same** file.

## 4. Use it from this repo

Move the generated files into `input/` in the `brrr` repo:

```sh
mv path/to/1brc/100M_measurements.txt path/to/1brc/1B_measurements.txt path/to/brrr/input/
```

A move within the same disk is instant, because the file is not copied.

`input/100M_measurements.txt` and `input/1B_measurements.txt` are listed in `.gitignore`, so
they are never committed.
