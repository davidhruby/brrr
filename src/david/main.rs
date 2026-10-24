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
            .and_modify(| ltm| {
                ltm.min = ltm.min.min(measurement);
                ltm.max = ltm.max.max(measurement);
                ltm.sum += measurement;
                ltm.count += 1;
            }).or_insert(MeasurementStats {
            min: measurement,
            max: measurement,
            sum: measurement,
            count: 1
        });
    }

    for (location, stats) in location_to_measurement.iter() {
        println!("{}={1:.1}/{2:.1}/{3:.1}", location, stats.min, stats.sum / stats.count as f32, stats.max);
    }
}
