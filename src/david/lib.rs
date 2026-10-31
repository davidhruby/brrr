#[allow(dead_code)]
#[path = "main.rs"]
pub mod app;

pub fn parse_f32(temperature: &[u8]) -> f32 {
    unsafe { str::from_utf8_unchecked(temperature).parse().unwrap() }
}

pub fn parse_temperature_v2(temperature: &[u8]) -> i16 {
    let is_negative = (temperature[0] == b'-') as usize;
    let digits = &temperature[is_negative..];
    let len = digits.len();

    let tenths = (digits[len - 1] - b'0') as i16;
    let ones = (digits[len - 3] - b'0') as i16;
    let tens = (digits[0] - b'0') as i16 * (len == 4) as i16;

    let value = tens * 100 + ones * 10 + tenths;
    // is_negative has 0 or 1 value therefore 1 - (2 * 0) = 1, 1 - (2 * 1) = -1
    value * (1 - (2 * is_negative as i16))
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
        assert_eq!(parse_temperature_v2(b"99.9"), 999);
        assert_eq!(parse_temperature_v2(b"2.5"), 25);
        assert_eq!(parse_temperature_v2(b"0.1"), 1);
        assert_eq!(parse_temperature_v2(b"0.0"), 0);
        assert_eq!(parse_temperature_v2(b"-0.1"), -1);
        assert_eq!(parse_temperature_v2(b"-2.5"), -25);
        assert_eq!(parse_temperature_v2(b"-99.9"), -999);
    }
}
