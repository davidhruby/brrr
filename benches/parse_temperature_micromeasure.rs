mod common;

use micromeasure::{benchmark_main, black_box, BenchContext, Throughput};
use brrr::app::parse_temperature;
use brrr::{parse_f32, parse_temperature_jon_version};

struct ParserContext {
    data: Vec<Vec<u8>>
}

impl BenchContext for ParserContext {
    fn prepare(_chunk_size: usize) -> Self {
        Self {
            data: common::generate_inputs(10_000)
        }
    }
}


fn bench_parse_f32(ctx: &mut ParserContext, chunk_size: usize, _chunk_num: usize) {
    let mut acc = black_box(0.0f32);
    for _ in 0..chunk_size {
        for number in &ctx.data {
            acc += parse_f32(black_box(number).as_slice());
        }
    }
    black_box(acc);
}

fn bench_parse_temperature_jon_version(ctx: &mut ParserContext, chunk_size: usize, _chunk_num: usize) {
    let mut checksum = black_box(0_i64);
    for _ in 0..chunk_size {
        for number in &ctx.data {
            checksum = checksum.wrapping_add(parse_temperature_jon_version(black_box(number).as_slice()) as i64);
        }
    }
    black_box(checksum);
}

fn bench_parse_temperature(ctx: &mut ParserContext, chunk_size: usize, _chunk_num: usize) {
    let mut checksum = black_box(0_i64);
    for _ in 0..chunk_size {
        for number in &ctx.data {
            checksum = checksum.wrapping_add(parse_temperature(black_box(number).as_slice()) as i64);
        }
    }
    black_box(checksum);
}

benchmark_main!(|runner| {
    runner.group::<ParserContext>("parse", |g| {
        g.throughput(Throughput::per_operation(10_000, "temperatures"))
            .bench("bench_parse_f32", bench_parse_f32);
        g.throughput(Throughput::per_operation(10_000, "temperatures"))
            .bench("bench_parse_temperature_jon_version", bench_parse_temperature_jon_version);
        g.throughput(Throughput::per_operation(10_000, "temperatures"))
            .bench("bench_parse_temperature", bench_parse_temperature);
    });
});