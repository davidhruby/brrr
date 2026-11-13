use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::File;
use std::io::BufRead;
use std::os::fd::AsRawFd;
use brrr::app::{IdealHasherBuilder, MeasurementStats, StationName, StationTemperatureIterator};

const KB: usize = 1024;
const MACOS_PAGE_SIZE: usize = 16 * KB;


fn main() {
    let file = File::open("input/1B_unique_station_names.txt").unwrap();
    let mut station_names = HashSet::new();
    for line in std::io::BufReader::new(file).lines() {
        station_names.insert(line.unwrap());
    }

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

    let mut stats: HashMap<StationName, MeasurementStats, IdealHasherBuilder> = HashMap::with_capacity_and_hasher(10_000, IdealHasherBuilder);
    let first_station_name_pointer = m_map_ptr;
    let mut last_station_name_pointer = None;
    for (station, _) in StationTemperatureIterator::new(m_map) {
        let station_name = unsafe { std::str::from_utf8_unchecked(&station.rest[..station.len]) };
        station_names.remove(station_name);
        if last_station_name_pointer.is_none() && station_names.is_empty() {
            last_station_name_pointer = Some(station.rest.as_ptr());
        }

        match stats.get_mut(&station) {
            Some(stat) => stat,
            None => stats.entry(station).or_insert(MeasurementStats::default())
        };
    }

    let first_station_name_pointer = first_station_name_pointer as usize;
    println!("First station name pointer: {} ({:#X})", first_station_name_pointer, first_station_name_pointer);

    let last_station_name_pointer = last_station_name_pointer.unwrap() as usize;
    println!("Last station name pointer: {} ({:#X})", last_station_name_pointer, last_station_name_pointer);

    let address_diff = last_station_name_pointer - first_station_name_pointer;
    let address_diff_pages = address_diff as f32 / MACOS_PAGE_SIZE as f32;
    println!("Address diff: {} (pages: {})", address_diff, address_diff_pages);

    let mut os_page_index_block_count = BTreeMap::<usize, u16>::new();
    let mut m_map_page_index_block_count = BTreeMap::<usize, u16>::new();
    for key in stats.keys() {
        let pointer = key.as_bytes().as_ptr() as usize;
        let page_index = pointer / MACOS_PAGE_SIZE;
        os_page_index_block_count.entry(page_index)
            .and_modify(|count| *count += 1)
            .or_insert(1);

        let m_map_index = unsafe { key.as_bytes().as_ptr().offset_from_unsigned(m_map_ptr as *const u8) };
        let m_map_page = m_map_index / MACOS_PAGE_SIZE;
        m_map_page_index_block_count.entry(m_map_page)
            .and_modify(|count| *count += 1)
            .or_insert(1);
    }

    println!("Page index with number of keys:");
    for (page_index, count) in os_page_index_block_count {
        println!("{page_index} => {count}");
    }
    println!("MMap index with number of keys:");
    for (map_index, count) in m_map_page_index_block_count {
        println!("{map_index} => {count}");
    }
}