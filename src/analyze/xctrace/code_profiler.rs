use std::collections::{HashMap, HashSet};
use std::fs;
use serde::{Deserialize};

#[derive(Debug, Deserialize)]
#[serde(rename = "trace-query-result")]
struct TraceQueryResult {
    #[serde(rename = "node")]
    nodes: Vec<Node>,
}

// One exported table.
#[derive(Debug, Deserialize)]
struct Node {
    #[serde(rename = "row")]
    rows: Vec<ProfileRow>,
}

#[derive(Debug, Deserialize)]
struct ProfileRow {
    #[serde(rename = "tagged-backtrace")]
    backtrace: Option<Backtrace>,
}

#[derive(Debug, Deserialize)]
struct Backtrace {
    #[serde(rename = "@id")]  id: Option<u32>,
    #[serde(rename = "@ref")] reference: Option<u32>,
    #[serde(default)]
    frame: Vec<Frame>,
}

#[derive(Debug, Deserialize)]
struct Frame {
    #[serde(rename = "@id")]   id: Option<u32>,
    #[serde(rename = "@ref")]  reference: Option<u32>,
    #[serde(rename = "@name")] name: Option<String>,
}
pub fn profile_code(input_file_path: &str) {
    // Read and parse the exported thread-state table.
    let xml: String = fs::read_to_string(input_file_path).unwrap();

    let trace_query_result: TraceQueryResult = quick_xml::de::from_str(&xml).unwrap();

    // Lookup tables: id -> value.
    let mut frame_names: HashMap<u32, String> = HashMap::new();     // frame id -> function name
    let mut backtraces: HashMap<u32, Vec<u32>> = HashMap::new();    // backtrace id -> frame ids

    // Results: function name -> number of samples.
    let mut self_samples: HashMap<String, u64> = HashMap::new();
    let mut inclusive_samples: HashMap<String, u64> = HashMap::new();
    let mut samples: u64 = 0;

    for node in &trace_query_result.nodes {
        // Pass 1: collect all frame and backtrace definitions.
        for row in &node.rows {
            let Some(backtrace) = &row.backtrace else { continue };   // no stack
            let Some(id) = backtrace.id else { continue };            // only a ref, nothing new

            let mut frame_ids = Vec::new();
            for frame in &backtrace.frame {
                if let (Some(frame_id), Some(name)) = (frame.id, &frame.name) {
                    frame_names.insert(frame_id, format!("{:#}", rustc_demangle::demangle(name)));
                }
                frame_ids.push(frame.id.or(frame.reference).unwrap());
            }
            backtraces.insert(id, frame_ids);
        }

        // Pass 2: count the samples.
        for row in &node.rows {
            let Some(backtrace) = &row.backtrace else { continue };
            let frame_ids = &backtraces[&backtrace.id.or(backtrace.reference).unwrap()];
            if frame_ids.is_empty() {
                continue;
            }
            samples += 1;

            // Self: the function the CPU was in = the first frame.
            let top = &frame_names[&frame_ids[0]];
            *self_samples.entry(top.clone()).or_insert(0) += 1;

            // Inclusive: every function on the stack, once per sample.
            let mut seen: HashSet<&String> = HashSet::new();
            for frame_id in frame_ids {
                let name = &frame_names[frame_id];
                if seen.insert(name) {
                    *inclusive_samples.entry(name.clone()).or_insert(0) += 1;
                }
            }
        }
    }

    println!("samples: {samples} (1 sample = 1 ms)");
    print_top("Self time (where the CPU was)", &self_samples, samples, 15);
    print_top("Inclusive time (function and everything it called)", &inclusive_samples, samples, 15);
}

// Print the functions with the most samples, highest first.
fn print_top(title: &str, counts: &HashMap<String, u64>, samples: u64, limit: usize) {
    let mut sorted: Vec<(&String, &u64)> = counts.iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(a.1));

    println!();
    println!("{title}");
    for (name, count) in sorted.iter().take(limit) {
        println!("{:>6}  {:5.1} %  {}", count, **count as f64 / samples as f64 * 100.0, name);
    }
}