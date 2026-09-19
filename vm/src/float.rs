//! Float to text, matching `float_to_list(F, [short])` (and hence `~w` / `~p`).
//!
//! Rust's `{:e}` formatting already produces the shortest digit string that reads back as the
//! same double, which is what OTP gets from Ryu. What remains is OTP's choice between fixed and
//! scientific layout, ported from `erts/emulator/ryu/to_chars.h`.

use alloc::format;
use alloc::string::String;

pub fn format_short(x: f64) -> String {
    let sci = format!("{:e}", x.abs());
    let (mantissa, exp) = sci.split_once('e').expect("{:e} always has an exponent");
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let sci_exp: i32 = exp.parse().expect("{:e} exponent is an integer");
    let olength = digits.len() as i32;
    // Ryu's representation: value = digits * 10^ryu_exp.
    let ryu_exp = sci_exp - (olength - 1);
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
        ] {
            assert_eq!(format_short(x), s, "formatting {x:e}");
        }
    }
}
