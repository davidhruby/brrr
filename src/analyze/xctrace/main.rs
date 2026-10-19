use std::env;
use crate::code_profiler::profile_code;
use crate::cpu_usage::analyse_cpu_usage;

mod cpu_usage;
mod code_profiler;

fn main() {
    let args: Vec<String> = env::args().collect();
    let input_file_path = &args[1];
    let analysis_type = &args[2];

    match analysis_type.as_str() {
        "cpu_usage" => analyse_cpu_usage(&input_file_path),
        "code_profile" => profile_code(input_file_path),
        _ => panic!("Unknown analysis type"),
    }
}