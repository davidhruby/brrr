use std::fs;
use std::collections::{BTreeMap, HashMap};
use serde::{Deserialize};

// The whole exported XML file.
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
    rows: Vec<Row>,
}

// One row = one interval of one thread.
#[derive(Debug, Deserialize)]
struct Row {
    #[serde(rename = "thread-state")]
    state: Value,                  // "Running", "Blocked", ...
    duration: Value,               // length of the interval in ns
    #[serde(default)]
    core: Vec<Value>,              // [0] = the core it ran on (empty when not on a core)
    #[serde(default)]
    narrative: Vec<Narrative>,     // text columns, some cores are defined only in here
}

// A text column, e.g. "... running on CPU 7 (P Core)".
#[derive(Debug, Deserialize)]
struct Narrative {
    #[serde(default)]
    core: Vec<Value>,
}

// A value is either defined here (id) or points to an earlier definition (ref).
#[derive(Debug, Deserialize)]
struct Value {
    #[serde(rename = "@id")]
    id: Option<u32>,
    #[serde(rename = "@ref")]
    reference: Option<u32>,
    #[serde(rename = "@fmt")]
    fmt: Option<String>,           // for humans: "CPU 8 (P Core)", "596.12 µs"
    #[serde(rename = "$text")]
    text: Option<String>,          // raw value: "8", "596125"
}

pub fn analyse_cpu_usage(input_file_path: &str) {
    // Read and parse the exported thread-state table.
    let xml: String = fs::read_to_string(input_file_path).unwrap();

    let trace_query_result: TraceQueryResult = quick_xml::de::from_str(&xml).unwrap();

    // Lookup tables: id -> value.
    let mut cores: HashMap<u32, String> = HashMap::new();    // id -> "CPU 8 (P Core)"
    let mut states: HashMap<u32, String> = HashMap::new();   // id -> "Running"
    let mut durations: HashMap<u32, u64> = HashMap::new();   // id -> ns

    // Results.
    let mut time_per_state: BTreeMap<String, u64> = BTreeMap::new();      // "Blocked" -> ns
    let mut intervals_per_state: BTreeMap<String, u64> = BTreeMap::new();
    let mut seen_running = false;      // before the first run, xctrace holds the process
    let mut time_per_core: BTreeMap<String, u64> = BTreeMap::new();
    let mut intervals_per_core: BTreeMap<String, u64> = BTreeMap::new();
    let mut previous_core: Option<String> = None;
    let mut moves = 0;
    let mut moves_p_e = 0;


    for node in trace_query_result.nodes {
        // Pass 1: collect all definitions into the lookup tables.
        for row in &node.rows {
            for core in &row.core {
                if let (Some(id), Some(fmt)) = (core.id, &core.fmt) {
                    cores.insert(id, fmt.clone());
                }
            }
            for narrative in &row.narrative {
                for core in &narrative.core {
                    if let (Some(id), Some(fmt)) = (core.id, &core.fmt) {
                        cores.insert(id, fmt.clone());
                    }
                }
            }
            if let (Some(id), Some(text)) = (row.state.id, &row.state.text) {
                states.insert(id, text.clone());
            }
            if let (Some(id), Some(text)) = (row.duration.id, &row.duration.text) {
                durations.insert(id, text.parse().unwrap());
            }
        }

        // Pass 2: add up the time per state and the running time per core.
        for row in &node.rows {
            let state = &states[&key(&row.state)];
            let ns = durations[&key(&row.duration)];

            // Skip everything before the program first runs: that's xctrace, not the program.
            if state == "Running" {
                seen_running = true;
            }
            if !seen_running {
                continue;
            }

            // Every interval, whatever the state.
            *time_per_state.entry(state.clone()).or_insert(0) += ns;
            *intervals_per_state.entry(state.clone()).or_insert(0) += 1;

            // Only running intervals are on a core.
            if state == "Running" {
                let core = &cores[&key(&row.core[0])];

                *time_per_core.entry(core.clone()).or_insert(0) += ns;
                *intervals_per_core.entry(core.clone()).or_insert(0) += 1;

                // A different core than last time = the thread moved.
                if let Some(previous) = &previous_core {
                    if previous != core {
                        moves += 1;
                        // Moved between a P-core and an E-core.
                        if previous.contains("P Core") != core.contains("P Core") {
                            moves_p_e += 1;
                        }
                    }
                }
                previous_core = Some(core.clone());
            }
        }
    }
    println!("Cores: {:?}", cores);
    println!("States: {:?}", states);
    println!("Durations count: {}", durations.len());


    println!();
    // Print time, share and number of intervals per state.
    let total: u64 = time_per_state.values().sum();
    for (state, ns) in &time_per_state {
        println!("{state:12}  {:.3} s  {:5.1} %  {} intervals",
                 *ns as f64 / 1e9,
                 *ns as f64 / total as f64 * 100.0,
                 intervals_per_state[state]);
    }

    println!();
    // Print time, share and number of intervals per core.
    let total: u64 = time_per_core.values().sum();
    for (core, ns) in &time_per_core {
        println!("{core}  {:.3} s  {:.1} %  {} intervals",
                 *ns as f64 / 1e9,
                 *ns as f64 / total as f64 * 100.0,
                 intervals_per_core[core]);
    }
    println!("moves: {moves}, P<->E: {moves_p_e}");
}

// The id of a value, whether it's defined here (id) or referenced (ref).
fn key(value: &Value) -> u32 {
    value.id.or(value.reference).unwrap()
}