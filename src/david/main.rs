#![feature(portable_simd)]

// this challenge has a rule that all code has to be in one file
use std::{env, thread};
use std::collections::BTreeMap;
use std::fs::File;
use std::hash::{BuildHasher, Hash, Hasher};
use std::os::fd::AsRawFd;
use std::simd::{u8x16, u8x64};
use std::simd::prelude::SimdPartialEq;
use std::simd::cmp::SimdPartialOrd;
use std::sync::mpsc;

#[derive(Debug)]
pub struct MeasurementStats {
    min: i16,
    max: i16,
    sum: i64,
    count: u32
}

impl MeasurementStats {
    pub fn update(&mut self, measurement: i16) {
        if measurement < self.min {
            self.min = measurement;
        }
        if measurement > self.max {
            self.max = measurement;
        }
        self.sum += i64::from(measurement);
        self.count += 1;
    }

    pub fn update_stats(&mut self, measurement_stats: &MeasurementStats) {
        if measurement_stats.min < self.min {
            self.min = measurement_stats.min;
        }
        if measurement_stats.max > self.max {
            self.max = measurement_stats.max;
        }
        self.sum += measurement_stats.sum;
        self.count += measurement_stats.count;
    }
}

impl Default for MeasurementStats {
    fn default() -> Self {
        MeasurementStats {
            min: i16::MAX,
            max: i16::MIN,
            sum: 0,
            count: 0
        }
    }
}

pub fn parse_temperature(temperature: &[u8]) -> i16 {
    let is_negative = (temperature[0] == b'-') as usize;
    let digits = &temperature[is_negative..];
    let len = digits.len();

    let tenths = (digits[len - 1] - b'0') as i16;
    let ones = (digits[len - 3] - b'0') as i16;
    let tens = (digits[0] - b'0') as i16 * (len == 4) as i16;

    let value = tens * 100 + ones * 10 + tenths;
    // is_negative has 0 or 1 value therefore 1 - (2 * 0) = 1, 1 - (2 * 1) = -1
    value * (1 - (2 * is_negative as i16))
}

#[cfg(test)]
mod parse_temperature_tests {
    use super::parse_temperature;

    #[test]
    fn test_parse_temperature() {
        assert_eq!(parse_temperature(b"99.9"), 999);
        assert_eq!(parse_temperature(b"2.5"), 25);
        assert_eq!(parse_temperature(b"0.1"), 1);
        assert_eq!(parse_temperature(b"0.0"), 0);
        assert_eq!(parse_temperature(b"-0.1"), -1);
        assert_eq!(parse_temperature(b"-2.5"), -25);
        assert_eq!(parse_temperature(b"-99.9"), -999);

    }
}

pub struct StationTemperatureIterator<'a>{
    map: &'a [u8],
    at: usize,
    block_start: usize,
    delimiter_bit_mask: u64,
    newline_bit_mask: u64,
}

impl <'a> StationTemperatureIterator<'a> {
    pub fn new(map: &'a [u8]) -> StationTemperatureIterator<'a> {
        Self {
            map,
            at: 0,
            block_start: 0,
            delimiter_bit_mask: 0,
            newline_bit_mask: 0,
        }
    }
}

//TODO this code works with full pairs in the SIMD only so it can be optimized
// for example delimiter in first simd and newline in another simd
impl <'a> Iterator for StationTemperatureIterator<'a> {
    type Item = (StationName<'a>, &'a [u8]);

    fn next(&mut self) -> Option<Self::Item> {
        const SIZE_64: usize = 64;
        const DELIMITER: u8x64 = u8x64::splat(b';');
        const END_LINE: u8x64 = u8x64::splat(b'\n');

        if self.at >= self.map.len() {
            return None;
        }

        if self.newline_bit_mask == 0 {
            self.block_start = self.at;
            let simd_value = if self.at + SIZE_64 <= self.map.len() {
                u8x64::from_slice(&self.map[self.at..self.at + SIZE_64])
            } else {
                u8x64::load_or_default(&self.map[self.at..self.map.len()])
            };
            let del_eq = DELIMITER.simd_eq(simd_value);
            self.delimiter_bit_mask = del_eq.to_bitmask();

            let end_line_eq = END_LINE.simd_eq(simd_value);
            self.newline_bit_mask = end_line_eq.to_bitmask();
        }
        let delimiter_index = self.delimiter_bit_mask.trailing_zeros() as usize;
        self.delimiter_bit_mask &= self.delimiter_bit_mask - 1;
        let newline_index = self.newline_bit_mask.trailing_zeros() as usize;
        self.newline_bit_mask &= self.newline_bit_mask - 1;
        let station = &self.map[self.at..self.block_start + delimiter_index];
        let station_name = StationName{ rest: &self.map[self.at..], len: station.len()};
        let temperature = &self.map[self.block_start + delimiter_index + 1..self.block_start + newline_index];
        self.at = self.block_start + newline_index + 1;
        Some((station_name, temperature))
    }
}

#[cfg(test)]
mod station_temperature_iterator_tests {
    use super::StationTemperatureIterator;

    #[test]
    fn next_stations_and_temperatures_merged_simd_64_test_last_line() {
        let data = "Chihuahua;16.7\n";
        let mut iterator = StationTemperatureIterator::new(data.as_bytes());
        let next_opt = iterator.next();
        assert_eq!(next_opt.is_some(), true);

        let next = next_opt.unwrap();
        assert_eq!(next.0.as_bytes()  , b"Chihuahua");
        assert_eq!(next.1, b"16.7");

        let next_opt = iterator.next();
        assert_eq!(next_opt.is_none(), true);
    }

    #[test]
    fn next_stations_and_temperatures_merged_simd_64_test_more_line() {
        let data = "Chihuahua;16.7\nBata;15.0\nDurban;12.2\nVientiane;13.9\nKarachi;12.6\nDurban;13.4\nLaunceston;-3.4\nBishkek;15.9\nKuwait City;39.3\nValletta;18.9\n";

        let mut iterator = StationTemperatureIterator::new(data.as_bytes());
        let mut next_opt = iterator.next();
        assert_eq!(next_opt.is_some(), true);
        let mut next = next_opt.unwrap();
        assert_eq!(next.0.as_bytes(), b"Chihuahua");
        assert_eq!(next.1, b"16.7");

        next_opt = iterator.next();
        assert_eq!(next_opt.is_some(), true);
        next = next_opt.unwrap();
        assert_eq!(next.0.as_bytes(), b"Bata");
        assert_eq!(next.1, b"15.0");

        next_opt = iterator.next();
        assert_eq!(next_opt.is_some(), true);
        next = next_opt.unwrap();
        assert_eq!(next.0.as_bytes(), b"Durban");
        assert_eq!(next.1, b"12.2");

        next_opt = iterator.next();
        assert_eq!(next_opt.is_some(), true);
        next = next_opt.unwrap();
        assert_eq!(next.0.as_bytes(), b"Vientiane");
        assert_eq!(next.1, b"13.9");

        next_opt = iterator.next();
        assert_eq!(next_opt.is_some(), true);
        next = next_opt.unwrap();
        assert_eq!(next.0.as_bytes(), b"Karachi");
        assert_eq!(next.1, b"12.6");

        next_opt = iterator.next();
        assert_eq!(next_opt.is_some(), true);
        next = next_opt.unwrap();
        assert_eq!(next.0.as_bytes(), b"Durban");
        assert_eq!(next.1, b"13.4");

        next_opt = iterator.next();
        assert_eq!(next_opt.is_some(), true);
        next = next_opt.unwrap();
        assert_eq!(next.0.as_bytes(), b"Launceston");
        assert_eq!(next.1, b"-3.4");

        next_opt = iterator.next();
        assert_eq!(next_opt.is_some(), true);
        next = next_opt.unwrap();
        assert_eq!(next.0.as_bytes(), b"Bishkek");
        assert_eq!(next.1, b"15.9");

        next_opt = iterator.next();
        assert_eq!(next_opt.is_some(), true);
        next = next_opt.unwrap();
        assert_eq!(next.0.as_bytes(), b"Kuwait City");
        assert_eq!(next.1, b"39.3");

        next_opt = iterator.next();
        assert_eq!(next_opt.is_some(), true);
        next = next_opt.unwrap();
        assert_eq!(next.0.as_bytes(), b"Valletta");
        assert_eq!(next.1, b"18.9");

        next_opt = iterator.next();
        assert_eq!(next_opt.is_none(), true);
    }
}

pub struct IdealHasherBuilder;
pub struct IdealHasher(u64);

impl BuildHasher for IdealHasherBuilder {
    type Hasher = IdealHasher;

    fn build_hasher(&self) -> Self::Hasher {
        IdealHasher(0x243F_6A88_85A3_08D3)
    }
}

impl IdealHasher {
    const K: u64 = 0x9E37_79B9_7F4A_7C15;

    fn folded_multiply(a: u64, b: u64) -> u64 {
        let m = a as u128 * b as u128;
        (m >> 64) as u64 ^ m as u64
    }

    fn read_u64(bytes: &[u8]) -> u64 {
        u64::from_le_bytes(bytes[..8].try_into().unwrap())
    }
}

impl Hasher for IdealHasher {
    fn finish(&self) -> u64 {
        // final mix, so that the top bits (hashbrown's tag) depend on everything
        Self::folded_multiply(self.0, Self::K)
    }

    // Hash for [u8] writes the length first; mix it in cheaply instead of as a whole block
    fn write_usize(&mut self, len: usize) {
        self.0 ^= len as u64;
    }

    fn write(&mut self, bytes: &[u8]) {
        let len = bytes.len();
        let (a, b) = if len >= 8 {
            // first 8 and last 8 bytes, they overlap for names of 8 – 15 bytes
            let mut a = Self::read_u64(bytes);
            let b = Self::read_u64(&bytes[len - 8..]);
            // the middle of names longer than 16 bytes (rare)
            let mut i = 8;
            while i + 8 < len - 8 {
                a = Self::folded_multiply(a ^ Self::read_u64(&bytes[i..]), Self::K);
                i += 8;
            }
            (a, b)
        } else if len >= 4 {
            // first 4 and last 4 bytes, they overlap for names of 4 – 7 bytes
            let a = u32::from_le_bytes(bytes[..4].try_into().unwrap()) as u64;
            let b = u32::from_le_bytes(bytes[len - 4..].try_into().unwrap()) as u64;
            (a, b)
        } else if len > 0 {
            // 1 – 3 bytes: first, middle and last byte
            let a = bytes[0] as u64 | (bytes[len / 2] as u64) << 8 | (bytes[len - 1] as u64) << 16;
            (a, 0)
        } else {
            (0, 0)
        };
        self.0 = Self::folded_multiply(a ^ self.0, b ^ Self::K);
    }
}

impl Hash for StationName<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.rest[..self.len].hash(state);
    }
}

#[derive(Clone, Copy)]
pub struct StationName<'a> {
    pub rest: &'a [u8],
    pub len: usize,
}

impl<'a> StationName<'a> {
    pub fn as_bytes(&self) -> &'a [u8] {
        &self.rest[..self.len]
    }
}


const fn indices<const N: usize>() -> [u8; N] {
    let mut indicies = [0u8; N];
    let mut i = 0;
    while i < N {
        indicies[i] = i as u8;
        i += 1;
    }
    indicies
}

const SIMD_INDICES_16: u8x16 = u8x16::from_array(indices::<16usize>());
impl PartialEq for StationName<'_> {
    fn eq(&self, other: &Self) -> bool {
        if self.len != other.len {
            false
        } else {
            if self.len <= 16 && self.rest.len() >= 16 && other.rest.len() >= 16 {
                let indices = u8x16::splat(self.len as u8);
                let valid = SIMD_INDICES_16.simd_lt(indices);
                let a = u8x16::from_slice(self.rest);
                let b = u8x16::from_slice(other.rest);
                let equal = a.simd_eq(b);
                (equal | !valid).all()
            } else {
                self.rest[..self.len] == other.rest[..other.len]
            }
        }
    }
}

impl Eq for StationName<'_> {}

#[allow(dead_code)]
fn one_upper_bound(map: &[u8]) -> [MeasurementStats; 128] {
    let mut stats_array: [MeasurementStats; 128] = std::array::from_fn(|_| MeasurementStats::default());
    for (station, temperature) in StationTemperatureIterator::new(map) {
        let t = parse_temperature(temperature);
        let measurement_stats = &mut stats_array[station.len];
        measurement_stats.update(t);
    }
    stats_array
}

#[inline(always)]
fn one(map: &[u8]) -> Vec<(&[u8], MeasurementStats)> {
    let mut stats = StationTable::new();
    for (station, temperature) in StationTemperatureIterator::new(map) {
        let t = parse_temperature(temperature);
        stats.get_mut(station).update(t);
    }
    stats.into_entries()
}

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

const SLOT_BITS: usize = 14;
//14 => 16_384 so it will cover the max 10K station names from 1BRC rules
const SLOTS: usize = 1 << SLOT_BITS;
const SLOT_MASK: usize = SLOTS - 1;

fn key_words(station: &StationName) -> (u64, u64) {
    if station.rest.len() < 16 {
        let mut station_name_with_zeros = [0u8; 16];
        station_name_with_zeros[..station.len].copy_from_slice(&station.rest[..station.len]);
        let station_first_16_bytes = u128::from_le_bytes(station_name_with_zeros);
        let word_1 = station_first_16_bytes as u64;
        let word_2 = (station_first_16_bytes >> 64) as u64;
        (word_1, word_2)
    } else {
        let mask = u128::MAX >> (128 - (8 * station.len.min(16)));
        let station_first_16_bytes = u128::from_le_bytes(station.rest[..16].try_into().unwrap());
        let masked_station = station_first_16_bytes & mask;
        let word_1 = masked_station as u64;
        let word_2 = (masked_station >> 64) as u64;
        (word_1, word_2)
    }
}

#[cfg(test)]
mod key_words_tests {
    use super::{key_words, StationName};

    #[test]
    fn name_of_4_bytes() {
        let data = b"Bata;15.0\nDurban;12.2\n";
        let station = StationName { rest: &data[..], len: 4 };
        let (word1, word2) = key_words(&station);
        assert_eq!(word1, u64::from_le_bytes(*b"Bata\0\0\0\0"));
        assert_eq!(word2, 0);
    }

    #[test]
    fn name_of_11_bytes() {
        let data = b"Kuwait City;39.3\nBata;15.0\n";
        let station = StationName { rest: &data[..], len: 11 };
        let (word1, word2) = key_words(&station);
        assert_eq!(word1, u64::from_le_bytes(*b"Kuwait C"));
        assert_eq!(word2, u64::from_le_bytes(*b"ity\0\0\0\0\0"));
    }

    #[test]
    fn name_of_16_bytes() {
        let data = b"Petropavlovsk-Ka;1.2\nBata;15.0\n";
        let station = StationName { rest: &data[..], len: 16 };
        let (word1, word2) = key_words(&station);
        assert_eq!(word1, u64::from_le_bytes(*b"Petropav"));
        assert_eq!(word2, u64::from_le_bytes(*b"lovsk-Ka"));
    }

    #[test]
    fn name_of_24_bytes() {
        let data = b"Petropavlovsk-Kamchatsky;1.2\nBata;15.0\n";
        let station = StationName { rest: &data[..], len: 24 };
        let (word1, word2) = key_words(&station);
        assert_eq!(word1, u64::from_le_bytes(*b"Petropav"));
        assert_eq!(word2, u64::from_le_bytes(*b"lovsk-Ka"));
    }

    #[test]
    fn name_end_chunk() {
        let data = b"Bata;15.0\n";
        let station = StationName { rest: &data[..], len: 4 };
        let (word1, word2) = key_words(&station);
        assert_eq!(word1, u64::from_le_bytes(*b"Bata\0\0\0\0"));
        assert_eq!(word2, 0);
    }
}

fn hash(word_1: u64, word_2: u64) -> usize {
    // reduce overlapping by rotating second word
    let words = word_1 ^ word_2.rotate_left(29u32);
    let multiplied_by_magic_constant = words.wrapping_mul(0x243F_6A88_85A3_08D3 as u64);
    //reduce to numbers from 0 to 2_047
    (multiplied_by_magic_constant as usize) >> (64 - SLOT_BITS)
}

#[cfg(test)]
mod hash_tests {
    use super::*;

    #[test]
    fn test_hash() {
        assert!(hash(0, 0) < SLOTS);
        assert!(hash(u64::MAX, u64::MAX) < SLOTS);
        assert!(hash(u64::MAX, 0) < SLOTS);
    }
}

#[cfg(test)]
mod keys_with_hash_test {
    use super::*;

    #[test]
    fn test_hash() {
        let hash_1 = {
            let data = b"Bata;15.0\nDurban;12.2\n";
            let station = StationName { rest: &data[..], len: 4 };
            let (word1, word2) = key_words(&station);
            hash(word1, word2)
        };
        let hash_2 = {
            let data = b"Bata;17.0\n";
            let station = StationName { rest: &data[..], len: 4 };
            let (word1, word2) = key_words(&station);
            hash(word1, word2)
        };
        assert_eq!(hash_1, hash_2);
    }
}

#[derive(Debug)]
struct Slot {
    word_1: u64,
    word_2: u64,
    stats: MeasurementStats,
}

impl Default for Slot {
    fn default() -> Self {
        Self {
            word_1: 0,
            word_2: 0,
            stats: MeasurementStats::default(),
        }
    }
}

struct StationTable<'a> {
    slots: Box<[Slot; SLOTS]>,
    names: Box<[&'a [u8]; SLOTS]>,
}

impl <'a> StationTable<'a> {
    fn new() -> Self {
        Self {
            slots: (0..SLOTS).map(|_| Slot::default())
                .collect::<Vec<_>>()
                .into_boxed_slice()
                .try_into()
                .unwrap(),
            names: (0..SLOTS).map(|_| &[][..])
                .collect::<Vec<_>>()
                .into_boxed_slice()
                .try_into()
                .unwrap(),
        }
    }

    #[inline(always)]
    fn get_mut(&mut self, station_name: StationName<'a>) -> &mut MeasurementStats {
        let (word_1, word_2) = key_words(&station_name);
        let mut index = hash(word_1, word_2);
        loop {
            if self.slots[index].word_1 == 0 {
                self.slots[index].word_1 = word_1;
                self.slots[index].word_2 = word_2;
                self.names[index] = station_name.as_bytes();
                return &mut self.slots[index].stats;
            } else if self.slots[index].word_1 == word_1
                && self.slots[index].word_2 == word_2
                && (station_name.len < 16 || self.names[index] == station_name.as_bytes()) {
                return &mut self.slots[index].stats;
            } else {
                index = (index + 1) & SLOT_MASK;
            }
        }
    }

    fn into_entries(self) -> Vec<(&'a [u8], MeasurementStats)> {
        self.names.into_iter().zip(self.slots)
            .filter(|(_, slot) | slot.word_1 != 0)
            .map(|(name, slot)| (name, slot.stats))
            .collect()
    }
}

#[cfg(test)]
mod station_table_tests {
    use super::*;

    #[test]
    fn into_entries_empty_test() {
        let table = StationTable::new();
        assert!(table.into_entries().is_empty());
    }

    #[test]
    fn new_test() {
        let station_table = StationTable::new();
        assert_eq!(station_table.slots.len(), SLOTS);
        assert_eq!(station_table.names.len(), SLOTS);
        assert!(station_table.slots.iter().all(|s| s.word_1 == 0));
    }

    #[test]
    fn get_mut_test() {
        let mut station_table = StationTable::new();
        let station_name = StationName { rest: b"Kuwait City;1.0\n", len: 11 };
        let stats = station_table.get_mut(station_name);
        assert_eq!(stats.min, i16::MAX);
        assert_eq!(stats.max, i16::MIN);
        assert_eq!(stats.sum, 0);
        assert_eq!(stats.count, 0);

        stats.min = 1;
        stats.max = 2;
        stats.sum = 3;
        stats.count = 4;

        let station_name = StationName { rest: b"Kuwait City;2.0\n", len: 11 };
        let stats = station_table.get_mut(station_name);
        assert_eq!(stats.min, 1);
        assert_eq!(stats.max, 2);
        assert_eq!(stats.sum, 3);
        assert_eq!(stats.count, 4);
    }

    #[test]
    fn into_entries_test() {
        let mut station_table = StationTable::new();
        let station_name = StationName { rest: b"Kuwait City;1.0\n", len: 11 };
        let stats = station_table.get_mut(station_name);
        stats.count = 1;

        let station_name = StationName { rest: b"Petropavlovsk-Kamchatsky;2.0\n", len: 24 };
        let stats = station_table.get_mut(station_name);
        stats.count = 2;

        let mut entries = station_table.into_entries();
        entries.sort_by(|a, b| a.0.cmp(b.0));
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].0, b"Kuwait City");
        assert_eq!(entries[0].1.count, 1);
        assert_eq!(entries[1].0, b"Petropavlovsk-Kamchatsky");
        assert_eq!(entries[1].1.count, 2);
    }

    #[test]
    fn into_entries_two_same_stations_test() {
        let mut station_table = StationTable::new();
        let station_name = StationName { rest: b"Kuwait City;1.0\n", len: 11 };
        let stats = station_table.get_mut(station_name);
        stats.count = 1;

        let station_name = StationName { rest: b"Kuwait City;2.0\n", len: 11 };
        let stats = station_table.get_mut(station_name);
        stats.count = 2;

        let mut entries = station_table.into_entries();
        entries.sort_by(|a, b| a.0.cmp(b.0));
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0, b"Kuwait City");
        assert_eq!(entries[0].1.count, 2);
    }

    #[test]
    fn get_mut_same_prefix_test() {
        let mut station_table = StationTable::new();
        let station_name = StationName { rest: b"Petropavlovsk-Kamchatsky;1.0\n", len: 24 };
        let stats = station_table.get_mut(station_name);
        assert_eq!(stats.min, i16::MAX);
        assert_eq!(stats.max, i16::MIN);
        assert_eq!(stats.sum, 0);
        assert_eq!(stats.count, 0);

        stats.min = 1;
        stats.max = 2;
        stats.sum = 3;
        stats.count = 4;

        let station_name = StationName { rest: b"Petropavlovsk-Ka ;2.0\n", len: 16 };
        let stats = station_table.get_mut(station_name);
        assert_eq!(stats.min, i16::MAX);
        assert_eq!(stats.max, i16::MIN);
        assert_eq!(stats.sum, 0);
        assert_eq!(stats.count, 0);

        let mut entries = station_table.into_entries();
        entries.sort_by(|a, b| a.0.cmp(b.0));
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].0, b"Petropavlovsk-Ka");
        assert_eq!(entries[0].1.count, 0);
        assert_eq!(entries[1].0, b"Petropavlovsk-Kamchatsky");
        assert_eq!(entries[1].1.count, 4);
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let input_file_path = &args[1];
    let number_of_threads = args[2].parse::<usize>().unwrap();

    let file = File::open(input_file_path).unwrap();
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
    let chunk_size = file_len / number_of_threads;

    thread::scope(|scope| {
        let (tx, rx) = mpsc::sync_channel(number_of_threads);

        let mut start = 0;
        for thread_number in 0..number_of_threads {
            if thread_number == (number_of_threads - 1) {
                // last chunk
                let chunk = &m_map[start..file_len];
                let tx = tx.clone();
                scope.spawn(move || {
                    tx.send(one(&chunk))
                });
            } else {
                let end = find_new_line(&m_map[start + chunk_size..]).unwrap();
                let chunk = &m_map[start..(start + chunk_size + end + 1).min(file_len)];
                let tx = tx.clone();
                scope.spawn(move || {
                    tx.send(one(&chunk))
                });
                start = start + chunk_size + end + 1;
            };
        }
        drop(tx);

        let mut stats_map = BTreeMap::<&[u8], MeasurementStats>::new();
        for one_stats in rx {
            for (station_name, measurement_stats) in one_stats {
                let ms = stats_map.entry(station_name).or_default();
                ms.update_stats(&measurement_stats);
            }
        }

        for (station_name, measurement_stats) in stats_map.iter() {
            println!(
                        "{}={:.1}/{:.1}/{:.1}",
                        std::str::from_utf8(station_name).unwrap(),
                        (measurement_stats.min as f64) / 10.,
                        (measurement_stats.sum as f64) / 10. / (measurement_stats.count as f64),
                        (measurement_stats.max as f64) / 10.
                    );
        }
    });
}
