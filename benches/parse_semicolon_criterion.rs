use std::hint::black_box;
use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use brrr::{split_semicolon, split_semicolon_simd};

fn bench_parse(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse_semicolon");

    let data = std::fs::read("input/1M_measurements.txt").unwrap();
    let lines = data.split(|c| *c == b'\n').filter(|line| !line.is_empty()).collect::<Vec<_>>();
    group.throughput(Throughput::Elements(lines.len() as u64));
    group.bench_function("1M_measurements_split_semicolon_rsplitn", |b| {
        b.iter(|| {
            for line in &lines {
                black_box(split_semicolon(line));
            }
        })
    });
    group.bench_function("1M_measurements_split_semicolon_simd", |b| {
        b.iter(|| {
            for line in &lines {
                black_box(split_semicolon_simd(line));
            }
        })
    });

    group.finish();
}

criterion_group!(benches, bench_parse);
criterion_main!(benches);