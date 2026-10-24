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

    let mut location_to_measurement: BTreeMap<String, MeasurementStats> = BTreeMap::new();
    for line in reader.lines() {
        let line = line.unwrap();
        let (location, measurement_str) = line.split_once(';').unwrap();
        let measurement = measurement_str.parse::<f32>().unwrap();

        location_to_measurement.entry(location.to_string())
            .and_modify(|m| m.update(measurement))
            .or_insert_with(|| MeasurementStats::new(measurement));
    }

    for (location, MeasurementStats { min, max, sum, count}) in location_to_measurement.iter() {
        let mean = sum / (*count as f32);
        println!("{location}={min:.1}/{mean:.1}/{max:.1}");
    }
}
