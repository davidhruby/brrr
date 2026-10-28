use std::{env, str, thread};
use std::collections::{BTreeMap, HashMap};
use std::ffi::c_void;
use std::fs::File;
use std::os::fd::AsRawFd;
use std::sync::mpsc;

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

const M_ADVISE_SIZE: usize = 512 * 1024 * 1024;

fn main() {
    let args: Vec<String> = env::args().collect();
    let input_file_path = &args[1];

    let (tx, rx) = mpsc::channel::<(usize, usize, usize)>();

    thread::spawn(move || {
        for (ptr, len, m_adivse_block_index) in rx {
            if unsafe { libc::madvise(ptr as *mut c_void, len as libc::size_t, libc::MADV_WILLNEED) } != 0 {
                panic!("madvise MADV_WILLNEED failed: {:?}", std::io::Error::last_os_error());
            }
            if m_adivse_block_index > 0 {
                if unsafe { libc::madvise((ptr - M_ADVISE_SIZE - M_ADVISE_SIZE) as *mut c_void, M_ADVISE_SIZE as libc::size_t, libc::MADV_DONTNEED) } != 0 {
                    panic!("madvise MADV_DONTNEED failed: {:?}", std::io::Error::last_os_error());
                }
            }
            let page_count = len / (16 * 1024);
            for page_index in 0..page_count {
                let byte_position = ptr + (page_index * 16 * 1024);
                let y = byte_position as *const u8;
                unsafe { std::ptr::read_volatile(y) };
            }
        }
    });

    let file = File::open(input_file_path).unwrap();
    let file_len: usize = file.metadata().unwrap().len() as usize;
    let when_to_start_m_advise = M_ADVISE_SIZE / 2;
    let ptr = mmap(&file, file_len);

    tx.send((ptr as usize, M_ADVISE_SIZE, 0)).unwrap();
    let map = unsafe { std::slice::from_raw_parts(ptr as *const u8, file_len) };
    let map_start_byte = map.as_ptr() as usize;
    let mut next_m_advise_byte = map_start_byte + M_ADVISE_SIZE;
    let map_end_byte = map_start_byte + file_len;

    let mut stats: HashMap<Vec<u8>, MeasurementStats> = HashMap::new();
    for line in map.split(|c| *c == b'\n') {
        if line.is_empty() {
            break;
        }

        if (next_m_advise_byte - line.as_ptr() as usize) < when_to_start_m_advise {
            let m_adivse_block_index = (next_m_advise_byte - (map_start_byte + M_ADVISE_SIZE)) / M_ADVISE_SIZE;
            if (next_m_advise_byte + M_ADVISE_SIZE) > map_end_byte {
                tx.send((next_m_advise_byte, (map_end_byte - next_m_advise_byte), m_adivse_block_index)).unwrap();
                next_m_advise_byte = usize::MAX;
            } else {
                tx.send((next_m_advise_byte, M_ADVISE_SIZE, m_adivse_block_index)).unwrap();
                next_m_advise_byte += M_ADVISE_SIZE;
            };
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
