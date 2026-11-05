use std::hint::black_box;
use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use brrr::{next_line_jon_version, next_line_swar};

fn bench_parse(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse_newline");

    let data = std::fs::read("input/1M_measurements.txt").unwrap();
    let lines = data.iter().filter(|&&b| b == b'\n').count();
    group.throughput(Throughput::Elements(lines as u64));
    group.bench_function("1M_measurements_next_line_memchr", |b| {
        b.iter(|| {
            let mut at = 0;
            while let Some(line) = next_line_jon_version(data.as_slice(), &mut at) {
                black_box(line);
            }
        })
    });
    group.bench_function("1M_measurements_next_line_split", |b| {
        b.iter(|| {
            for line in data.split(|c| *c == b'\n') {
                black_box(line);
            }
        })
    });
    group.bench_function("1M_measurements_next_line_swar", |b| {
        b.iter(|| {
            let mut at = 0;
            while let Some(line) = next_line_swar(data.as_slice(), &mut at) {
                black_box(line);
            }
        })
    });

    group.finish();
}

criterion_group!(benches, bench_parse);
criterion_main!(benches);