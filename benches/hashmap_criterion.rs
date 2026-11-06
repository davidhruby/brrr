use std::collections::HashMap;
use std::hint::black_box;
use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use brrr::{hashmap_upsert, parse_temperature_jon_version, split_semicolon_simd, FastHasherBuilder, hashmap_upsert_custom_hash_builder, hashmap_upsert_ideal_hash_builder};
use brrr::app::{IdealHasherBuilder, MeasurementStats};

fn bench_parse(c: &mut Criterion) {
    let mut group = c.benchmark_group("hashmap");

    let data = std::fs::read("input/1M_measurements.txt").unwrap();
    let lines = data.split(|c| *c == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| split_semicolon_simd(line))
        .map(|(station, temperature)| (station, parse_temperature_jon_version(temperature)))
        .collect::<Vec<_>>();

    group.throughput(Throughput::Elements(lines.len() as u64));
    group.bench_function("1M_measurements_hashmap_upsert", |b| {
        b.iter(|| {
            let mut stats: HashMap<Vec<u8>, MeasurementStats> = HashMap::new();
            for (station, temperature) in lines.iter() {
                hashmap_upsert(&mut stats, station, *temperature);
            }
            black_box(stats);
        })
    });
    group.bench_function("1M_measurements_hashmap_with_capacity_10K_upsert", |b| {
        b.iter(|| {
            let mut stats: HashMap<Vec<u8>, MeasurementStats> = HashMap::with_capacity(10_000);
            for (station, temperature) in lines.iter() {
                hashmap_upsert(&mut stats, station, *temperature);
            }
            black_box(stats);
        })
    });
    group.bench_function("1M_measurements_hashmap_with_fasthasher_and_capacity_10K_upsert", |b| {
        b.iter(|| {
            let mut stats: HashMap<Vec<u8>, MeasurementStats, FastHasherBuilder> = HashMap::with_capacity_and_hasher(10_000, FastHasherBuilder);
            for (station, temperature) in &lines {
                hashmap_upsert_custom_hash_builder(&mut stats, station, *temperature);
            }
            black_box(stats);
        })
    });
    group.bench_function("1M_measurements_hashmap_with_ideal_hasher_upsert", |b| {
        b.iter(|| {
            let mut stats: HashMap<Vec<u8>, MeasurementStats, IdealHasherBuilder> = HashMap::with_hasher(IdealHasherBuilder);
            for (station, temperature) in lines.iter() {
                hashmap_upsert_ideal_hash_builder(&mut stats, station, *temperature);
            }
            black_box(stats);
        })
    });
    group.bench_function("1M_measurements_hashmap_with_ideal_hasher_capacity_10K_upsert", |b| {
        b.iter(|| {
            let mut stats: HashMap<Vec<u8>, MeasurementStats, IdealHasherBuilder> = HashMap::with_capacity_and_hasher(10_000, IdealHasherBuilder);
            for (station, temperature) in lines.iter() {
                hashmap_upsert_ideal_hash_builder(&mut stats, station, *temperature);
            }
            black_box(stats);
        })
    });

    group.finish();
}

criterion_group!(benches, bench_parse);
criterion_main!(benches);