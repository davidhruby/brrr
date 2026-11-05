use std::hint::black_box;
use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use brrr::{next_station_and_temperature_2_fns, next_station_and_temperature_merged_simd_16, next_station_and_temperature_merged_simd_32, next_station_and_temperature_merged_simd_64, next_stations_and_temperatures_merged_simd_64};
use brrr::app::StationTemperatureIterator;

fn bench_parse(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse_newline");

    let data = std::fs::read("input/1M_measurements.txt").unwrap();
    let lines_count = data.iter().filter(|&&b| b == b'\n').count();
    group.throughput(Throughput::Elements(lines_count as u64));
    group.bench_function("1M_measurements_next_station_and_temperature", |b| {
        b.iter(|| {
            let mut at = 0;
            while let Some((station, temperature)) = next_station_and_temperature_2_fns(data.as_slice(), &mut at) {
                black_box(station);
                black_box(temperature);
            }
        })
    });
    group.bench_function("1M_measurements_next_station_and_temperature_merged_simd_16", |b| {
        b.iter(|| {
            let mut at = 0;
            while let Some((station, temperature)) = next_station_and_temperature_merged_simd_16(data.as_slice(), &mut at) {
                black_box(station);
                black_box(temperature);
            }
        })
    });
    group.bench_function("1M_measurements_next_station_and_temperature_merged_simd_32", |b| {
        b.iter(|| {
            let mut at = 0;
            while let Some((station, temperature)) = next_station_and_temperature_merged_simd_32(data.as_slice(), &mut at) {
                black_box(station);
                black_box(temperature);
            }
        })
    });
    group.bench_function("1M_measurements_next_station_and_temperature_merged_simd_64", |b| {
        b.iter(|| {
            let mut at = 0;
            while let Some((station, temperature)) = next_station_and_temperature_merged_simd_64(data.as_slice(), &mut at) {
                black_box(station);
                black_box(temperature);
            }
        })
    });
    group.bench_function("1M_measurements_next_stations_and_temperatures_merged_simd_64", |b| {
        b.iter(|| {
            let mut at = 0;
            let mut processed = 0;
            loop {
                let helper = black_box(next_stations_and_temperatures_merged_simd_64(data.as_slice(), &mut at));
                processed += helper.first.is_some() as usize
                    + helper.second.is_some() as usize
                    + helper.third.is_some() as usize;
                if helper.first.is_none() && helper.second.is_none() && helper.third.is_none() {
                    break;
                }
            }
            assert_eq!(processed, lines_count, "next_stations_and_temperatures_merged_simd_64 skipped lines");
        })
    });
    group.bench_function("1M_measurements_next_station_and_temperature_iterator", |b| {
        b.iter(|| {
            let mut processed = 0;

            for (station, temperature) in StationTemperatureIterator::new(data.as_slice()) {
                processed += 1;
                black_box(station);
                black_box(temperature);
            }
            assert_eq!(processed, lines_count, "next_stations_and_temperatures_merged_simd_64 skipped lines");
        })
    });

    group.finish();
}

criterion_group!(benches, bench_parse);
criterion_main!(benches);