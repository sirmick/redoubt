// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-FileCopyrightText: 2023 Foundation Devices, Inc. <hello@foundationdevices.com>
// SPDX-License-Identifier: Apache-2.0

/// Prints to the debug output directly.
#[macro_export]
macro_rules! print {
    ($($args:tt)+) => {{
        $crate::debug::console::print(format_args!($($args)+))
    }};
}

/// Prints to the debug output directly, with a newline.
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
