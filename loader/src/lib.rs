//! The loader's parts that run on the host as well: the device-tree read (`dt.rs`) and the
//! console it reports through. `main.rs` is the loader itself, which only builds for the
//! machine; splitting these out lets host unit tests read device-tree fixtures with no boot.

#![cfg_attr(not(test), no_std)]

pub mod dt;

/// The firmware's console, a byte at a time. Host tests print to their own output instead.
pub struct Console;

impl core::fmt::Write for Console {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        #[cfg(not(test))]
        for b in s.bytes() {
            sbi_rt::console_write_byte(b);
        }
        #[cfg(test)]
        std::print!("{s}");
        Ok(())
    }
}

#[macro_export]
macro_rules! println {
    () => {{ let _ = core::fmt::Write::write_str(&mut $crate::Console, "\n"); }};
    ($($arg:tt)*) => {{
        let _ = core::fmt::Write::write_fmt(&mut $crate::Console, format_args!($($arg)*));
        let _ = core::fmt::Write::write_str(&mut $crate::Console, "\n");
    }};
}
