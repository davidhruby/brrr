pub fn generate_inputs(n: usize) -> Vec<Vec<u8>> {
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