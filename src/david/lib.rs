#![feature(portable_simd)]
#![feature(slice_split_once)]

#[allow(dead_code, unused_attributes)]
#[path = "main.rs"]
pub mod app;

use std::collections::HashMap;
use std::ffi::{c_int, c_void};
use std::hash::{BuildHasher, Hasher};
use std::iter;
use libc::size_t;
use std::simd::{u8x16, u8x32, u8x64};
use std::simd::prelude::SimdPartialEq;
use crate::app::{IdealHasherBuilder, MeasurementStats};

pub fn parse_f32(temperature: &[u8]) -> f32 {
    unsafe { str::from_utf8_unchecked(temperature).parse().unwrap() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_f32() {
        assert_eq!(parse_f32(b"99.9"), 99.9);
        assert_eq!(parse_f32(b"2.5"), 2.5);
        assert_eq!(parse_f32(b"0.1"), 0.1);
        assert_eq!(parse_f32(b"0.0"), 0.);
        assert_eq!(parse_f32(b"-0.1"), -0.1);
        assert_eq!(parse_f32(b"-2.5"), -2.5);
        assert_eq!(parse_f32(b"-99.9"), -99.9);
    }

    #[test]
    fn test_parse_temperature_v2() {
        assert_eq!(parse_temperature_jon_version(b"99.9"), 999);
        assert_eq!(parse_temperature_jon_version(b"2.5"), 25);
        assert_eq!(parse_temperature_jon_version(b"0.1"), 1);
        assert_eq!(parse_temperature_jon_version(b"0.0"), 0);
        assert_eq!(parse_temperature_jon_version(b"-0.1"), -1);
        assert_eq!(parse_temperature_jon_version(b"-2.5"), -25);
        assert_eq!(parse_temperature_jon_version(b"-99.9"), -999);
    }
}

pub fn parse_temperature_jon_version(temperature: &[u8]) -> i16 {
    let mut t: i16 = 0;
    let mut mul = 1;
    for &d in temperature.iter().rev() {
        match d {
            b'.' => {
                continue;
            }
            b'-' => {
                t = -t;
                break;
            }
            _ => {
                t += i16::from(d - b'0') * mul;
                mul *= 10;
            }
        }
    }
    t
}

// SWAR: tests 8 bytes at once for '\n'
fn find_new_line(bytes: &[u8]) -> Option<usize> {
    const NEW_LINES: u64 = u64::from_ne_bytes([b'\n'; 8]);
    const ONES: u64 = 0x0101_0101_0101_0101;
    const HIGH_BITS: u64 = 0x8080_8080_8080_8080;

    let mut i = 0;
    while i + 8 <= bytes.len() {
        // first byte in memory is the lowest byte of the number
        let chunk = u64::from_le_bytes(bytes[i..i + 8].try_into().unwrap());
        // bytes that were '\n' are 0 now
        let x = chunk ^ NEW_LINES;
        // highest bit set in every byte that is 0
        let mask = x.wrapping_sub(ONES) & !x & HIGH_BITS;
        if mask != 0 {
            // the lowest set bit is the first '\n'
            return Some(i + (mask.trailing_zeros() / 8) as usize);
        }
        i += 8;
    }
    // fewer than 8 bytes left, only at the end of the file
    bytes[i..].iter().position(|&b| b == b'\n').map(|position| i + position)
}

pub fn next_line_swar<'a>(map: &'a [u8], at: &mut usize) -> Option<&'a [u8]> {
    let rest = &map[*at..];
    let line_len = find_new_line(rest)?;
    // line_len is count without \n therefore +1 in the end
    *at += line_len + 1;
    Some(&rest[..line_len])
}

pub fn split_semicolon(line: &[u8]) -> (&[u8], &[u8]) {
    let mut fields = line.rsplitn(2, |c| *c == b';');
    let (Some(temperature), Some(station)) = (fields.next(), fields.next()) else {
        panic!("unexpected line: {:?}", String::from_utf8_lossy(line));
    };
    (station, temperature)
}

#[cfg(test)]
mod split_semicolon_tests {
    use crate::split_semicolon;

    #[test]
    fn test_split_semicolon() {
        let line = "Chihuahua;16.7";
        let (station, temperature) = split_semicolon(line.as_bytes());
        assert_eq!(station, b"Chihuahua");
        assert_eq!(temperature, b"16.7");
    }
}

pub fn split_semicolon_simd(line: &[u8]) -> (&[u8], &[u8]) {
    const DELIMITER: u8x64 = u8x64::splat(b';');
    if line.len() > 64 {
        // fallback to normal not optimized approach
        line.rsplit_once(|c| *c == b';').unwrap()
    } else {
        let del_eq = DELIMITER.simd_eq(u8x64::load_or_default(line));
        // SAFETY: we are promised there is a ; in every line
        let index_of_delim = unsafe { del_eq.first_set().unwrap_unchecked() };
        (&line[..index_of_delim], &line[index_of_delim + 1..])
    }
}

pub fn split_semicolon_simd_copy(line: &[u8]) -> (&[u8], &[u8]) {
    const DELIMITER: u8x64 = u8x64::splat(b';');
    if line.len() > 64 {
        return line.rsplit_once(|c| *c == b';').unwrap();
    } else {
        let mut buf = [0u8; 64];
        buf[..line.len()].copy_from_slice(line);
        let del_eq = DELIMITER.simd_eq(u8x64::from_array(buf));
        // SAFETY: we are promised there is a ; in every line
        let index_of_delim = unsafe { del_eq.first_set().unwrap_unchecked() };
        (&line[..index_of_delim], &line[index_of_delim + 1..])
    }
}

pub fn split_semicolon_simd_from_data<'a>(data: &'a [u8], start: usize, line: &'a [u8]) -> (&'a [u8], &'a [u8]) {
    const DELIMITER: u8x64 = u8x64::splat(b';');
    if line.len() > 64 {
        return line.rsplit_once(|c| *c == b';').unwrap();
    } else {
        let simd_value = if start + 64 <= data.len() {
            u8x64::from_slice(&data[start..start + 64])
        } else {
            u8x64::load_or_default(line)
        };
        let del_eq = DELIMITER.simd_eq(simd_value);
        // SAFETY: we are promised there is a ; in every line
        let index_of_delim = unsafe { del_eq.first_set().unwrap_unchecked() };
        (&line[..index_of_delim], &line[index_of_delim + 1..])
    }
}

pub fn next_line_jon_version<'a>(map: &'a [u8], at: &mut usize) -> Option<&'a [u8]> {
    let rest = &map[*at..];
    let next_new_line = unsafe { libc::memchr(rest.as_ptr() as *const c_void, b'\n' as c_int, rest.len() as size_t) };
    if next_new_line.is_null() {
        None
    } else {
        let line_len = unsafe { (next_new_line as *const u8).offset_from_unsigned(rest.as_ptr()) };
        // line_len is count wihthout \n therefore +1 in the end
        *at += line_len + 1;
        Some(&rest[..line_len])
    }
}

pub fn next_station_and_temperature_2_fns<'a>(map: &'a [u8], at: &mut usize) -> Option<(&'a [u8], &'a [u8])> {
    let line = next_line_swar(map, at)?;
    if line.is_empty() {
        None
    } else {
        let (temperature, station) = split_semicolon(&line);
        Some((station, temperature))
    }
}

fn next_station_and_temperature_merged_fallback<'a>(map: &'a [u8], at: &mut usize) -> Option<(&'a [u8], &'a [u8])> {
    //fallback to two two fns
    let line = next_line_swar(map, at)?;
    Some(split_semicolon(&line))
}

pub fn next_station_and_temperature_merged_simd_16<'a>(map: &'a [u8], at: &mut usize) -> Option<(&'a [u8], &'a [u8])> {
    if *at + 16 <= map.len() {
        const DELIMITER: u8x16 = u8x16::splat(b';');
        const END_LINE: u8x16 = u8x16::splat(b'\n');
        let simd_value = u8x16::from_slice(&map[*at..*at + 16]);
        let del_eq = DELIMITER.simd_eq(simd_value);
        let end_line_eq = END_LINE.simd_eq(simd_value);
        let (Some(index_of_delim), Some(index_of_end_line)) = (del_eq.first_set(), end_line_eq.first_set()) else {
            return next_station_and_temperature_merged_fallback(map, at);
        };
        let station = &map[*at..*at + index_of_delim];
        let temperature = &map[*at + index_of_delim + 1..*at + index_of_end_line];
        // index_of_end_line is index of \n therefore +1 in the end
        *at += index_of_end_line + 1;
        Some((station, temperature))
    } else {
        next_station_and_temperature_merged_fallback(map, at)
    }
}

pub fn next_station_and_temperature_merged_simd_32<'a>(map: &'a [u8], at: &mut usize) -> Option<(&'a [u8], &'a [u8])> {
    if *at + 32 <= map.len() {
        const DELIMITER: u8x32 = u8x32::splat(b';');
        const END_LINE: u8x32 = u8x32::splat(b'\n');
        let simd_value = u8x32::from_slice(&map[*at..*at + 32]);
        let del_eq = DELIMITER.simd_eq(simd_value);
        let end_line_eq = END_LINE.simd_eq(simd_value);
        let (Some(index_of_delim), Some(index_of_end_line)) = (del_eq.first_set(), end_line_eq.first_set()) else {
            return next_station_and_temperature_merged_fallback(map, at);
        };
        let station = &map[*at..*at + index_of_delim];
        let temperature = &map[*at + index_of_delim + 1..*at + index_of_end_line];
        // index_of_end_line is index of \n therefore +1 in the end
        *at += index_of_end_line + 1;
        Some((station, temperature))
    } else {
        next_station_and_temperature_merged_fallback(map, at)
    }
}

pub fn next_station_and_temperature_merged_simd_64<'a>(map: &'a [u8], at: &mut usize) -> Option<(&'a [u8], &'a [u8])> {
    if *at + 64 <= map.len() {
        const DELIMITER: u8x64 = u8x64::splat(b';');
        const END_LINE: u8x64 = u8x64::splat(b'\n');
        let simd_value = u8x64::from_slice(&map[*at..*at + 64]);
        let del_eq = DELIMITER.simd_eq(simd_value);
        let end_line_eq = END_LINE.simd_eq(simd_value);
        let (Some(index_of_delim), Some(index_of_end_line)) = (del_eq.first_set(), end_line_eq.first_set()) else {
            return next_station_and_temperature_merged_fallback(map, at);
        };
        let station = &map[*at..*at + index_of_delim];
        let temperature = &map[*at + index_of_delim + 1..*at + index_of_end_line];
        // index_of_end_line is index of \n therefore +1 in the end
        *at += index_of_end_line + 1;
        Some((station, temperature))
    } else {
        next_station_and_temperature_merged_fallback(map, at)
    }
}

pub struct Helper<'a> {
    pub first: Option<(&'a [u8], &'a [u8])>,
    pub second: Option<(&'a [u8], &'a [u8])>,
    pub third: Option<(&'a [u8], &'a [u8])>,
    pub fallback_result: bool
}

pub fn next_stations_and_temperatures_merged_simd_64<'a>(map: &'a [u8], at: &mut usize) -> Helper<'a> {
    if *at + 64 <= map.len() {
        const DELIMITER: u8x64 = u8x64::splat(b';');
        const END_LINE: u8x64 = u8x64::splat(b'\n');

        let simd_value = u8x64::from_slice(&map[*at..*at + 64]);
        let del_eq = DELIMITER.simd_eq(simd_value);
        let end_line_eq = END_LINE.simd_eq(simd_value);
        let mut delimiters = del_eq.to_bitmask();
        let mut end_lines = end_line_eq.to_bitmask();

        if end_lines == 0 {
            return Helper {
                first: next_station_and_temperature_merged_fallback(map, at),
                second: None,
                third: None,
                fallback_result: true,
            }
        }

        let first_end_line_index = end_lines.trailing_zeros() as usize;
        end_lines &= end_lines - 1;
        let first_delimiter_index = delimiters.trailing_zeros() as usize;
        let first_station = &map[*at..*at + first_delimiter_index];
        let first_temperature = &map[*at + first_delimiter_index + 1..*at + first_end_line_index];

        if end_lines == 0 {
            *at += first_end_line_index + 1;
            Helper {
                first: Some((first_station, first_temperature)),
                second: None,
                third: None,
                fallback_result: false,
            }
        } else {
            delimiters &= delimiters - 1;

            let second_end_line_index = end_lines.trailing_zeros() as usize;
            end_lines &= end_lines - 1;
            let second_delimiter_index = delimiters.trailing_zeros() as usize;
            let second_station = &map[*at + first_end_line_index + 1..*at + second_delimiter_index];
            let second_temperature = &map[*at + second_delimiter_index + 1..*at + second_end_line_index];
            if end_lines == 0 {
                *at += second_end_line_index + 1;
                Helper {
                    first: Some((first_station, first_temperature)),
                    second: Some((second_station, second_temperature)),
                    third: None,
                    fallback_result: false,
                }
            } else {
                delimiters &= delimiters - 1;

                let third_end_line_index = end_lines.trailing_zeros() as usize;
                let third_delimiter_index = delimiters.trailing_zeros() as usize;
                let third_station = &map[*at + second_end_line_index + 1..*at + third_delimiter_index];
                let third_temperature = &map[*at + third_delimiter_index + 1..*at + third_end_line_index];
                *at += third_end_line_index + 1;
                Helper {
                    first: Some((first_station, first_temperature)),
                    second: Some((second_station, second_temperature)),
                    third: Some((third_station, third_temperature)),
                    fallback_result: false,
                }
            }
        }
        // }
    } else {
        Helper {
            first: next_station_and_temperature_merged_fallback(map, at),
            second: None,
            third: None,
            fallback_result: true,
        }
    }
}

#[cfg(test)]
mod next_stations_and_temperatures_merged_simd_64_tests {
    use crate::next_stations_and_temperatures_merged_simd_64;

    #[test]
    fn next_stations_and_temperatures_merged_simd_64_test_fallback() {
        let data = "Chihuahua;16.7\nBata";
        let mut at: usize = 0;
        let result = next_stations_and_temperatures_merged_simd_64(data.as_bytes(), &mut at);
        assert_eq!(result.first, Some((b"Chihuahua".as_slice(), b"16.7".as_slice())));
        assert_eq!(result.second, None);
        assert_eq!(result.third, None);
        assert_eq!(result.fallback_result, true);
    }

    #[test]
    fn next_stations_and_temperatures_merged_simd_64_test_1_value() {
        let data = "Chihuahua;16.7\nBataaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let mut at: usize = 0;
        let result = next_stations_and_temperatures_merged_simd_64(data.as_bytes(), &mut at);
        assert_eq!(result.first, Some((b"Chihuahua".as_slice(), b"16.7".as_slice())));
        assert_eq!(result.second, None);
        assert_eq!(result.third, None);
        assert_eq!(result.fallback_result, false);
    }

    #[test]
    fn next_stations_and_temperatures_merged_simd_64_test_1_2_values() {
        let data = "Chihuahua;16.7\nBata;15.0\nBataaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let mut at: usize = 0;
        let result = next_stations_and_temperatures_merged_simd_64(data.as_bytes(), &mut at);
        assert_eq!(result.first, Some((b"Chihuahua".as_slice(), b"16.7".as_slice())));
        assert_eq!(result.second, Some((b"Bata".as_slice(), b"15.0".as_slice())));
        assert_eq!(result.third, None);
        assert_eq!(result.fallback_result, false);
    }

    #[test]
    fn next_stations_and_temperatures_merged_simd_64_test_3_values() {
        let data = "Chihuahua;16.7\nBata;15.0\nDurban;12.2\nVientiane;13.9\nKarachi;12.6\nDurban;13.4\nLaunceston;-3.4\nBishkek;15.9\nKuwait City;39.3\nValletta;18.9";
        let mut at: usize = 0;
        let result = next_stations_and_temperatures_merged_simd_64(data.as_bytes(), &mut at);
        assert_eq!(result.first, Some((b"Chihuahua".as_slice(), b"16.7".as_slice())));
        assert_eq!(result.second, Some((b"Bata".as_slice(), b"15.0".as_slice())));
        assert_eq!(result.third, Some((b"Durban".as_slice(), b"12.2".as_slice())));
        assert_eq!(result.fallback_result, false);
    }
}

pub fn hashmap_upsert(stats: &mut HashMap<Vec<u8>, MeasurementStats>, station: &[u8], temperature: i16) {
    let stat = match stats.get_mut(station) {
        Some(stat) => stat,
        None => stats.entry(station.to_vec()).or_insert(MeasurementStats::default())
    };
    stat.update(temperature);
}

pub fn hashmap_upsert_custom_hash_builder(stats: &mut HashMap<Vec<u8>, MeasurementStats, FastHasherBuilder>, station: &[u8], temperature: i16) {
    let stat = match stats.get_mut(station) {
        Some(stat) => stat,
        None => stats.entry(station.to_vec()).or_insert(MeasurementStats::default())
    };
    stat.update(temperature);
}

pub fn hashmap_upsert_ideal_hash_builder(stats: &mut HashMap<Vec<u8>, MeasurementStats, IdealHasherBuilder>, station: &[u8], temperature: i16) {
    let stat = match stats.get_mut(station) {
        Some(stat) => stat,
        None => stats.entry(station.to_vec()).or_insert(MeasurementStats::default())
    };
    stat.update(temperature);
}

pub struct FastHasherBuilder;
pub struct FastHasher(u64);

impl BuildHasher for FastHasherBuilder {
    type Hasher = FastHasher;

    fn build_hasher(&self) -> Self::Hasher {
        FastHasher(0xcbf29ce484222325)
    }
}

impl Hasher for FastHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        let (chunks,remainder) = bytes.as_chunks::<8>();
        let mut last = [1u8; 8];
        (last[..remainder.len()].copy_from_slice(remainder));
        for &chunk in chunks.iter().chain(iter::once(&last)) {
            let mixed = self.0 as u128 * (u64::from_ne_bytes(chunk) as u128);
            self.0 = (mixed >> 64) as u64 ^ mixed as u64;
        }
    }
}

#[test]
fn collisions_fast_hasher() {
    use std::collections::{HashMap, HashSet};
    use std::hash::BuildHasher;
    use std::fs::File;
    use crate::app::{MMapHelper, StationTemperatureIterator};

    let file = File::open("input/1B_measurements.txt").unwrap();
    let file_len: usize = file.metadata().unwrap().len() as usize;
    let m_m_helper = MMapHelper::new(&file, file_len);
    let map = m_m_helper.as_slice();

    // FIXME tohle je problem. cely vektor zabere strasne mista. je potreba delat spodni cast postupne misto pouzivani cele pameti
    let stations = StationTemperatureIterator::new(map)
        .map(|(station, _)| station.as_bytes())
        .collect::<Vec<&[u8]>>();

    // FIXME => mam napad ze kdyz zvednu slots tak bude mene kolizi (nejlepe zadne) a pak to bude super rychle jen to musi vlezt do SIMD
    // FIXME => tohle chces hlavne udelat pro klasickou HashMap => tady zacit!
    // with_capacity(10_000) gives 16_384 slots (a power of two)
    let slots = 16_384u64;
    let hashes: Vec<u64> = stations.iter().map(|name| FastHasherBuilder.hash_one(name)).collect();

    let distinct = |f: &dyn Fn(u64) -> u64| hashes.iter().map(|&h| f(h)).collect::<HashSet<_>>().len();
    println!("names:            {}", stations.len());
    println!("distinct hashes:  {}", distinct(&|h| h));
    println!("distinct slots:   {}", distinct(&|h| h & (slots - 1)));
    println!("distinct tags:    {} of 128", distinct(&|h| h >> 57));

    // the biggest group of names with the same hash
    let mut groups: HashMap<u64, usize> = HashMap::new();
    for &h in &hashes { *groups.entry(h).or_default() += 1; }
    println!("largest group:    {}", groups.values().max().unwrap());
}