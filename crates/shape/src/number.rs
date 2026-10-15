//! Numbers in XFL edge text.
//!
//! Animate writes coordinates either as decimals (`-20.5625`) or, for values that are not whole twips, as
//! 24.8 fixed point in hex with a leading `#`: the integer part is written in two's complement (`#FFFFEC` is -20),
//! the fraction as hex digits after a dot (`.87` is 135/256). `#1A.8` is therefore 26.5.

use crate::error::ParseErrorKind;

/// Longest number token accepted, in bytes.
pub const MAX_NUMBER_TEXT: usize = 40;

/// Coordinates beyond this many twips (about 50 million pixels) are rejected as hostile or corrupt.
pub const MAX_COORD: f64 = 1.0e9;

/// Parses one number token: decimal, or `#` hex with an optional `.` hex fraction.
///
/// # Errors
/// [`ParseErrorKind::BadNumber`] for malformed text, [`ParseErrorKind::NumberRange`] for huge magnitudes.
pub fn parse_number(token: &str) -> Result<f64, ParseErrorKind> {
    if token.len() > MAX_NUMBER_TEXT {
        return Err(ParseErrorKind::BadNumber);
    }
    let value = match token.strip_prefix('#') {
        Some(hex) => parse_hex(hex)?,
        None => parse_decimal(token)?,
    };
    if value.is_finite() && value.abs() <= MAX_COORD { Ok(value) } else { Err(ParseErrorKind::NumberRange) }
}

fn parse_decimal(token: &str) -> Result<f64, ParseErrorKind> {
    let body = token.strip_prefix(['-', '+']).unwrap_or(token);
    let (int, frac) = body.split_once('.').unwrap_or((body, ""));
    let digits_only = int.bytes().all(|b| b.is_ascii_digit()) && frac.bytes().all(|b| b.is_ascii_digit());
    if !digits_only || (int.is_empty() && frac.is_empty()) {
        return Err(ParseErrorKind::BadNumber);
    }
    token.parse::<f64>().map_err(|_| ParseErrorKind::BadNumber)
}

fn parse_hex(body: &str) -> Result<f64, ParseErrorKind> {
    let (int, frac) = body.split_once('.').unwrap_or((body, ""));
    let valid = !int.is_empty() && int.len() <= 8 && frac.len() <= 8 && int.bytes().chain(frac.bytes()).all(|b| b.is_ascii_hexdigit());
    if !valid {
        return Err(ParseErrorKind::BadNumber);
    }
    let raw = u32::from_str_radix(int, 16).map_err(|_| ParseErrorKind::BadNumber)?;
    // Six digits are a 24-bit two's complement integer part; seven or eight digits are a full 32-bit one.
    let int_part = if int.len() > 6 {
        f64::from(i32::from_ne_bytes(raw.to_ne_bytes()))
    } else if raw >= 0x80_0000 {
        f64::from(raw) - 16_777_216.0
    } else {
        f64::from(raw)
    };
    let fraction = if frac.is_empty() {
        0.0
    } else {
        let digits = u32::from_str_radix(frac, 16).map_err(|_| ParseErrorKind::BadNumber)?;
        let scale = frac.chars().fold(1.0, |scale, _| scale * 16.0);
        f64::from(digits) / scale
    };
    Ok(int_part + fraction)
}

/// Formats a coordinate in canonical decimal: rounded to 1/256 twip (the precision Animate stores) and printed
/// with the shortest exact representation, so whole numbers have no decimal point.
#[must_use]
pub fn format_number(value: f64) -> String {
    format!("{}", quantize(value))
}

/// Rounds to the nearest 1/256 and maps `-0` and non-finite values to `0`.
#[must_use]
pub fn quantize(value: f64) -> f64 {
    if !value.is_finite() {
        return 0.0;
    }
    let q = (value * 256.0).round() / 256.0;
    if q.abs() < f64::EPSILON { 0.0 } else { q }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-12
    }

    #[test]
    fn decimals() {
        assert!(close(parse_number("12").unwrap(), 12.0));
        assert!(close(parse_number("-20.5625").unwrap(), -20.5625));
        assert!(close(parse_number("+3.").unwrap(), 3.0));
        assert!(close(parse_number(".5").unwrap(), 0.5));
        assert!(close(parse_number("-0").unwrap(), 0.0));
    }

    #[test]
    fn hex_numbers() {
        assert!(close(parse_number("#1A.8").unwrap(), 26.5));
        assert!(close(parse_number("#27.9").unwrap(), 39.5625));
        assert!(close(parse_number("#28.76").unwrap(), 40.0 + 118.0 / 256.0));
        assert!(close(parse_number("#FFFFEC").unwrap(), -20.0));
        assert!(close(parse_number("#FFFFEC.87").unwrap(), -20.0 + 135.0 / 256.0));
        assert!(close(parse_number("#FFFFBA.5A").unwrap(), -70.0 + 90.0 / 256.0));
        assert!(close(parse_number("#0").unwrap(), 0.0));
        assert!(close(parse_number("#FFFFFFEC").unwrap(), -20.0));
    }

    #[test]
    fn rejects_garbage() {
        for bad in ["", "-", "+", ".", "#", "#.5", "#G1", "1e5", "inf", "NaN", "1..2", "1.2.3", "--1", "0x10", "#123456789", "#1.123456789"] {
            assert!(parse_number(bad).is_err(), "{bad:?} should not parse");
        }
        assert_eq!(parse_number("99999999999"), Err(ParseErrorKind::NumberRange));
        assert_eq!(parse_number(&"1".repeat(MAX_NUMBER_TEXT + 1)), Err(ParseErrorKind::BadNumber));
    }

    #[test]
    fn formatting_is_canonical() {
        assert_eq!(format_number(5.0), "5");
        assert_eq!(format_number(-20.5625), "-20.5625");
        assert_eq!(format_number(-0.0), "0");
        assert_eq!(format_number(0.001), "0");
        assert_eq!(format_number(1.0 / 3.0), "0.33203125");
        assert_eq!(format_number(f64::NAN), "0");
    }
}
