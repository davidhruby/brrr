#![feature(portable_simd)]

// this challenge has a rule that all code has to be in one file
use std::{env, thread};
use std::collections::{BTreeMap, HashMap};
use std::ffi::c_void;
use std::fs::File;
use std::os::fd::AsRawFd;
use std::simd::u8x64;
use std::simd::prelude::SimdPartialEq;
use std::sync::mpsc;
use std::sync::mpsc::Sender;

const KB: usize = 1024;
const MB: usize = 1024 * KB;
const M_ADVISE_SIZE: usize = 512 * MB;
const MACOS_PAGE_SIZE: usize = 16 * KB;

pub struct MeasurementStats {
    min: i16,
    max: i16,
    sum: i64,
    count: u32
}

impl MeasurementStats {
    pub fn update(&mut self, measurement: i16) {
        self.min = self.min.min(measurement);
        self.max = self.max.max(measurement);
        self.sum += i64::from(measurement);
        self.count += 1;
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

struct MMemMessage {
    ptr: *const u8,
    len: usize,
    do_not_need_previous_block: bool,
}

// SAFETY: ptr points into the mmap region, which lives for the whole run of the program,
// and the helper thread only reads from it.
unsafe impl Send for MMemMessage {}

impl MMemMessage {
    // ptr is next block
    // => m_m_message.ptr - M_ADVISE_SIZE is the block currently being processed
    // => m_m_message.ptr - 2 * M_ADVISE_SIZE is previous block
    fn do_not_need_block_ptr(&self) -> *const u8 {
        unsafe { self.ptr.sub( 2 * M_ADVISE_SIZE) }
    }
}

#[cfg(test)]
mod m_mem_message_tests {
    use super::*;
    #[test]
    fn test_do_not_need_block_ptr() {
        let m_mem_message = MMemMessage {
            ptr: std::ptr::without_provenance(0x5000_0000),
            len: 10,
            do_not_need_previous_block: false,
        };
        // 2 * 512 MB = 0x4000_0000
        assert_eq!(m_mem_message.do_not_need_block_ptr(), std::ptr::without_provenance(0x1000_0000));
    }
}

struct MMapHelper {
    ptr: *mut c_void,
    file_len: usize,
}

// all libc mmap / madvise calls live here
impl MMapHelper {
    // we cannot use external dependency therefore this way copied from memmap2 lib
    fn new(file: &File, file_len: usize) -> Self {
        // SAFETY: creating a new memory map with a nullptr as address hint is always sound:
        // it does not modify any existing mapping or memory contents.
        unsafe {
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
                Self { ptr, file_len }
            }
        }
    }

    fn as_slice(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.ptr as *const u8, self.file_len) }
    }

    // preload next block into RAM
    fn pre_load_next_block(m_m_message: &MMemMessage) {
        if unsafe { libc::madvise(m_m_message.ptr as *mut c_void, m_m_message.len as libc::size_t, libc::MADV_WILLNEED) } != 0 {
            panic!("madvise MADV_WILLNEED failed: {:?}", std::io::Error::last_os_error());
        }

        // read first byte from each page to have all the block pages loaded into RAM
        let page_count = m_m_message.len / (MACOS_PAGE_SIZE);
        for page_index in 0..page_count {
            let byte_position = unsafe { m_m_message.ptr.add(page_index * MACOS_PAGE_SIZE) };
            unsafe { std::ptr::read_volatile(byte_position) };
        }
    }

    fn mark_previous_block_not_needed(m_m_message: &MMemMessage) {
        if m_m_message.do_not_need_previous_block {
            // mark previous RAM block as not needed
            if unsafe { libc::madvise(m_m_message.do_not_need_block_ptr() as *mut c_void, M_ADVISE_SIZE as libc::size_t, libc::MADV_DONTNEED) } != 0 {
                panic!("madvise MADV_DONTNEED failed: {:?}", std::io::Error::last_os_error());
            }
        }
    }
}

struct NextBlockPreLoadInfo {
    next_m_advise_ptr_opt: Option<*const u8>,
    map_end_ptr: *const u8,
    m_advise_size: usize,
    m_advise_size_threshold: usize,
    do_not_need_previous_block: bool,
}

impl NextBlockPreLoadInfo {
    fn new(map_start_ptr: *const u8, file_len: usize, m_advise_size: usize) -> Self {
        Self {
            next_m_advise_ptr_opt: Some(map_start_ptr),
            map_end_ptr: unsafe { map_start_ptr.add(file_len) },
            m_advise_size,
            m_advise_size_threshold: m_advise_size / 2,
            do_not_need_previous_block: false,
        }
    }

    fn request_first_block_load(&mut self, tx: &Sender<MMemMessage>) {
        if let Some(next_m_advise_ptr) = self.next_m_advise_ptr_opt {
            tx.send(MMemMessage { ptr: next_m_advise_ptr, len: self.m_advise_size, do_not_need_previous_block: self.do_not_need_previous_block }).unwrap();
            self.next_m_advise_ptr_opt = unsafe { Some(next_m_advise_ptr.add(self.m_advise_size)) };
        }
    }

    fn request_next_block_load(&mut self, tx: &Sender<MMemMessage>, line_ptr: *const u8) {
        if let Some(next_m_advise_ptr) = self.next_m_advise_ptr_opt {
            if unsafe { next_m_advise_ptr.offset_from_unsigned(line_ptr) } < self.m_advise_size_threshold {
                let remaining_bytes_len = unsafe { self.map_end_ptr.offset_from_unsigned(next_m_advise_ptr) };

                // are fewer bytes left than a full block?
                if remaining_bytes_len <= self.m_advise_size {
                    tx.send(MMemMessage { ptr: next_m_advise_ptr, len: remaining_bytes_len, do_not_need_previous_block: self.do_not_need_previous_block }).unwrap();
                    self.next_m_advise_ptr_opt = None;
                } else {
                    tx.send(MMemMessage { ptr: next_m_advise_ptr, len: self.m_advise_size, do_not_need_previous_block: self.do_not_need_previous_block }).unwrap();
                    self.next_m_advise_ptr_opt = unsafe { Some(next_m_advise_ptr.add(self.m_advise_size)) };
                }
                // this way madvise MADV_DONTNEED will be true from 3rd MMemMessage
                // if this block is executed during first block processing we don't have previous block to mark as MADV_DONTNEED
                // if this block is executed during second or next block processing we are able to mark previous block as MADV_DONTNEED
                self.do_not_need_previous_block = true;
            }
        }
    }
}

#[cfg(test)]
mod next_block_pre_load_tests {
    use super::*;
    #[test]
    fn test_new() {
        let next_block_pre_load_info = NextBlockPreLoadInfo::new(
            std::ptr::without_provenance(0x5000_0000),
            0x1000,
            M_ADVISE_SIZE
        );
        assert_eq!(next_block_pre_load_info.next_m_advise_ptr_opt, Some(std::ptr::without_provenance(0x5000_0000)));
        assert_eq!(next_block_pre_load_info.map_end_ptr, std::ptr::without_provenance(0x5000_1000));
        assert_eq!(next_block_pre_load_info.do_not_need_previous_block, false);
    }

    #[test]
    fn test_request_first_block_load_with_remaining_bytes_lower_than_m_advise_size() {
        let mut next_block_pre_load_info = NextBlockPreLoadInfo::new(
            std::ptr::without_provenance(0x5000_0000),
            0x1000,
            M_ADVISE_SIZE
        );

        let (tx, rx) = mpsc::channel::<MMemMessage>();
        next_block_pre_load_info.request_first_block_load(&tx);

        let received_messages = rx.try_iter().collect::<Vec<MMemMessage>>();
        assert_eq!(received_messages.len(), 1);

        let received_m_mem_message = received_messages.get(0).unwrap();
        assert_eq!(received_m_mem_message.ptr, std::ptr::without_provenance(0x5000_0000));
        // M_ADVISE_SIZE = 512 MB = 2²⁹ = 0x2000_0000
        assert_eq!(received_m_mem_message.len, 0x2000_0000);
        assert_eq!(received_m_mem_message.do_not_need_previous_block, false);

        // M_ADVISE_SIZE = 512 MB = 2²⁹ = 0x2000_0000
        // 0x5000_0000 + 0x2000_0000 = 0x7000_0000
        assert_eq!(next_block_pre_load_info.next_m_advise_ptr_opt, Some(std::ptr::without_provenance(0x7000_0000)));
        assert_eq!(next_block_pre_load_info.map_end_ptr, std::ptr::without_provenance(0x5000_1000));
        assert_eq!(next_block_pre_load_info.do_not_need_previous_block, false);
    }

    #[test]
    fn test_request_first_block_load_with_none_pointer() {
        let mut next_block_pre_load_info = NextBlockPreLoadInfo {
            next_m_advise_ptr_opt: None,
            map_end_ptr: std::ptr::without_provenance(0x1000),
            m_advise_size: 1024,
            m_advise_size_threshold: 512,
            do_not_need_previous_block: false,
        };

        let (tx, rx) = mpsc::channel::<MMemMessage>();
        next_block_pre_load_info.request_first_block_load(&tx);

        let received_messages = rx.try_iter().collect::<Vec<MMemMessage>>();
        assert_eq!(received_messages.is_empty(), true);

        assert_eq!(next_block_pre_load_info.next_m_advise_ptr_opt, None);
        assert_eq!(next_block_pre_load_info.map_end_ptr, std::ptr::without_provenance(0x1000));
        assert_eq!(next_block_pre_load_info.do_not_need_previous_block, false);
    }

    #[test]
    fn test_request_next_block_load_with_remaining_bytes_bigger_than_m_advise_size() {
        let data = vec![0u8; 2048];

        let mut next_block_pre_load_info = NextBlockPreLoadInfo {
            next_m_advise_ptr_opt: Some(unsafe { data.as_ptr().offset(512) }),
            map_end_ptr: unsafe { data.as_ptr().offset(data.len() as isize) },
            m_advise_size: 1024,
            m_advise_size_threshold: 512,
            do_not_need_previous_block: false,
        };

        let (tx, rx) = mpsc::channel::<MMemMessage>();
        let line_ptr = unsafe { data.as_ptr().offset(10) };
        next_block_pre_load_info.request_next_block_load(&tx, line_ptr);

        let received_messages = rx.try_iter().collect::<Vec<MMemMessage>>();
        assert_eq!(received_messages.len(), 1);

        let received_m_mem_message = received_messages.get(0).unwrap();
        assert_eq!(received_m_mem_message.ptr, unsafe { data.as_ptr().offset(512) });
        assert_eq!(received_m_mem_message.len, 1024);
        assert_eq!(received_m_mem_message.do_not_need_previous_block, false);

        assert_eq!(next_block_pre_load_info.next_m_advise_ptr_opt, Some(unsafe { data.as_ptr().offset(512 + 1024) }));
        assert_eq!(next_block_pre_load_info.map_end_ptr, unsafe { data.as_ptr().offset(data.len() as isize) });
        assert_eq!(next_block_pre_load_info.m_advise_size, 1024);
        assert_eq!(next_block_pre_load_info.m_advise_size_threshold, 512);
        assert_eq!(next_block_pre_load_info.do_not_need_previous_block, true);
    }

    #[test]
    fn test_request_next_block_load_with_remaining_bytes_lower_than_m_advise_size() {
        let data = vec![0u8; 2048];

        let mut next_block_pre_load_info = NextBlockPreLoadInfo {
            next_m_advise_ptr_opt: Some(unsafe { data.as_ptr().offset(1_540) }),
            map_end_ptr: unsafe { data.as_ptr().offset(data.len() as isize) },
            m_advise_size: 1024,
            m_advise_size_threshold: 512,
            do_not_need_previous_block: false,
        };

        let (tx, rx) = mpsc::channel::<MMemMessage>();
        let line_ptr = unsafe { data.as_ptr().offset(1_300) };
        next_block_pre_load_info.request_next_block_load(&tx, line_ptr);

        let received_messages = rx.try_iter().collect::<Vec<MMemMessage>>();
        assert_eq!(received_messages.len(), 1);

        let received_m_mem_message = received_messages.get(0).unwrap();
        assert_eq!(received_m_mem_message.ptr, unsafe { data.as_ptr().offset(1_540) });
        assert_eq!(received_m_mem_message.len, 2048 - 1_540);
        assert_eq!(received_m_mem_message.do_not_need_previous_block, false);

        assert_eq!(next_block_pre_load_info.next_m_advise_ptr_opt, None);
        assert_eq!(next_block_pre_load_info.map_end_ptr, unsafe { data.as_ptr().offset(data.len() as isize) });
        assert_eq!(next_block_pre_load_info.m_advise_size, 1024);
        assert_eq!(next_block_pre_load_info.m_advise_size_threshold, 512);
        assert_eq!(next_block_pre_load_info.do_not_need_previous_block, true);
    }

    #[test]
    fn test_request_next_block_load_with_none_pointer() {
        let mut next_block_pre_load_info = NextBlockPreLoadInfo {
            next_m_advise_ptr_opt: None,
            map_end_ptr: std::ptr::without_provenance(0x1000),
            do_not_need_previous_block: false,
            m_advise_size: 1024,
            m_advise_size_threshold: 512
        };

        let (tx, rx) = mpsc::channel::<MMemMessage>();
        next_block_pre_load_info.request_next_block_load(&tx, std::ptr::without_provenance(0x10));

        let received_messages = rx.try_iter().collect::<Vec<MMemMessage>>();
        assert_eq!(received_messages.is_empty(), true);

        assert_eq!(next_block_pre_load_info.next_m_advise_ptr_opt, None);
        assert_eq!(next_block_pre_load_info.map_end_ptr, std::ptr::without_provenance(0x1000));
        assert_eq!(next_block_pre_load_info.m_advise_size, 1024);
        assert_eq!(next_block_pre_load_info.m_advise_size_threshold, 512);
        assert_eq!(next_block_pre_load_info.do_not_need_previous_block, false);
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
    type Item = (&'a [u8], &'a [u8]);

    fn next(&mut self) -> Option<Self::Item> {
        const SIZE_64: usize = 64;
        const DELIMITER: u8x64 = u8x64::splat(b';');
        const END_LINE: u8x64 = u8x64::splat(b'\n');


        if self.newline_bit_mask == 0 {
            //file ends with empty line
            if self.at == self.map.len() {
                return None;
            }
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
        let temperature = &self.map[self.block_start + delimiter_index + 1..self.block_start + newline_index];
        self.at = self.block_start + newline_index + 1;
        Some((station, temperature))
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
        assert_eq!(next.0, b"Chihuahua");
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
        assert_eq!(next.0, b"Chihuahua");
        assert_eq!(next.1, b"16.7");

        next_opt = iterator.next();
        assert_eq!(next_opt.is_some(), true);
        next = next_opt.unwrap();
        assert_eq!(next.0, b"Bata");
        assert_eq!(next.1, b"15.0");

        next_opt = iterator.next();
        assert_eq!(next_opt.is_some(), true);
        next = next_opt.unwrap();
        assert_eq!(next.0, b"Durban");
        assert_eq!(next.1, b"12.2");

        next_opt = iterator.next();
        assert_eq!(next_opt.is_some(), true);
        next = next_opt.unwrap();
        assert_eq!(next.0, b"Vientiane");
        assert_eq!(next.1, b"13.9");

        next_opt = iterator.next();
        assert_eq!(next_opt.is_some(), true);
        next = next_opt.unwrap();
        assert_eq!(next.0, b"Karachi");
        assert_eq!(next.1, b"12.6");

        next_opt = iterator.next();
        assert_eq!(next_opt.is_some(), true);
        next = next_opt.unwrap();
        assert_eq!(next.0, b"Durban");
        assert_eq!(next.1, b"13.4");

        next_opt = iterator.next();
        assert_eq!(next_opt.is_some(), true);
        next = next_opt.unwrap();
        assert_eq!(next.0, b"Launceston");
        assert_eq!(next.1, b"-3.4");

        next_opt = iterator.next();
        assert_eq!(next_opt.is_some(), true);
        next = next_opt.unwrap();
        assert_eq!(next.0, b"Bishkek");
        assert_eq!(next.1, b"15.9");

        next_opt = iterator.next();
        assert_eq!(next_opt.is_some(), true);
        next = next_opt.unwrap();
        assert_eq!(next.0, b"Kuwait City");
        assert_eq!(next.1, b"39.3");

        next_opt = iterator.next();
        assert_eq!(next_opt.is_some(), true);
        next = next_opt.unwrap();
        assert_eq!(next.0, b"Valletta");
        assert_eq!(next.1, b"18.9");

        next_opt = iterator.next();
        assert_eq!(next_opt.is_none(), true);
    }
}


fn main() {
    let args: Vec<String> = env::args().collect();
    let input_file_path = &args[1];

    let (tx, rx) = mpsc::channel::<MMemMessage>();

    thread::spawn(move || {
        for m_m_message in rx {
            MMapHelper::mark_previous_block_not_needed(&m_m_message);
            MMapHelper::pre_load_next_block(&m_m_message);
        }
    });

    let file = File::open(input_file_path).unwrap();
    let file_len: usize = file.metadata().unwrap().len() as usize;
    let m_m_helper = MMapHelper::new(&file, file_len);
    let map = m_m_helper.as_slice();

    let mut next_block_pre_load_info = NextBlockPreLoadInfo::new(map.as_ptr(), file_len, M_ADVISE_SIZE);
    next_block_pre_load_info.request_first_block_load(&tx);

    let mut stats: HashMap<Vec<u8>, MeasurementStats> = HashMap::with_capacity(10_000);
    for (station, temperature) in StationTemperatureIterator::new(map) {
        // trigger next block preload into RAM if needed
        next_block_pre_load_info.request_next_block_load(&tx, station.as_ptr());

        let t = parse_temperature(temperature);

        let stat = match stats.get_mut(station) {
            Some(stat) => stat,
            None => stats.entry(station.to_vec()).or_insert(MeasurementStats::default())
        };
        stat.update(t);
    }

    let stats = BTreeMap::from_iter(stats.into_iter().map(|(k, v)| (unsafe { String::from_utf8_unchecked(k) }, v)));
    for (location, MeasurementStats { min, max, sum, count}) in stats.iter() {
        println!(
            "{location}={:.1}/{:.1}/{:.1}",
            (*min as f64) / 10.,
            (*sum as f64) / 10. / (*count as f64),
            (*max as f64) / 10.
        );
    }
}
