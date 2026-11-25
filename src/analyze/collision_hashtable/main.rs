use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::hint::black_box;
use std::io::BufRead;
use std::os::fd::AsRawFd;
use std::str::from_utf8;
use brrr::app::{StationTemperatureIterator, StationTable, key_words, StationName, hash_analyse, SLOTS, SLOT_BITS};

fn main() {
    find_first_word_constant();
}

// the hash from the first 8 bytes of the name and its length: the first 8 bytes are unique for
// 412 of the 413 names, and the two that share them (Alexandra, Alexandria) differ in length
fn hash_first_word(word_1: u64, len: usize, multiplier: u64) -> usize {
    ((word_1 ^ len as u64).wrapping_mul(multiplier) as usize) >> (64 - SLOT_BITS)
}

// the first odd multiplier from splitmix64 with no collision of hash_first_word on the 413 names
#[allow(dead_code)]
fn find_first_word_constant() {
    let file = File::open("input/1B_unique_station_names.txt").unwrap();
    let mut keys = Vec::new();
    for line in std::io::BufReader::new(file).lines() {
        let line = line.unwrap();
        let station_name = StationName {
            rest: line.as_bytes(),
            len: line.len(),
        };
        let (word_1, _) = key_words(&station_name);
        keys.push((word_1, line.len()));
    }

    let mut state: u64 = 0;
    for candidate in 0..100_000 {
        let multiplier = splitmix64(&mut state) | 1;
        let mut taken = vec![false; SLOTS];
        let without_collision = keys.iter().all(|(word_1, len)| {
            let index = hash_first_word(*word_1, *len, multiplier);
            !std::mem::replace(&mut taken[index], true)
        });
        if without_collision {
            println!("candidate {}: multiplier {:#018X} has no collision", candidate, multiplier);
            return;
        }
    }
    println!("no multiplier without a collision in 100,000 candidates");
}

#[allow(dead_code)]
fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

#[allow(dead_code)]
fn check_zero_colisions() {
    let file = File::open("input/1B_unique_station_names.txt").unwrap();
    let mut keywords = Vec::new();
    for line in std::io::BufReader::new(file).lines() {
        let line = line.unwrap();
        let station_name = StationName {
            rest: line.as_bytes(),
            len: line.len(),
        };
        let (word_1, word_2) = key_words(&station_name);
        keywords.push((word_1, word_2));
    }

    let mut existing_stations = HashSet::new();
    let mut total_collisions: usize = 0;
    for (word_1, word_2) in &keywords {
        let index = hash_analyse(*word_1, *word_2, 7, 0xA903_8A92_1825_F10D);
        if !existing_stations.insert(index) {
            total_collisions += 1;
        }
    }

    println!("Total collisions: {}", total_collisions);
}

#[allow(dead_code)]
fn find_collisions() {
    let file = File::open("input/1B_unique_station_names.txt").unwrap();
    let mut keywords = Vec::new();
    for line in std::io::BufReader::new(file).lines() {
        let line = line.unwrap();
        let station_name = StationName {
            rest: line.as_bytes(),
            len: line.len(),
        };
        let (word_1, word_2) = key_words(&station_name);
        keywords.push((word_1, word_2));
    }

    let mut rotate_to_total_collision: Vec<(u32, usize)> = vec![];
    for rotate in 0..64 {
        let mut existing_stations = HashSet::new();
        let mut total_collisions: usize = 0;
        for (word_1, word_2) in &keywords {
            let index = hash_analyse(*word_1, *word_2, rotate, 0x243F_6A88_85A3_08D3);
            if !existing_stations.insert(index) {
                total_collisions += 1;
            }
        }
        rotate_to_total_collision.push((rotate, total_collisions));
    }

    rotate_to_total_collision.sort_by(|a, b| a.1.cmp(&b.1));

    for (rotate, total_collisions) in rotate_to_total_collision.iter() {
        println!("Total collisions: {} for rotate: {}", total_collisions, rotate);
    }

    let top_ten_rotate = rotate_to_total_collision.iter().map(|a| a.0)
        .collect::<Vec<u32>>()
        .first_chunk::<10>().unwrap()
        .to_vec();

    let mut state: u64 = 0;
    for _ in 0..10_000 {
        let multiplier = splitmix64(&mut state) | 1;
        for rotate in top_ten_rotate.iter() {
            let mut existing_stations = HashSet::new();
            let mut total_collisions: usize = 0;
            for (word_1, word_2) in &keywords {
                let index = hash_analyse(*word_1, *word_2, *rotate, multiplier);
                if !existing_stations.insert(index) {
                    total_collisions += 1;
                }
            }
            if total_collisions == 0 {
                println!("Total collisions: {} for rotate: {} and multiplier 0x{:x}", total_collisions, *rotate, multiplier);
            }
        }
    }
}

#[allow(dead_code)]
fn calculate_the_extra_effort() {
    let file = File::open("input/1B_measurements.txt").unwrap();
    let file_len: usize = file.metadata().unwrap().len() as usize;

    let m_map_ptr = unsafe {
        let ptr = libc::mmap(
            std::ptr::null_mut(),
            file_len as libc::size_t,
            libc::PROT_READ,
            libc::MAP_SHARED,
            file.as_raw_fd(),
            0,
        );
        if ptr == libc::MAP_FAILED {
            panic!("mmap failed: {:?}", std::io::Error::last_os_error());
        } else {
            ptr
        }
    };
    if unsafe { libc::madvise(m_map_ptr, file_len as libc::size_t, libc::MADV_WILLNEED) } != 0 {
        panic!("madvise MADV_WILLNEED failed: {:?}", std::io::Error::last_os_error());
    }
    let m_map = unsafe { std::slice::from_raw_parts(m_map_ptr as *const u8, file_len) };

    let mut processed_count: usize = 0;
    let mut total_extra_loops: usize = 0;
    let mut collisions = HashMap::<&[u8], usize>::new();
    let mut stats = StationTable::new();
    for (station_name, _) in StationTemperatureIterator::new(m_map) {
        let (word_1, word_2) = key_words(&station_name);
        let index = hash_analyse(word_1, word_2, 7u32, 0xA903_8A92_1825_F10D);
        let (_, _final_index, extra_loop) = black_box(stats.get_mut_analyse(station_name, 7u32, 0xA903_8A92_1825_F10D));
        if stats.names[index] != station_name.as_bytes() {
            total_extra_loops += extra_loop;
            let _station_name_from_file_text = from_utf8(&station_name.rest[..station_name.len]).unwrap();
            let _station_name_from_table_text = from_utf8(&stats.names[index]).unwrap();
            collisions.entry(&station_name.rest[..station_name.len])
                .and_modify(|entry| {
                    // println!("Collision: {}={}", station_name_from_file_text, *entry);
                    *entry += 1
                }).or_insert(1);
        }

        processed_count += 1;
        if processed_count & ((1024 * 1024) - 1) == 0 {
            println!("Processed count {}. Actual total extra loops: {}", processed_count, total_extra_loops);
        }
    }

    for (station_name, collision_count) in collisions {
        println!("Collision {}={}:", from_utf8(station_name).unwrap(), collision_count);
    }
    println!("Total extra loops: {}", total_extra_loops);
}