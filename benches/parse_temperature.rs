use brrr::app::parse_temperature;
use brrr::{parse_f32, parse_temperature_v2};
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::hint::black_box;

fn generate_inputs(n: usize) -> Vec<Vec<u8>> {
    let mut state: u64 = 42;
    (0..n)
        .map(|_| {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let v = ((state >> 33) % 1999) as i32 - 999;
            let sign = if v < 0 { "-" } else { "" };
            format!("{sign}{}.{}", v.abs() / 10, v.abs() % 10).into_bytes()
        })
        .collect()
}

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
        group.bench_with_input(BenchmarkId::new("single_parse_temperature_v2", s), s.as_bytes(), |b, input| {
            b.iter(|| parse_temperature_v2(black_box(input)))
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
    group.bench_function("random_10k_parse_temperature_v2", |b| {
        b.iter(|| {
            let mut sum: i32 = 0;
            for t in &inputs {
                sum += parse_temperature_v2(black_box(t)) as i32;
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