// SPDX-FileCopyrightText: 2022 Foundation Devices, Inc. <hello@foundationdevices.com>
// SPDX-License-Identifier: Apache-2.0

/// A trait for serial like drivers which are byte-oriented sinks.
pub trait SerialWrite {
    /// Write a single byte.
    fn putc(&mut self, b: u8);
}
