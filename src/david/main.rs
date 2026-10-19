use std::{collections::BTreeMap, env, fs::read_to_string};

struct MeasurementStats {
    min: f32,
    max: f32,
    sum: f32,
    count: u32
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let input_file_path = &args[1];

    let mut location_to_measurement: BTreeMap<String, MeasurementStats> = BTreeMap::new();
    for line in read_to_string(input_file_path).unwrap().lines() {
        let split = line.split(';').collect::<Vec<&str>>();
        if split.len() == 2 {
            let location = split[0];
            let measurement = &split[1].parse::<f32>().unwrap();

            if location_to_measurement.contains_key(location) {
                let measurement_stats = location_to_measurement.get_mut(location).unwrap();
                if measurement_stats.min > *measurement {
                    measurement_stats.min = *measurement;
                }
                if measurement_stats.max < *measurement {
                    measurement_stats.max = *measurement;
                }
                measurement_stats.sum += measurement;
                measurement_stats.count += 1;
            } else {
                location_to_measurement.insert(location.to_string(), MeasurementStats {
                    min: *measurement,
                    max: *measurement,
                    sum: *measurement,
                    count: 1
                });
            }
        }
    }

    for (location, stats) in location_to_measurement.iter() {
        println!("{}={1:.1}/{2:.1}/{3:.1}", location, stats.min, stats.sum / stats.count as f32, stats.max);
    }
}
