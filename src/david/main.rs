use std::env;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufRead, BufReader};

struct MeasurementStats {
    min: f32,
    max: f32,
    sum: f32,
    count: u32
}

impl MeasurementStats {
    fn new(measurement: f32) -> Self {
        Self {
            min: measurement,
            max: measurement,
            sum: measurement,
            count: 1
        }
    }

    fn update(&mut self, measurement: f32) {
        self.min = self.min.min(measurement);
        self.max = self.max.max(measurement);
        self.sum += measurement;
        self.count += 1;
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let input_file_path = &args[1];

    let file = File::open(input_file_path).unwrap();
    let reader = BufReader::new(file);

    let mut stats: BTreeMap<String, MeasurementStats> = BTreeMap::new();
    for line in reader.lines() {
        let line = line.unwrap();
        let (station, temperature) = line.split_once(';').unwrap();
        let temperature = temperature.parse::<f32>().unwrap();

        stats.entry(station.to_string())
            .and_modify(|m| m.update(temperature))
            .or_insert_with(|| MeasurementStats::new(temperature));
    }

    for (location, MeasurementStats { min, max, sum, count}) in stats.iter() {
        let mean = sum / (*count as f32);
        println!("{location}={min:.1}/{mean:.1}/{max:.1}");
    }
}
