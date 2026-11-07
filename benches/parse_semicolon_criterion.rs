use std::hint::black_box;
use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use brrr::{split_semicolon, split_semicolon_simd, split_semicolon_simd_copy, split_semicolon_simd_from_data};

fn bench_parse(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse_semicolon");

    let data = std::fs::read("input/1M_measurements.txt").unwrap();

    // (start of the line in data, the line), so that variant C can load from data
    let mut lines: Vec<(usize, &[u8])> = Vec::new();
    let mut start = 0;
    for line in data.split(|c| *c == b'\n') {
        if !line.is_empty() {
            lines.push((start, line));
        }
        start += line.len() + 1;
    }

    // all variants must give the same result as rsplitn, checked once, outside of the measurement
    for &(start, line) in &lines {
        let expected = split_semicolon(line);
        assert_eq!(split_semicolon_simd(line), expected);
        assert_eq!(split_semicolon_simd_copy(line), expected);
        assert_eq!(split_semicolon_simd_from_data(&data, start, line), expected);
    }

    group.throughput(Throughput::Elements(lines.len() as u64));
    group.bench_function("1M_measurements_split_semicolon_rsplitn", |b| {
        b.iter(|| {
            for &(_, line) in &lines {
                black_box(split_semicolon(line));
            }
        })
    });
    group.bench_function("1M_measurements_split_semicolon_simd_load_or_default", |b| {
        b.iter(|| {
            for &(_, line) in &lines {
                black_box(split_semicolon_simd(line));
            }
        })
    });
    group.bench_function("1M_measurements_split_semicolon_simd_copy", |b| {
        b.iter(|| {
            for &(_, line) in &lines {
                black_box(split_semicolon_simd_copy(line));
            }
        })
    });
    group.bench_function("1M_measurements_split_semicolon_simd_from_data", |b| {
        b.iter(|| {
            for &(start, line) in &lines {
                black_box(split_semicolon_simd_from_data(&data, start, line));
            }
        })
    });

    group.finish();
}

criterion_group!(benches, bench_parse);
criterion_main!(benches);