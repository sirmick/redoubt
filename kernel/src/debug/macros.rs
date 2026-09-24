// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-FileCopyrightText: 2023 Foundation Devices, Inc. <hello@foundationdevices.com>
// SPDX-License-Identifier: Apache-2.0

/// Prints to the debug output directly.
#[cfg(baremetal)]
#[macro_export]
macro_rules! print {
    ($($args:tt)+) => {{
        // SAFETY: `OUTPUT` is written only by `shell::init`, before the first `print!`, and only
        // the boot hart prints (the `smp` spike's `secondary_main` never does), so this is its
        // only live reference. Known residual: a panic inside this `write!` re-enters through
        // the panic handler's `println!` while the first reference is live; the handler then
        // powers off.
        #[allow(unused_unsafe)]
        unsafe {
			use core::fmt::Write;
            if let Some(stream) = &mut *(&raw mut crate::debug::shell::OUTPUT) {
                write!(stream, $($args)+).unwrap();
            }
        }
    }};
}

/// Prints to the debug output directly, with a newline.
#[cfg(baremetal)]
#[macro_export]
macro_rules! println {
	() => ({
		print!("\r\n")
	});
	($fmt:expr) => ({
		print!(concat!($fmt, "\r\n"))
	});
	($fmt:expr, $($args:tt)+) => ({
		print!(concat!($fmt, "\r\n"), $($args)+)
	});
}

#[cfg(feature = "debug-print")]
#[macro_export]
macro_rules! klog {
	() => ({
		println!(" [{}:{}]", file!(), line!())
	});
	($fmt:expr) => ({
        println!(concat!(" [{}:{} ", $fmt, "]"), file!(), line!())
	});
	($fmt:expr, $($args:tt)+) => ({
		println!(concat!(" [{}:{} ", $fmt, "]"), file!(), line!(), $($args)+)
	});
}

#[cfg(not(feature = "debug-print"))]
#[macro_export]
macro_rules! klog {
    ($($args:tt)+) => {{}};
}
