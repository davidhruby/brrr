use std::hint::black_box;
use std::time::Instant;

struct MyInfo {
    _a: u64,
    _b: u32
}

#[repr(align(16384))]
struct MyInfoFixedAlign {
    _a: u64,
    _b: u32
}

#[repr(align(16384))]
struct MyInfoFixedAlignArray {
    a: [usize; 2048]
}

fn main() {
    println!("align_of::<u64>()={}", align_of::<u64>());

    println!("align_of::<MyInfo>()={}", align_of::<MyInfo>());
    println!("align_of::<MyInfoFixedAlign>()={}", align_of::<MyInfoFixedAlign>());

    println!("size_of::<MyInfo>()={}", size_of::<MyInfo>());
    println!("size_of::<MyInfoFixedAlign>()={}", size_of::<MyInfoFixedAlign>());

    let a = 1u64;
    let b = 1u32;
    println!("size_of_val(&a)={}", size_of_val(&a));
    println!("size_of_val(&b)={}", size_of_val(&b));

    let my_info = MyInfo { _a: 1, _b: 2 };
    println!("size_of_val(&my_info)={}", size_of_val(&my_info));

    let my_info_fixed_align = MyInfoFixedAlign { _a: 1, _b: 2 };
    println!("size_of_val(&my_info_fixed_align)={}", size_of_val(&my_info_fixed_align));

    //warmup
    for _ in 0..2 {
        measurement(&16_384, true);
    }

    // let scenarios = [8, 12, 16, 24, 32, 48, 64, 96, 128, 192, 256, 384, 512, 768, 1_024, 1_536, 2_048, 3_072, 4_096, 6_144, 8_192, 12_288, 16_384, 24_576, 32_768];
    // for n in scenarios.iter() {
    //     measurement(n, false);
    // }

    for i in 1..=1_000 {
        measurement(&(i * 8), false);
    }
}

fn measurement(n: &usize, is_warmup: bool) {
    let mut available_pages: Vec<usize> = (0..*n).collect();
    let mut rand_pages: Vec<usize> = Vec::with_capacity(*n);
    for i in 0..*n {
        let page_num = available_pages.swap_remove(rand::random_range(0..n - i));
        rand_pages.push(page_num);
    }

    // 1. create all pages, empty
    let mut pages = Vec::<MyInfoFixedAlignArray>::with_capacity(*n);
    for _ in 0..*n {
        pages.push(MyInfoFixedAlignArray { a: [0; 2048] });
    }

    // 2. chain: rand_pages[k] -> rand_pages[k + 1], the last one back to rand_pages[0]
    for k in 0..*n {
        let p = rand_pages[k];
        let q = rand_pages[(k + 1) % n];
        let offset = (p % 128) * 16;
        pages[p].a[offset] = q;
    }

    let mut next_page = 0;

    // let mut check = vec![false; *n];
    // for _ in 0..*n {
    //     check[next_page] = true;
    //     let offset = (next_page % 128) * 16;
    //     next_page = pages[next_page].a[offset];
    // }
    // let any_false = check.iter().any(|&b| !b);
    // println!("any_false={}", any_false);

    // warm up
    for _ in 0..2 {
        for _ in 0..16384 {
            let offset = (next_page % 128) * 16;
            next_page = pages[next_page].a[offset];
        }
    }

    let start = Instant::now();
    let steps = 100_000_000;
    for _ in 0..steps {
        let offset = (next_page % 128) * 16;
        next_page = pages[next_page].a[offset];
    }
    let duration = start.elapsed();

    println!("next_page={}", black_box(next_page));
    println!("is_warmup={}, n={}, duration={:.2} nanos per step", is_warmup, n, duration.as_nanos() as f64 / steps as f64);
}