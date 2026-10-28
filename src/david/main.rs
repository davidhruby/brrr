use std::{env, str};
use std::collections::{BTreeMap, HashMap};
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
    let map = mmap(&file);

    let mut stats: HashMap<Vec<u8>, MeasurementStats> = HashMap::new();
    for line in map.split(|c| *c == b'\n') {
        if line.is_empty() {
            break;
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
fn mmap(file: &File) -> &'static [u8] {
    let len = file.metadata().unwrap().len() as usize;
    // SAFETY: creating a new memory map with a nullptr as address hint is always sound:
    // it does not modify any existing mapping or memory contents.
    unsafe {
        let ptr = libc::mmap(
                std::ptr::null_mut(),
                len as libc::size_t,
                libc::PROT_READ,
                libc::MAP_SHARED,
                file.as_raw_fd(),
                0,
            );
        if ptr == libc::MAP_FAILED {
            panic!("mmap failed: {:?}", std::io::Error::last_os_error());
        } else {
            if libc::madvise(ptr, len, libc::MADV_WILLNEED) != 0 {
                panic!("madvise  failed: {:?}", std::io::Error::last_os_error());
            } else {
                std::slice::from_raw_parts(ptr as *const u8, len)
            }
        }
    }


}
