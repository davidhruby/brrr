#[allow(dead_code)]
#[path = "main.rs"]
pub mod app;

pub fn parse_f32(temperature: &[u8]) -> f32 {
    unsafe { str::from_utf8_unchecked(temperature).parse().unwrap() }
}

pub fn parse_temperature_jon_version(temperature: &[u8]) -> i16 {
    let mut t: i16 = 0;
    let mut mul = 1;
    for &d in temperature.iter().rev() {
        match d {
            b'.' => {
                continue;
            }
            b'-' => {
                t = -t;
                break;
            }
            _ => {
                t += i16::from(d - b'0') * mul;
                mul *= 10;
            }
        }
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_f32() {
        assert_eq!(parse_f32(b"99.9"), 99.9);
        assert_eq!(parse_f32(b"2.5"), 2.5);
        assert_eq!(parse_f32(b"0.1"), 0.1);
        assert_eq!(parse_f32(b"0.0"), 0.);
        assert_eq!(parse_f32(b"-0.1"), -0.1);
        assert_eq!(parse_f32(b"-2.5"), -2.5);
        assert_eq!(parse_f32(b"-99.9"), -99.9);
    }

    #[test]
    fn test_parse_temperature_v2() {
        assert_eq!(parse_temperature_jon_version(b"99.9"), 999);
        assert_eq!(parse_temperature_jon_version(b"2.5"), 25);
        assert_eq!(parse_temperature_jon_version(b"0.1"), 1);
        assert_eq!(parse_temperature_jon_version(b"0.0"), 0);
        assert_eq!(parse_temperature_jon_version(b"-0.1"), -1);
        assert_eq!(parse_temperature_jon_version(b"-2.5"), -25);
        assert_eq!(parse_temperature_jon_version(b"-99.9"), -999);
    }
}
