use std::{env, str};
use std::collections::{BTreeMap, HashMap};
use std::ffi::c_void;
use std::fs::File;
use std::os::fd::AsRawFd;

struct MeasurementStats {
    min: f32,
    max: f32,
    sum: f32,
    count: u32
}

impl MeasurementStats {
    fn update(&mut self, measurement: f32) {
        self.min = self.min.min(measurement);
        self.max = self.max.max(measurement);
        self.sum += measurement;
        self.count += 1;
    }
}

impl Default for MeasurementStats {
    fn default() -> Self {
        MeasurementStats {
            min: f32::MAX,
            max: f32::MIN,
            sum: 0f32,
            count: 0u32
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let input_file_path = &args[1];

    let file = File::open(input_file_path).unwrap();
    let file_len: usize = file.metadata().unwrap().len() as usize;
    let m_advise_size: usize = 512 * 1024 * 1024;
    let mut m_adivse_block_index = 0;
    let when_to_start_m_advise = m_advise_size / 2;
    let ptr = mmap(&file, file_len);

    if unsafe { libc::madvise(ptr, m_advise_size as libc::size_t, libc::MADV_WILLNEED) } != 0 {
        panic!("madvise  failed: {:?}", std::io::Error::last_os_error());
    }
    let map = unsafe { std::slice::from_raw_parts(ptr as *const u8, file_len) };
    let map_start_byte = map.as_ptr() as usize;
    let mut next_m_advise_byte = map_start_byte + m_advise_size;
    let map_end_byte = map_start_byte + file_len;

    let mut stats: HashMap<Vec<u8>, MeasurementStats> = HashMap::new();
    for line in map.split(|c| *c == b'\n') {
        if line.is_empty() {
            break;
        }

        if (next_m_advise_byte - line.as_ptr() as usize) < when_to_start_m_advise {
            if (next_m_advise_byte + m_advise_size) > map_end_byte {
                if unsafe { libc::madvise(next_m_advise_byte as *mut c_void, (map_end_byte - next_m_advise_byte) as libc::size_t, libc::MADV_WILLNEED) } != 0 {
                    panic!("madvise MADV_WILLNEED failed: {:?}", std::io::Error::last_os_error());
                }
                next_m_advise_byte = usize::MAX;
            } else {
                if unsafe { libc::madvise(next_m_advise_byte as *mut c_void, m_advise_size as libc::size_t, libc::MADV_WILLNEED) } != 0 {
                    panic!("madvise MADV_WILLNEED failed: {:?}", std::io::Error::last_os_error());
                }
                next_m_advise_byte += m_advise_size;
            };
            if m_adivse_block_index > 0 {
                if unsafe { libc::madvise((map_start_byte + ((m_adivse_block_index - 1) * m_advise_size)) as *mut c_void, m_advise_size as libc::size_t, libc::MADV_DONTNEED) } != 0 {
                    panic!("madvise MADV_DONTNEED failed: {:?}", std::io::Error::last_os_error());
                }
            }
            m_adivse_block_index += 1;
        }

        let mut fields = line.rsplitn(2, |c| *c == b';');
        let temperature = unsafe { str::from_utf8_unchecked(fields.next().unwrap()).parse::<f32>().unwrap() };
        let station = fields.next().unwrap();

        let stat = match stats.get_mut(station) {
            Some(stat) => stat,
            None => stats.entry(station.to_vec()).or_insert(MeasurementStats::default())
        };
        stat.update(temperature);
    }

    let stats = BTreeMap::from_iter(stats.into_iter().map(|(k, v)| (unsafe { String::from_utf8_unchecked(k) }, v)));
    for (location, MeasurementStats { min, max, sum, count}) in stats.iter() {
        let mean = sum / (*count as f32);
        println!("{location}={min:.1}/{mean:.1}/{max:.1}");
    }
}

// we cannot use external dependency therefore this way coppied from memmap2 lib
fn mmap(file: &File, file_len: usize) -> *mut c_void {
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
            ptr
        }
    }
}
