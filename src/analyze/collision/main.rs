use std::collections::HashMap;
use std::fs::read_to_string;
use std::hash::{BuildHasher, DefaultHasher, RandomState};
use brrr::app::StationTemperatureIterator;
use brrr::FastHasherBuilder;

struct HashInfo {
    station: String,
    hash: u64,
    count: u64,
}
const TOTAL_LINES: u64 = 1_000_000_000;

fn main() {
    // let file = File::open("input/1B_measurements.txt").unwrap();
    // let file_len: usize = file.metadata().unwrap().len() as usize;
    // let m_m_helper = MMapHelper::new(&file, file_len);
    // let map = m_m_helper.as_slice();
    //
    // process_data(map)
    analyse_fast_hash_map_collisions();
}

#[allow(dead_code)]
fn process_data(map: &[u8]) {
    let mut processed_lines = 0;
    let mut hash_station_infos: HashMap<u64, HashMap<String, HashInfo>> = HashMap::new();
    // FIXME tohle je problem. cely vektor zabere strasne mista. je potreba delat spodni cast postupne misto pouzivani cele pameti
    let hasher = RandomState::new();
    for (station_bytes, _) in StationTemperatureIterator::new(map) {
        let station_hash = hasher.hash_one(&station_bytes);
        let station_name = str::from_utf8(station_bytes).unwrap().to_string();
        hash_station_infos.entry(station_hash)
            .and_modify(|existing_station| {
                existing_station.entry(station_name.to_string())
                    .and_modify(|existing_station| {
                        existing_station.count += 1;
                    })
                    .or_insert_with(|| {
                        HashInfo {
                            station: station_name.to_string(),
                            hash: station_hash,
                            count: 1,
                        }
                    });
            })
            .or_insert_with(|| {
                let mut map = HashMap::new();
                map.insert(station_name.to_string(), HashInfo {
                    station: station_name.to_string(),
                    hash: station_hash,
                    count: 1,
                });
                map
            });
        processed_lines += 1;
        if processed_lines & 1_048_575 == 0 {
            println!("processed lines: {}/{TOTAL_LINES}", processed_lines);
        }
    }

    println!("processed lines: {}", processed_lines);
    println!("hash stations: {}", hash_station_infos.keys().len());

    let mut hash_info_vec = hash_station_infos.values()
        .filter(|&hash_info| hash_info.len() > 1)
        .collect::<Vec<&HashMap<String, HashInfo>>>();
    if hash_info_vec.is_empty() {
        println!("no collision data found");
    } else {
        hash_info_vec.sort_by(|a, b| a.len().cmp(&b.len()));
        for hash_infos in hash_info_vec {
            println!("--- Group ---");
            println!("size: {}", hash_infos.len());
            for hash_info in hash_infos.values() {
                println!("station: {}", hash_info.station);
                println!("count: {}", hash_info.count);
                println!("hash: {}", hash_info.hash);
            }
            // println!("distinct hashes:  {}", distinct(&|h| h));
            // println!("distinct slots:   {}", distinct(&|h| h & (slots - 1)));
            // println!("distinct tags:    {} of 128", distinct(&|h| h >> 57));
        }
    }
}

#[allow(dead_code)]
fn analyse_hash_map_collisions() {
    println!("analyse_hash_map_collisions - BuildHasherDefault::<DefaultHasher>::default()");
    let data = read_to_string(&"input/1B_unique_station_names.txt").unwrap();
    let split = data.split("\n");

    let hasher = std::hash::BuildHasherDefault::<DefaultHasher>::default();
    for i in std::iter::successors(Some(128u32), |&x| x.checked_mul(2)).take_while(|&x| x <= 10_000_000) {
        let max_buckets = i / 2;
        println!("hash map size: {} so buckets count: {}", i, max_buckets);

        let mut bucket_to_station: HashMap<u64, Vec<String>> = HashMap::new();
        let mut tag_to_station: HashMap<u64, Vec<String>> = HashMap::new();
        let mut bucket_and_tag_to_station: HashMap<(u64, u64), Vec<String>> = HashMap::new();
        for station_name in split.clone() {
            if station_name.is_empty() {
                continue;
            }

            let hash = hasher.hash_one(&station_name.as_bytes());

            let bucket = hash & (max_buckets - 1) as u64;
            bucket_to_station.entry(bucket)
                .and_modify(|bucket| bucket.push(station_name.to_string()))
                .or_insert_with(|| {
                    let mut stations = Vec::new();
                    stations.push(station_name.to_string());
                    stations
                });

            let tag = hash >> 57;
            tag_to_station.entry(tag)
                .and_modify(|tag| tag.push(station_name.to_string()))
                .or_insert_with(|| {
                    let mut stations = Vec::new();
                    stations.push(station_name.to_string());
                    stations
                });

            bucket_and_tag_to_station.entry((bucket, tag))
                .and_modify(|bucket_and_tag| bucket_and_tag.push(station_name.to_string()))
                .or_insert_with(|| {
                    let mut stations = Vec::new();
                    stations.push(station_name.to_string());
                    stations
                });
        }

        let bucket_to_station_with_collision = bucket_to_station.iter()
            .filter(|(_, v)| v.len() > 1)
            .collect::<Vec<(&u64, &Vec<String>)>>();
        println!("bucket collisions: {}", bucket_to_station_with_collision.len());

        let tag_to_station_with_collision = tag_to_station.iter()
            .filter(|(_, v)| v.len() > 1)
            .collect::<Vec<(&u64, &Vec<String>)>>();
        println!("tag collisions: {}", tag_to_station_with_collision.len());

        let bucket_and_tag_to_station_with_collision = bucket_and_tag_to_station.iter()
            .filter(|(_, v)| v.len() > 1)
            .collect::<Vec<(&(u64, u64), &Vec<String>)>>();
        println!("bucket+tag collisions: {}", bucket_and_tag_to_station_with_collision.len());
    }
}

fn analyse_fast_hash_map_collisions() {
    println!("analyse_hash_map_collisions - FastHasherBuilder");
    let data = read_to_string(&"input/1B_unique_station_names.txt").unwrap();
    let split = data.split("\n");

    for i in std::iter::successors(Some(128u32), |&x| x.checked_mul(2)).take_while(|&x| x <= 10_000_000) {
        let max_buckets = i / 2;
        println!("hash map size: {} so buckets count: {}", i, max_buckets);

        let fast_hasher = FastHasherBuilder;

        let mut bucket_to_station: HashMap<u64, Vec<String>> = HashMap::new();
        let mut tag_to_station: HashMap<u64, Vec<String>> = HashMap::new();
        let mut bucket_and_tag_to_station: HashMap<(u64, u64), Vec<String>> = HashMap::new();
        for station_name in split.clone() {
            if station_name.is_empty() {
                continue;
            }

            let hash = fast_hasher.hash_one(&station_name.as_bytes());

            let bucket = hash & (max_buckets - 1) as u64;
            bucket_to_station.entry(bucket)
                .and_modify(|bucket| bucket.push(station_name.to_string()))
                .or_insert_with(|| {
                    let mut stations = Vec::new();
                    stations.push(station_name.to_string());
                    stations
                });

            let tag = hash >> 57;
            tag_to_station.entry(tag)
                .and_modify(|tag| tag.push(station_name.to_string()))
                .or_insert_with(|| {
                    let mut stations = Vec::new();
                    stations.push(station_name.to_string());
                    stations
                });

            bucket_and_tag_to_station.entry((bucket, tag))
                .and_modify(|bucket_and_tag| bucket_and_tag.push(station_name.to_string()))
                .or_insert_with(|| {
                    let mut stations = Vec::new();
                    stations.push(station_name.to_string());
                    stations
                });
        }

        let bucket_to_station_with_collision = bucket_to_station.iter()
            .filter(|(_, v)| v.len() > 1)
            .collect::<Vec<(&u64, &Vec<String>)>>();
        println!("bucket collisions: {}", bucket_to_station_with_collision.len());

        let tag_to_station_with_collision = tag_to_station.iter()
            .filter(|(_, v)| v.len() > 1)
            .collect::<Vec<(&u64, &Vec<String>)>>();
        println!("tag collisions: {}", tag_to_station_with_collision.len());

        let bucket_and_tag_to_station_with_collision = bucket_and_tag_to_station.iter()
            .filter(|(_, v)| v.len() > 1)
            .collect::<Vec<(&(u64, u64), &Vec<String>)>>();
        println!("bucket+tag collisions: {}", bucket_and_tag_to_station_with_collision.len());
    }
}

#[cfg(test)]
mod tests {
    use std::hash::RandomState;
    use crate::process_data;

    #[test]
    fn process_data_test() {
        let data = "Chihuahua;16.7\nChihuahua;2.7\n";
        process_data(data.as_bytes());
    }
}