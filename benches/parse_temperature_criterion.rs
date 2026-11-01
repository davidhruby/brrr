mod common;

use brrr::app::parse_temperature;
use brrr::{parse_f32, parse_temperature_jon_version};
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::hint::black_box;
use crate::common::generate_inputs;

fn bench_parse(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse");

    for s in ["0.0", "9.9", "-9.9", "99.9", "-99.9"] {
        group.bench_with_input(BenchmarkId::new("single_parse_temperature", s), s.as_bytes(), |b, input| {
            b.iter(|| parse_temperature(black_box(input)))
        });
    }
    for s in ["0.0", "9.9", "-9.9", "99.9", "-99.9"] {
        group.bench_with_input(BenchmarkId::new("single_parse_f32", s), s.as_bytes(), |b, input| {
            b.iter(|| parse_f32(black_box(input)))
        });
    }
    for s in ["0.0", "9.9", "-9.9", "99.9", "-99.9"] {
        group.bench_with_input(BenchmarkId::new("single_parse_parse_temperature_jon_version", s), s.as_bytes(), |b, input| {
            b.iter(|| parse_temperature_jon_version(black_box(input)))
        });
    }

    let inputs = generate_inputs(10_000);
    group.throughput(Throughput::Elements(inputs.len() as u64));
    group.bench_function("random_10k_parse_temperature", |b| {
        b.iter(|| {
            let mut sum: i32 = 0;
            for t in &inputs {
                sum += parse_temperature(black_box(t)) as i32;
            }
            sum
        })
    });
    group.bench_function("random_10k_parse_parse_temperature_jon_version", |b| {
        b.iter(|| {
            let mut sum: i32 = 0;
            for t in &inputs {
                sum += parse_temperature_jon_version(black_box(t)) as i32;
            }
            sum
        })
    });
    group.bench_function("random_10k_parse_f32", |b| {
        b.iter(|| {
            let mut sum: f32 = 0.;
            for t in &inputs {
                sum += parse_f32(black_box(t));
            }
            sum
        })
    });

    group.finish();
}

criterion_group!(benches, bench_parse);
criterion_main!(benches);