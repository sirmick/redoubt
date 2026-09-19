//! Float to text, matching `float_to_list(F, [short])` (and hence `~w` / `~p`).
//!
//! OTP finds the shortest digit string that reads back as the same double with Ryu; so do we,
//! through the `ryu` crate, so that ties break the same way. What remains is OTP's choice
//! between fixed and scientific layout, ported from `erts/emulator/ryu/to_chars.h`.

use alloc::format;
use alloc::string::String;

/// The shortest round-tripping decimal for a finite `x >= 0`: `(digits, exponent)` with
/// `x = digits * 10^exponent` and no trailing zeros in `digits` ("0" for zero).
fn shortest(x: f64) -> (String, i32) {
    let mut buf = ryu::Buffer::new();
    let s = buf.format_finite(x);
    let (mantissa, exp) = match s.split_once('e') {
        Some((m, e)) => (m, e.parse::<i32>().expect("ryu writes an integer exponent")),
        None => (s, 0),
    };
    let (whole, frac) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let mut digits: String = whole.chars().chain(frac.chars()).collect();
    let mut exp = exp - frac.len() as i32;
    let lead = digits.len() - digits.trim_start_matches('0').len();
    digits.drain(..lead);
    while digits.len() > 1 && digits.ends_with('0') {
        digits.pop();
        exp += 1;
    }
    if digits.is_empty() || digits == "0" {
        return (String::from("0"), 0);
    }
    (digits, exp)
}

pub fn format_short(x: f64) -> String {
    let (digits, ryu_exp) = shortest(x.abs());
    let olength = digits.len() as i32;
    let sci_exp = ryu_exp + olength - 1;
    let output: u64 = digits.parse().expect("at most 17 digits");

    let (lower, upper) = if olength == 1 {
        (-4, 2)
    } else if sci_exp >= 10 {
        (-(olength + 2), 2)
    } else {
        (-(olength + 2), 1)
    };
    let fixed = lower <= ryu_exp
        && ryu_exp <= upper
        && !((output >= 1 << 53 && ryu_exp == 0)
            || (output > (1 << 52) / 5 && ryu_exp == 1)
            || (output > (1 << 51) / 25 && ryu_exp == 2));

    let mut out = String::new();
    if x.is_sign_negative() {
        out.push('-');
    }
    if fixed {
        let whole = olength + ryu_exp;
        if ryu_exp >= 0 {
            out.push_str(&digits);
            out.extend(core::iter::repeat_n('0', ryu_exp as usize));
            out.push_str(".0");
        } else if whole > 0 {
            out.push_str(&digits[..whole as usize]);
            out.push('.');
            out.push_str(&digits[whole as usize..]);
        } else {
            out.push_str("0.");
            out.extend(core::iter::repeat_n('0', (-whole) as usize));
            out.push_str(&digits);
        }
    } else {
        out.push_str(&digits[..1]);
        out.push('.');
        if olength == 1 {
            out.push('0');
        } else {
            out.push_str(&digits[1..]);
        }
        out.push('e');
        out.push_str(&format!("{sci_exp}"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::format_short;

    /// Expected strings are from `float_to_list(F, [short])` on OTP 28.
    #[test]
    #[allow(clippy::approx_constant)] // 3.141592653589793 is a test vector, not a use of pi
    fn matches_otp() {
        for (x, s) in [
            (0.0, "0.0"),
            (-0.0, "-0.0"),
            (1.0, "1.0"),
            (-1.5, "-1.5"),
            (0.1, "0.1"),
            (100.0, "100.0"),
            (1000.0, "1.0e3"),
            (1729.0, "1729.0"),
            (0.001, "0.001"),
            (0.0001, "0.0001"),
            (1.0e10, "1.0e10"),
            (123456789.0, "123456789.0"),
            (1.0e-10, "1.0e-10"),
            (3.141592653589793, "3.141592653589793"),
            (1.7976931348623157e308, "1.7976931348623157e308"),
            (5.0e-324, "5.0e-324"),
            (9007199254740992.0, "9.007199254740992e15"),
            (9007199254740994.0, "9.007199254740994e15"),
            // A tie between two shortest candidates: Ryu, like OTP, rounds to even.
            (2.9802322387695312e-8, "2.9802322387695312e-8"),
            (1.0e23, "1.0e23"),
            (9.0e-265, "9.0e-265"),
            (0.30000000000000004, "0.30000000000000004"),
        ] {
            assert_eq!(format_short(x), s, "formatting {x:e}");
        }
    }
}
