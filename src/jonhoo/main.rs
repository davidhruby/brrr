use std::{collections::BTreeMap, env, fs::File, io::{BufRead, BufReader}};

fn main() {
    let args: Vec<String> = env::args().collect();
    let input_file_path = &args[1];

    let f = File::open(input_file_path).unwrap();
    let f = BufReader::new(f);
    let mut stats = BTreeMap::<String, (f64, f64, usize, f64)>::new();
    for line in f.lines() {
        let line = line.unwrap();
        let (station, temperature) = line.split_once(';').unwrap();
        let temperature: f64 = temperature.parse().unwrap();
        let stats = stats
            .entry(station.to_string())
            .or_insert((f64::MAX, 0., 0, f64::MIN));
        stats.0 = stats.0.min(temperature);
        stats.1 += temperature;
        stats.2 += 1;
        stats.3 = stats.3.max(temperature);
    }
    let mut stats = stats.into_iter().peekable();
    while let Some((station, (min, sum, count, max))) = stats.next() {
        println!("{station}={min:.1}/{:.1}/{max:.1}", sum / (count as f64));
    }
}
