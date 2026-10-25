# Part 003

**Dates:** 2026-10-25 – 2026-10-25
**Video progress:** stopped at [43:19](https://www.youtube.com/watch?v=tCY7p6dVAGE&t=2599s) when this part was published
**Code:** git tag [`part003`](https://github.com/davidhruby/brrr/tree/part003). To get the code exactly as it was for this part: `git checkout part003`

New here? Start with the [Introduction](intro.md).

## Goal

My version is now close to Jon's, so I don't compare it with his any more, only with the
previous step.

## What changed

### 1. Renaming to Jon's names

No change in behavior: the variables now have the same names as in Jon's version, so when he
talks about `stats`, `station` or `temperature` in the stream, it means the same thing in my
code.

Before:

```rust
let mut location_to_measurement: BTreeMap<String, MeasurementStats> = BTreeMap::new();
for line in reader.lines() {
    let line = line.unwrap();
    let (location, measurement_str) = line.split_once(';').unwrap();
    let measurement = measurement_str.parse::<f32>().unwrap();

    location_to_measurement.entry(location.to_string())
        .and_modify(|m| m.update(measurement))
        .or_insert_with(|| MeasurementStats::new(measurement));
}
```

After:

```rust
let mut stats: BTreeMap<String, MeasurementStats> = BTreeMap::new();
for line in reader.lines() {
    let line = line.unwrap();
    let (station, temperature) = line.split_once(';').unwrap();
    let temperature = temperature.parse::<f32>().unwrap();

    stats.entry(station.to_string())
        .and_modify(|m| m.update(temperature))
        .or_insert_with(|| MeasurementStats::new(temperature));
}
```

Measurements after this change (raw output in
[`measurements/part003/`](../measurements/part003/)):

| | `david` 100M | `david` 1B |
|---|---|---|
| Wall time | 10.7 s | 108 s |
| Memory | 1.9 MB | 1.9 MB |
| Swapped to the SSD | 0 | 0 |
| Instructions per row | ~1,840 | ~1,860 |

**No change in speed.**

## What I learned

## Next steps

## TODO
