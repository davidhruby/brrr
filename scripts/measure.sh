#!/usr/bin/env bash
# Runs one binary once and records what happens during the run, second by second,
# using standard macOS tools. Every tool writes its raw output into one folder.
#
# Usage:   scripts/measure.sh <binary> <size> <part> [e-cores]
# Example: scripts/measure.sh david 100M part001
#          scripts/measure.sh david 100M part001 e-cores
#
# With "e-cores", the program runs under `taskpolicy -c background`, which keeps it on the
# efficiency cores. The results then go into folders and files ending in _e-cores.
#
# 1M and 100M are read from the RAM disk (/Volumes/RAMDisk).
# 1B doesn't fit there, so it's read from the SSD.
# Every run starts with an empty file cache (sudo purge), so repeated runs are comparable.

bin=$1     # david or jonhoo
size=$2    # 1M, 100M or 1B
part=$3    # part001, part002, ...
cores=$4   # empty, or e-cores

run=./target/release/$bin
suffix=
if [ "$cores" = e-cores ]; then
    run="taskpolicy -c background ./target/release/$bin"
    suffix=_e-cores
fi

if [ "$size" = 1B ]; then
    input=input/1B_measurements.txt
else
    input=/Volumes/RAMDisk/${size}_measurements.txt
fi
out=measurements/$part/${size}_${bin}$suffix
mkdir -p "$out" "output/$part"

# 1. Ask for the password now, not in the middle of the run.
sudo -v

# 2. Empty the file cache, so every run starts from the same state:
#    1B comes from the SSD, 1M and 100M only from the RAM disk.
sudo purge

# 3. Memory and swap before the run.
{ date; memory_pressure | tail -n 1; sysctl vm.swapusage; vm_stat; } > "$out/memory_before_run.txt"

# 4. Start the system-wide samplers in the background, one sample per second.
vm_stat 1 > "$out/vm_stat.txt" &                  # memory, compression, swap
vm_pid=$!
iostat -w 1 disk0 > "$out/iostat.txt" &           # SSD reads and writes
io_pid=$!
> "$out/powermetrics.txt"          # empty the file first, in case powermetrics appends to it
sudo powermetrics --samplers cpu_power -i 1000 -o "$out/powermetrics.txt" > /dev/null &   # P/E clusters
sleep 1

# 5. Start the program.
echo "Running $run $input"
/usr/bin/time -l $run "$input" > "output/$part/${size}_${bin}_output$suffix.txt" 2> "$out/time.txt" &
time_pid=$!

# 6. As soon as the program is running, watch it with top.
until pgrep -x "$bin" > /dev/null; do sleep 0.01; done
top -l 0 -s 1 -pid "$(pgrep -n -x "$bin")" -stats pid,cpu,mem,cmprs,faults,pageins,csw,threads,state > "$out/top.txt" &
top_pid=$!

# 7. Wait for the program to finish, then stop the samplers.
wait $time_pid
sleep 1
kill $vm_pid $io_pid $top_pid
wait $vm_pid $io_pid $top_pid 2> /dev/null
sudo pkill -x powermetrics

# 8. Memory and swap after the run.
{ date; memory_pressure | tail -n 1; sysctl vm.swapusage; vm_stat; } > "$out/memory_after_run.txt"

echo "Done. Measurements are in $out/"
head -n 1 "$out/time.txt"
