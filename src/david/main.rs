#![feature(portable_simd)]

// this challenge has a rule that all code has to be in one file
use std::{env, thread};
use std::collections::BTreeMap;
use std::fs::File;
use std::hash::{BuildHasher, Hash, Hasher};
use std::os::fd::AsRawFd;
use std::simd::{u8x16, u8x32, u8x64};
use std::simd::prelude::SimdPartialEq;
use std::simd::cmp::SimdPartialOrd;
use std::sync::mpsc;

#[derive(Debug, Clone, Copy)]
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

#[inline(always)]
pub fn dot_index(temperature: &[u8]) -> u8 {
    let temperature_in_number = u32::from_le_bytes(temperature[..4].try_into().unwrap());
    let masked = !temperature_in_number & 0b00010000_00010000_00010000_00000000u32;
    let trailing_zeros = masked.trailing_zeros() as u8;
    trailing_zeros / 8
}

// if 4th bit is 0 it's negative because otherwise it's number from 0 to 9 and those have 4th bit 1
// return -1 for minus on first byt and 0 for everything else
#[inline(always)]
pub fn positive_or_negative(temperature: &[u8]) -> i8 {
    ((!temperature[0] as i8) << 3) >> 7
}

#[inline(always)]
pub fn parse_temperature_swar_full_version(input: &[u8]) -> i32 {
    const MASKS: [u64; 2] = [
        0x00FF_FFFF, // len 4 so mask index 4 so 5th index is ;
        0xFFFF_FFFF, // len 4 so mask index 5 as it's full number there
    ];

    let positive_or_negative = positive_or_negative(input);
    let shift = (positive_or_negative & 1) as usize;
    let without_sign = u64::from_le_bytes(input[shift..shift + 8].try_into().unwrap());
    let dot_index = dot_index(input) as usize;
    let mask_index = dot_index - shift - 1;
    let bound_safe_mask_index = mask_index & 1;
    let masked = without_sign & MASKS[bound_safe_mask_index];
    let align_shift = 8 * (1 - bound_safe_mask_index);
    let align = masked << align_shift;
    let dot_to_zero = align &  0xFF00_FFFF;
    let from_text_to_numbers = dot_to_zero & 0x0F00_0F0F;
    let unsigned_result = (from_text_to_numbers.wrapping_mul(0x640A_0001) >> 24) & 0x3FF;
    let signed_result = unsigned_result as i32 * (1 - (2 * shift as i32));
    signed_result
}

#[inline(always)]
pub fn parse_temperature_swar(input: &[u8]) -> i16 {
    let word = if input.len() >= 8 {
        u64::from_le_bytes(input[..8].try_into().unwrap())
    } else {
        let mut helper_array = [0u8; 8];
        helper_array[..input.len()].copy_from_slice(input);
        u64::from_le_bytes(helper_array)
    };
    let dot_index =  (!word & 0x1010_1000).trailing_zeros() / 8;
    let sign =  ((!word << 59) as i64) >> 63;
    let without_sign = word & !((sign as u64) & 0xFF);
    let aligned = without_sign << (8 * (3 - dot_index));
    let digits = aligned & 0x0F_000F_0F00;
    let value = (digits.wrapping_mul(0x640A_0001) >> 32) & 0x3FF;
    ((value as i64 ^ sign) - sign) as i16
}

pub fn parse_temperature_swar_with_dot_index(input: &[u8]) -> (i16, usize) {
    let word = if input.len() >= 8 {
        u64::from_le_bytes(input[..8].try_into().unwrap())
    } else {
        let mut helper_array = [0u8; 8];
        helper_array[..input.len()].copy_from_slice(input);
        u64::from_le_bytes(helper_array)
    };
    parse_temperature_word(word)
}

// the 8 bytes from the start of the temperature, already loaded
#[inline(always)]
pub fn parse_temperature_word(word: u64) -> (i16, usize) {
    // 12, 20 or 28: bit 4 of the byte of the dot
    let dot_bits = (!word & 0x1010_1000).trailing_zeros();
    let dot_index = (dot_bits / 8) as usize;
    let sign =  ((!word << 59) as i64) >> 63;
    let without_sign = word & !((sign as u64) & 0xFF);
    // 8 * (3 - dot_index) is 28 - dot_bits, one subtraction
    let aligned = without_sign << (28 - dot_bits);
    let digits = aligned & 0x0F_000F_0F00;
    let value = (digits.wrapping_mul(0x640A_0001) >> 32) & 0x3FF;
    let temperature = ((value as i64 ^ sign) - sign) as i16;
    (temperature, dot_index)
}

#[cfg(test)]
mod parse_temperature_tests {
    use super::{dot_index, parse_temperature, positive_or_negative, parse_temperature_swar_full_version, parse_temperature_swar};

    #[test]
    fn parse_temperature_swar_test() {
        assert_eq!(parse_temperature_swar_full_version("0.0;abcd".as_bytes()), 0);
        assert_eq!(parse_temperature_swar_full_version("0.1;abcd".as_bytes()), 1);
        assert_eq!(parse_temperature_swar_full_version("0.2;abcd".as_bytes()), 2);
        assert_eq!(parse_temperature_swar_full_version("1.0;abcd".as_bytes()), 10);
        assert_eq!(parse_temperature_swar_full_version("99.9;abcd".as_bytes()), 999);

        assert_eq!(parse_temperature_swar_full_version("-0.1;abcd".as_bytes()), -1);
        assert_eq!(parse_temperature_swar_full_version("-0.2;abcd".as_bytes()), -2);
        assert_eq!(parse_temperature_swar_full_version("-1.0;abcd".as_bytes()), -10);
        assert_eq!(parse_temperature_swar_full_version("-99.9;abcd".as_bytes()), -999);

        assert_eq!(parse_temperature_swar("0.0;abcd".as_bytes()), 0);
        assert_eq!(parse_temperature_swar("0.1;abcd".as_bytes()), 1);
        assert_eq!(parse_temperature_swar("0.2;abcd".as_bytes()), 2);
        assert_eq!(parse_temperature_swar("1.0;abcd".as_bytes()), 10);
        assert_eq!(parse_temperature_swar("99.9;abcd".as_bytes()), 999);

        assert_eq!(parse_temperature_swar("-0.1;abcd".as_bytes()), -1);
        assert_eq!(parse_temperature_swar("-0.2;abcd".as_bytes()), -2);
        assert_eq!(parse_temperature_swar("-1.0;abcd".as_bytes()), -10);
        assert_eq!(parse_temperature_swar("-99.9;abcd".as_bytes()), -999);

        assert_eq!(parse_temperature_swar(b"-12.3\n"), -123);
        assert_eq!(parse_temperature_swar(b"1.5\n"), 15);
        assert_eq!(parse_temperature_swar(b"1.5"), 15);
    }

    #[test]
    fn how_to() {
        let input = "0.1;abcd";
        let input_bytes = input.as_bytes();
        let dot_index = dot_index(input_bytes) as usize;

        //-1 or 0
        let positive_or_negative = positive_or_negative(input.as_bytes());
        let shift = (positive_or_negative & 1) as usize;
        let without_sign = u32::from_le_bytes(input_bytes[shift..shift + 4].try_into().unwrap());
        // asci 0 = 00110000
        // asci 1 = 00110001
        // asci . = 00101110
        assert_eq!(format!("{:032b}", without_sign), "00111011001100010010111000110000");
        // dot_index can be 1 or 2 or 3 where shift is 0 or 1
        // 1.3 => 1 - 0 - 1 = 0
        // -1.3 => 2 - 1 - 1 = 0
        // 11.3 => 2 - 0 - 1 = 1
        // -11.3 => 3 - 1 - 1 = 1
        let mask_index = dot_index - shift - 1;
        // so values from <0; 1>
        let bound_safe_mask_index = mask_index & 1;
        // len can be 3 (for example number 1.8) or 4 (for example number 99.8) only as this number is without sign
        const MASKS: [u32; 2] = [
            0x00FF_FFFF, // len 4 so mask index 4 so 5th index is ;
            0xFFFF_FFFF, // len 4 so mask index 5 as it's full number there
        ];
        let masked = without_sign & MASKS[bound_safe_mask_index];
        assert_eq!(format!("{:032b}", masked), "00000000001100010010111000110000");
        // align numbers
        let align_shift = 8 * (1 - bound_safe_mask_index);
        let align = masked << align_shift;
        // asci 0 = 00110000
        // asci 1 = 00110001
        // asci . = 00101110
        assert_eq!(format!("{:032b}", align), "00110001001011100011000000000000");
        let dot_to_zero = align &  0xFF00_FFFF;
        // value 0 = 00000000
        assert_eq!(format!("{:032b}", dot_to_zero), "00110001000000000011000000000000");
        let from_text_to_numbers = dot_to_zero & 0x0F00_0F0F;
        assert_eq!(format!("{:032b}", from_text_to_numbers), "00000001000000000000000000000000");
        let tens = from_text_to_numbers & 0xFF;
        assert_eq!(format!("{:032b}", tens), "00000000000000000000000000000000");
        let ones = (from_text_to_numbers >> 8) & 0xFF;
        assert_eq!(format!("{:032b}", ones), "00000000000000000000000000000000");
        let tenths = (from_text_to_numbers >> 24) & 0xFF;
        assert_eq!(format!("{:032b}", tenths), "00000000000000000000000000000001");
        let value = tens * 100 + ones * 10 + tenths;
        assert_eq!(value, 1);

        // zkus tohle pro 99.9
        let a = from_text_to_numbers.wrapping_mul(0x640A_0001);
        assert_eq!(format!("{:032b}", a), "00000001000000000000000000000000");

        assert_eq!(100u64 << 24, 0x6400_0000);
        assert_eq!(10u64 << 16, 0x000A_0000);
        assert_eq!((100u64 << 24) + (10u64 << 16) + 1, 0x640A_0001);

        // and
        // byte:                    6     5     4     3     2     1     0
        // digits * 1                                 3     0     2     1
        // digits * 10 << 16              30     0    20    10
        // digits * 100 << 24      300     0   200   100
        //                                          ---
        //                                          123   <- column of byte 3
        let value = (from_text_to_numbers.wrapping_mul(0x640A_0001) >> 24) & 0x3FF;
        assert_eq!(value, 1);
    }

    #[test]
    fn positive_or_negative_test() {
        assert_eq!(positive_or_negative("0.0;abcd".as_bytes()), 0);
        assert_eq!(positive_or_negative("10.0;abcd".as_bytes()), 0);
        assert_eq!(positive_or_negative("-10.0;abcd".as_bytes()), -1);
    }

    #[test]
    fn sign_complement_test() {
        assert_eq!(format!("{:08b}", 1i8), "00000001");
        assert_eq!(format!("{:08b}", 2i8), "00000010");
        assert_eq!(format!("{:08b}", -1i8), "11111111");
        assert_eq!(format!("{:08b}", -2i8), "11111110");
        assert_eq!(!1i8, -2);
        assert_eq!(!-1i8, 0);
        assert_eq!(!0i8, -1);
        assert_eq!(-1i8 as u8, 255);
    }

    #[test]
    fn arithmetic_shift_test() {
        assert_eq!((0b1100_0000u8 as i8) >> 1, 0b1110_0000u8 as i8);
        assert_eq!((0b0100_0000u8 as i8) >> 1, 0b0010_0000u8 as i8);
    }

    #[test]
    fn parse_temperature_test() {
        assert_eq!(parse_temperature(b"99.9"), 999);
        assert_eq!(parse_temperature(b"2.5"), 25);
        assert_eq!(parse_temperature(b"0.1"), 1);
        assert_eq!(parse_temperature(b"0.0"), 0);
        assert_eq!(parse_temperature(b"-0.1"), -1);
        assert_eq!(parse_temperature(b"-2.5"), -25);
        assert_eq!(parse_temperature(b"-99.9"), -999);
    }

    #[test]
    fn parse_temperature_swar_intro() {
        // 4th bit is 0 for . and -
        assert_eq!(format!("{:08b}", b'.'), "00101110");
        assert_eq!(format!("{:08b}", b'-'), "00101101");
        assert_eq!(format!("{:08b}", !b'-'), "11010010");

        assert_eq!(format!("{:08b}", !(b'-' as i8) << 3), "10010000");
        assert_eq!(format!("{:08b}", (!(b'-' as i8) << 3) >> 7), "11111111");

        // 4th bit is 1 for numbers from 0 to 9
        assert_eq!(format!("{:08b}", b'0'), "00110000");
        assert_eq!(format!("{:08b}", b'1'), "00110001");
        assert_eq!(format!("{:08b}", b'2'), "00110010");
        assert_eq!(format!("{:08b}", b'3'), "00110011");
        assert_eq!(format!("{:08b}", b'4'), "00110100");
        assert_eq!(format!("{:08b}", b'5'), "00110101");
        assert_eq!(format!("{:08b}", b'6'), "00110110");
        assert_eq!(format!("{:08b}", b'7'), "00110111");
        assert_eq!(format!("{:08b}", b'8'), "00111000");
        assert_eq!(format!("{:08b}", b'9'), "00111001");

        assert_eq!(format!("0x{:x}", b'.'), "0x2e");
        assert_eq!(format!("0x{:x}", b'-'), "0x2d");
        assert_eq!(format!("0x{:x}", b';'), "0x3b");
        assert_eq!(format!("0x{:x}", b'0'), "0x30");
        assert_eq!(format!("0x{:x}", b'1'), "0x31");
        assert_eq!(format!("0x{:x}", b'2'), "0x32");
        assert_eq!(format!("0x{:x}", b'3'), "0x33");
        assert_eq!(format!("0x{:x}", b'4'), "0x34");
        assert_eq!(format!("0x{:x}", b'5'), "0x35");
        assert_eq!(format!("0x{:x}", b'6'), "0x36");
        assert_eq!(format!("0x{:x}", b'7'), "0x37");
        assert_eq!(format!("0x{:x}", b'8'), "0x38");
        assert_eq!(format!("0x{:x}", b'9'), "0x39");

        assert_eq!(format!("{:08b}", b'1' - b'0'), "00000001");
        assert_eq!(format!("{:08b}", b'2' - b'0'), "00000010");
        assert_eq!(format!("{:08b}", b'3' - b'0'), "00000011");
        assert_eq!(format!("{:08b}", b'4' - b'0'), "00000100");
        assert_eq!(format!("{:08b}", b'5' - b'0'), "00000101");
        assert_eq!(format!("{:08b}", b'6' - b'0'), "00000110");
        assert_eq!(format!("{:08b}", b'7' - b'0'), "00000111");
        assert_eq!(format!("{:08b}", b'8' - b'0'), "00001000");
        assert_eq!(format!("{:08b}", b'9' - b'0'), "00001001");

        assert_eq!(format!("{:08b}", b'.'), "00101110");
        let dot_on_index_1 = u32::from_le_bytes("0.0;abcd"[..4].as_bytes().try_into().unwrap());
        assert_eq!(format!("{:032b}", dot_on_index_1), "00111011001100000010111000110000");
        assert_eq!(format!("{:032b}", dot_on_index_1 & 0b00000000_00000000_00101110_00000000u32), "00000000000000000010111000000000");
        assert_eq!(format!("{:032b}", dot_on_index_1 & 0b00000000_00000000_00101110_00000000u32), "00000000000000000010111000000000");

        let dot_on_index_2 = u32::from_le_bytes("10.0;abcd"[..4].as_bytes().try_into().unwrap());
        assert_eq!(format!("{:032b}", dot_on_index_2), "00110000001011100011000000110001");
        assert_eq!(format!("{:032b}", dot_on_index_2 & 0b00000000_00101110_00000000_00000000u32), "00000000001011100000000000000000");

        let dot_on_index_3 = u32::from_le_bytes("-10.0;abcd"[..4].as_bytes().try_into().unwrap());
        assert_eq!(format!("{:032b}", dot_on_index_3), "00101110001100000011000100101101");
        assert_eq!(format!("{:032b}", dot_on_index_3 & 0b00101110_00000000_00000000_00000000u32), "00101110000000000000000000000000");

        // unified mask: 0b00001000_00001000_00001000_00000000u32
        let masked_dot_on_index_1 = dot_on_index_1 & 0b00001000_00001000_00001000_00000000u32;
        assert_eq!(format!("{:032b}", masked_dot_on_index_1), "00001000000000000000100000000000");
        let masked_dot_on_index_2 = dot_on_index_2 & 0b00001000_00001000_00001000_00000000u32;
        assert_eq!(format!("{:032b}", masked_dot_on_index_2), "00000000000010000000000000000000");
        let masked_dot_on_index_3 = dot_on_index_3 & 0b00001000_00001000_00001000_00000000u32;
        assert_eq!(format!("{:032b}", masked_dot_on_index_3), "00001000000000000000000000000000");

        assert_eq!(masked_dot_on_index_1.trailing_zeros(), 11);
        assert_eq!(masked_dot_on_index_2.trailing_zeros(), 19);
        assert_eq!(masked_dot_on_index_3.trailing_zeros(), 27);
    }

    #[test]
    fn dot_index_test() {
        assert_eq!(dot_index("0.0;abcd".as_bytes()), 1);
        assert_eq!(dot_index("10.0;abcd".as_bytes()), 2);
        assert_eq!(dot_index("-10.0;abcd".as_bytes()), 3);
    }

    #[test]
    fn parse_temperature_swar_all_values_test() {
        for tenths in -999i16..=999 {
            let sign = if tenths < 0 { "-" } else { "" };
            let text = format!("{}{}.{}\nAbcdefgh", sign, tenths.abs() / 10, tenths.abs() % 10);
            let temperature = &text.as_bytes()[..text.find('\n').unwrap()];
            assert_eq!(parse_temperature(temperature), tenths, "{}", text);
            assert_eq!(parse_temperature_swar(text.as_bytes()), tenths, "{}", text);
        }
    }
}

pub struct StationTemperatureIterator<'a>{
    map: &'a [u8],
    at: usize,
    block_start: usize,
    delimiter_bit_mask: u64
}

impl <'a> StationTemperatureIterator<'a> {
    pub fn new(map: &'a [u8]) -> StationTemperatureIterator<'a> {
        Self {
            map,
            at: 0,
            block_start: 0,
            delimiter_bit_mask: 0
        }
    }
}

//TODO this code works with full pairs in the SIMD only so it can be optimized
// for example delimiter in first simd and newline in another simd

// one(): the rows of a chunk until this many bytes before its end go through next_fast
const FAST_MARGIN: usize = 128;

impl <'a> StationTemperatureIterator<'a> {
    // next without the checks of the end of the chunk, only while at + FAST_MARGIN <= map.len():
    // the block of 64 bytes from at, and the 8 bytes of the temperature, which starts within
    // 64 bytes of at, are then in the map
    #[inline(always)]
    fn next_fast(&mut self) -> (StationName<'a>, i16) {
        const DELIMITER: u8x64 = u8x64::splat(b';');

        if self.delimiter_bit_mask == 0 {
            self.block_start = self.at;
            let simd_value = unsafe { std::ptr::read_unaligned(self.map.as_ptr().add(self.at) as *const u8x64) };
            self.delimiter_bit_mask = DELIMITER.simd_eq(simd_value).to_bitmask();
        }
        let delimiter_index = self.delimiter_bit_mask.trailing_zeros() as usize;
        self.delimiter_bit_mask &= self.delimiter_bit_mask - 1;
        let delimiter = self.block_start + delimiter_index;
        let station_name = StationName{ rest: unsafe { self.map.get_unchecked(self.at..) }, len: delimiter - self.at};
        let word = unsafe { std::ptr::read_unaligned(self.map.as_ptr().add(delimiter + 1) as *const u64) };
        let (parsed_temperature, dot_index) = parse_temperature_word(u64::from_le(word));
        self.at = delimiter + dot_index + 4;
        (station_name, parsed_temperature)
    }
}

impl <'a> Iterator for StationTemperatureIterator<'a> {
    type Item = (StationName<'a>, i16);

    #[inline(always)]
    fn next(&mut self) -> Option<Self::Item> {
        const SIZE_64: usize = 64;
        const DELIMITER: u8x64 = u8x64::splat(b';');

        if self.at >= self.map.len() {
            return None;
        }

        if self.delimiter_bit_mask == 0 {
            self.block_start = self.at;
            let simd_value = if self.at + SIZE_64 <= self.map.len() {
                u8x64::from_slice(&self.map[self.at..self.at + SIZE_64])
            } else {
                u8x64::load_or_default(&self.map[self.at..self.map.len()])
            };
            let del_eq = DELIMITER.simd_eq(simd_value);
            self.delimiter_bit_mask = del_eq.to_bitmask();
        }
        let delimiter_index = self.delimiter_bit_mask.trailing_zeros() as usize;
        self.delimiter_bit_mask &= self.delimiter_bit_mask - 1;
        let delimiter = self.block_start + delimiter_index;
        let station_name = StationName{ rest: &self.map[self.at..], len: delimiter - self.at};
        // the ';' was found in the map (at the end of the chunk the mask has bits only for its bytes),
        // so delimiter < len and delimiter + 1 <= len
        let temperature = unsafe { self.map.get_unchecked(delimiter + 1..) };
        let (parsed_temperature, dot_index) = parse_temperature_swar_with_dot_index(temperature);
        self.at = delimiter + dot_index + 4;
        Some((station_name, parsed_temperature))
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
        assert_eq!(next.1, 167);

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
        assert_eq!(next.1, 167);

        next_opt = iterator.next();
        assert_eq!(next_opt.is_some(), true);
        next = next_opt.unwrap();
        assert_eq!(next.0.as_bytes(), b"Bata");
        assert_eq!(next.1, 150);

        next_opt = iterator.next();
        assert_eq!(next_opt.is_some(), true);
        next = next_opt.unwrap();
        assert_eq!(next.0.as_bytes(), b"Durban");
        assert_eq!(next.1, 122);

        next_opt = iterator.next();
        assert_eq!(next_opt.is_some(), true);
        next = next_opt.unwrap();
        assert_eq!(next.0.as_bytes(), b"Vientiane");
        assert_eq!(next.1, 139);

        next_opt = iterator.next();
        assert_eq!(next_opt.is_some(), true);
        next = next_opt.unwrap();
        assert_eq!(next.0.as_bytes(), b"Karachi");
        assert_eq!(next.1, 126);

        next_opt = iterator.next();
        assert_eq!(next_opt.is_some(), true);
        next = next_opt.unwrap();
        assert_eq!(next.0.as_bytes(), b"Durban");
        assert_eq!(next.1, 134);

        next_opt = iterator.next();
        assert_eq!(next_opt.is_some(), true);
        next = next_opt.unwrap();
        assert_eq!(next.0.as_bytes(), b"Launceston");
        assert_eq!(next.1, -34);

        next_opt = iterator.next();
        assert_eq!(next_opt.is_some(), true);
        next = next_opt.unwrap();
        assert_eq!(next.0.as_bytes(), b"Bishkek");
        assert_eq!(next.1, 159);

        next_opt = iterator.next();
        assert_eq!(next_opt.is_some(), true);
        next = next_opt.unwrap();
        assert_eq!(next.0.as_bytes(), b"Kuwait City");
        assert_eq!(next.1, 393);

        next_opt = iterator.next();
        assert_eq!(next_opt.is_some(), true);
        next = next_opt.unwrap();
        assert_eq!(next.0.as_bytes(), b"Valletta");
        assert_eq!(next.1, 189);

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
        let measurement_stats = &mut stats_array[station.len];
        measurement_stats.update(temperature);
    }
    stats_array
}

#[inline(always)]
fn one(map: &[u8]) -> Vec<(&[u8], MeasurementStats)> {
    let mut stats = StationTable::new();
    let mut rows = StationTemperatureIterator::new(map);
    // all rows but the last ~128 bytes of the chunk without the checks of its end
    let fast_end = map.len().saturating_sub(FAST_MARGIN);
    while rows.at < fast_end {
        let (station, temperature) = rows.next_fast();
        stats.get_mut_fast(station).update(temperature);
    }
    // the last rows with them
    for (station, temperature) in rows {
        stats.get_mut(station).update(temperature);
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

pub const SLOT_BITS: usize = 14;
//14 => 16_384 so it will cover the max 10K station names from 1BRC rules
pub const SLOTS: usize = 1 << SLOT_BITS;
pub const SLOT_MASK: usize = SLOTS - 1;

pub fn key_words(station: &StationName) -> (u64, u64) {
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

// first 32 of FF and last 32 of 0
static KEY_MASK_BYTES: [[u8; 32]; 2] = [[0xFF; 32], [0; 32]];

// 128 of FF and 32 of 0: the 32 bytes from 128 - len have FF in the first min(len, 32) of them,
// for every len from 1 to 128, so the window needs no min; the rules allow names of up to 100 bytes
const MAX_NAME_LEN: usize = 128;
static KEY_MASK_WINDOWS: [u8; MAX_NAME_LEN + 32] = {
    let mut bytes = [0u8; MAX_NAME_LEN + 32];
    let mut i = 0;
    while i < MAX_NAME_LEN {
        bytes[i] = 0xFF;
        i += 1;
    }
    bytes
};

#[inline(always)]
pub fn key(station: &StationName) -> u8x32 {
    let in_key = station.len.min(32);
    let mask = u8x32::from_slice(&KEY_MASK_BYTES.as_flattened()[32 - in_key..64 - in_key]);
    let bytes = if station.rest.len() < 32 {
        u8x32::load_or_default(station.rest)
    } else {
        u8x32::from_slice(&station.rest[..32])
    };
    bytes & mask
}

#[inline(always)]
pub fn key_hash_words(key: u8x32) -> (u64, u64) {
    let key_array = key.to_array();
    let word_1 = u64::from_le_bytes(key_array[..8].try_into().unwrap());
    let word_2 = u64::from_le_bytes(key_array[8..16].try_into().unwrap());
    (word_1, word_2)
}

#[cfg(test)]
mod key_words_tests {
    use std::simd::u8x32;
    use super::{key_words, key, key_hash_words, StationName};

    #[test]
    fn key_hash_words_test() {
        let data = b"Petropavlovsk-Kamchatsky;1.0\nBata;15.0\n";
        let simd_data = u8x32::from_slice(&data[..32]);
        let (word_1, word_2) = key_hash_words(simd_data);
        assert_eq!(word_1, u64::from_le_bytes(*b"Petropav"));
        assert_eq!(word_2, u64::from_le_bytes(*b"lovsk-Ka"));
    }

    #[test]
    fn smaller_key_test() {
        let data = b"Petropavlovsk-Kamchatsky;1.0\nBata;15.0\n";
        let station = StationName { rest: &data[..], len: 24 };
        let result = key(&station).to_array();
        assert_eq!(&result[..24], b"Petropavlovsk-Kamchatsky");
        assert!(result[24..].iter().all(|&b| b == 0));
    }

    #[test]
    fn bigger_key_test() {
        let data =  b"Llanfairpwllgwyngyllgogerychwyrndrobwll;1.0\n";
        let station = StationName { rest: &data[..], len: 39 };
        let result = key(&station).to_array();
        assert_eq!(&result[..], &b"Llanfairpwllgwyngyllgogerychwyrndrobwll"[..32]);
    }

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

// the first 8 bytes of the name are unique for 412 of the 413 names, and the two that share them
// (Alexandra, Alexandria) differ in length; the multiplier is the first one from splitmix64 with
// no collision on the 413 names (analyze-collision-hashtable, find_first_word_constant)
#[inline(always)]
pub fn hash(word_1: u64, len: usize) -> usize {
    let multiplied_by_magic_constant = (word_1 ^ len as u64).wrapping_mul(0x4BF2_D035_8334_1075);
    (multiplied_by_magic_constant as usize) >> (64 - SLOT_BITS)
}

// the first 8 bytes of the key: the low lane of its register, one move
#[inline(always)]
pub fn key_first_word(key: u8x32) -> u64 {
    u64::from_le_bytes(key.to_array()[..8].try_into().unwrap())
}

pub fn hash_analyse(word_1: u64, word_2: u64, rotate: u32, magic_const: u64) -> usize {
    // reduce overlapping by rotating second word
    let words = word_1 ^ word_2.rotate_left(rotate);
    let multiplied_by_magic_constant = words.wrapping_mul(magic_const);
    //reduce to numbers from 0 to 2_047
    (multiplied_by_magic_constant as usize) >> (64 - SLOT_BITS)
}

#[cfg(test)]
mod hash_tests {
    use super::*;

    #[test]
    fn test_hash() {
        assert!(hash(0, 0) < SLOTS);
        assert!(hash(u64::MAX, 100) < SLOTS);
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
            let (word1, _) = key_words(&station);
            hash(word1, station.len)
        };
        let hash_2 = {
            let data = b"Bata;17.0\n";
            let station = StationName { rest: &data[..], len: 4 };
            let (word1, _) = key_words(&station);
            hash(word1, station.len)
        };
        assert_eq!(hash_1, hash_2);
    }
}

// one cpu cache line per slot to reduce slower split load
#[derive(Debug)]
#[repr(C, align(64))]
pub struct Slot {
    key: u8x32,
    stats: MeasurementStats,
}

impl Default for Slot {
    fn default() -> Self {
        Self {
            key: u8x32::splat(0),
            stats: MeasurementStats::default(),
        }
    }
}

// 2MB page size
#[repr(C, align(2_097_152))]
pub struct HugePageAligned<T>(pub T);

pub struct StationTable<'a> {
    pub slots: Box<HugePageAligned<[Slot; SLOTS]>>,
    pub names: Box<[&'a [u8]; SLOTS]>,
}

impl <'a> StationTable<'a> {
    pub fn new() -> Self {
        // allocated, nothing written yet, so no 2 MiB on the stack
        let mut slots = Box::<HugePageAligned<[Slot; SLOTS]>>::new_uninit();
        let ptr = slots.as_mut_ptr();
        unsafe {
            // before the first write: the page fault then gets a 2 MiB page
            #[cfg(target_os = "linux")]
            if libc::madvise(ptr as *mut libc::c_void, size_of::<HugePageAligned<[Slot; SLOTS]>>(), libc::MADV_HUGEPAGE) != 0 {
                panic!("madvise MADV_HUGEPAGE failed: {:?}", std::io::Error::last_os_error());
            }
            for i in 0..SLOTS {
                (&raw mut (*ptr).0[i]).write(Slot::default());
            }
        }
        Self {
            slots: unsafe { slots.assume_init() },
            // this way it's not created on stack before boxing
            names: (0..SLOTS).map(|_| &[][..])
                .collect::<Vec<_>>()
                .into_boxed_slice()
                .try_into()
                .unwrap(),
        }
    }

    #[inline(always)]
    pub fn get_mut(&mut self, station_name: StationName<'a>) -> &mut MeasurementStats {
        self.get_mut_with_key(station_name, key(&station_name))
    }

    // the key without the check of the end of the chunk: station_name.rest has at least 32 bytes
    #[inline(always)]
    fn get_mut_fast(&mut self, station_name: StationName<'a>) -> &mut MeasurementStats {
        // one subtraction for the start of the window instead of a min and a subtraction, on the
        // way from the length of the name to the hash and the slot
        debug_assert!(station_name.len >= 1 && station_name.len <= MAX_NAME_LEN);
        let mask = unsafe {
            std::ptr::read_unaligned(KEY_MASK_WINDOWS.as_ptr().add(MAX_NAME_LEN - station_name.len) as *const u8x32)
        };
        let bytes = unsafe { std::ptr::read_unaligned(station_name.rest.as_ptr() as *const u8x32) };
        self.get_mut_with_key(station_name, bytes & mask)
    }

    #[inline(always)]
    fn get_mut_with_key(&mut self, station_name: StationName<'a>, key: u8x32) -> &mut MeasurementStats {
        let mut index = hash(key_first_word(key), station_name.len);
        loop {
            if self.slots.0[index].key == key && (station_name.len <= 32 || self.names[index] == station_name.as_bytes()) {
                return &mut self.slots.0[index].stats;
            } else if self.slots.0[index].key == u8x32::splat(0) {
                self.slots.0[index].key = key;
                self.names[index] = station_name.as_bytes();
                return &mut self.slots.0[index].stats;
            } else {
                index = (index + 1) & SLOT_MASK;
            }
        }
    }

    pub fn get_mut_analyse(&mut self, station_name: StationName<'a>, rotate: u32, magic_const: u64) -> (&mut MeasurementStats, usize, usize) {
        let key = key(&station_name);
        let (word_1, word_2) = key_hash_words(key);
        let mut index = hash_analyse(word_1, word_2, rotate, magic_const);
        let mut extra_loop = 0;
        loop {
            if self.slots.0[index].key == key && (station_name.len <= 32 || self.names[index] == station_name.as_bytes()) {
                return (&mut self.slots.0[index].stats, index, extra_loop);
            } else if self.slots.0[index].key == u8x32::splat(0) {
                self.slots.0[index].key = key;
                self.names[index] = station_name.as_bytes();
                return (&mut self.slots.0[index].stats, index, extra_loop);
            } else {
                index = (index + 1) & SLOT_MASK;
                extra_loop += 1;
            }
        }
    }

    fn into_entries(self) -> Vec<(&'a [u8], MeasurementStats)> {
        self.names.iter().zip(self.slots.0.iter())
            .filter(|(_, slot)| slot.key != u8x32::splat(0))
            .map(|(name, slot)| (*name, slot.stats))
            .collect()
    }
}

#[cfg(test)]
mod station_table_tests {
    use super::*;

    #[test]
    fn key_mask_windows_test() {
        // the window without min is the same mask as the one with min, for every length
        for len in 1..=MAX_NAME_LEN {
            let without_min = &KEY_MASK_WINDOWS[MAX_NAME_LEN - len..MAX_NAME_LEN - len + 32];
            let in_key = len.min(32);
            let with_min = &KEY_MASK_BYTES.as_flattened()[32 - in_key..64 - in_key];
            assert_eq!(without_min, with_min, "len {}", len);
        }
    }

    #[test]
    fn one_fast_and_last_rows_test() {
        let names = ["A", "Kuwait City", "Bata", "Petropavlovsk-Kamchatsky", "Llanfairpwllgwyngyllgogerychwyrndrobwll", "Ab"];
        let temperatures = ["-99.9", "0.0", "12.3", "-1.5", "99.9", "5.0", "-12.3"];
        // shorter than FAST_MARGIN (only the checked rows), and longer (both)
        for lines in [0, 1, 2, 3, 10, 301, 302] {
            let mut data = String::new();
            for i in 0..lines {
                data.push_str(&format!("{};{}\n", names[i % names.len()], temperatures[i % temperatures.len()]));
            }

            // the same with only the checked iterator
            let mut expected = StationTable::new();
            for (station, temperature) in StationTemperatureIterator::new(data.as_bytes()) {
                expected.get_mut(station).update(temperature);
            }
            let as_tuples = |entries: Vec<(&[u8], MeasurementStats)>| {
                let mut tuples: Vec<_> = entries.into_iter()
                    .map(|(name, stats)| (name.to_vec(), stats.min, stats.max, stats.sum, stats.count))
                    .collect();
                tuples.sort();
                tuples
            };
            assert_eq!(as_tuples(one(data.as_bytes())), as_tuples(expected.into_entries()), "{} lines", lines);
        }
    }

    #[test]
    fn into_entries_empty_test() {
        let table = StationTable::new();
        assert!(table.into_entries().is_empty());
    }

    #[test]
    fn new_test() {
        let station_table = StationTable::new();
        assert_eq!(station_table.slots.0.len(), SLOTS);
        assert_eq!(station_table.names.len(), SLOTS);
        assert!(station_table.slots.0.iter().all(|s| s.key == u8x32::splat(0)));
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
