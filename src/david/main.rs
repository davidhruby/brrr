// this challenge has a rule that all code has to be in one file
use std::{env, str, thread};
use std::collections::{BTreeMap, HashMap};
use std::ffi::c_void;
use std::fs::File;
use std::os::fd::AsRawFd;
use std::sync::mpsc;
use std::sync::mpsc::Sender;

const KB: usize = 1024;
const MB: usize = 1024 * KB;
const M_ADVISE_SIZE: usize = 512 * MB;
const MACOS_PAGE_SIZE: usize = 16 * KB;

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

    // pre-load next block into RAM
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

    let mut stats: HashMap<Vec<u8>, MeasurementStats> = HashMap::new();
    for line in map.split(|c| *c == b'\n') {
        if line.is_empty() {
            break;
        }

        // trigger next block pre-load into RAM if needed
        next_block_pre_load_info.request_next_block_load(&tx, line.as_ptr());

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
