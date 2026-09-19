//! The machines the bench knows how to build for and boot.

pub struct Target {
    pub name: &'static str,
    /// Rust target for the kernel, the loader and `no_std` programs.
    pub triple: &'static str,
    /// How to boot it, or why it cannot be booted yet. Build cases work either way.
    pub machine: Result<Machine, &'static str>,
}

pub struct Machine {
    pub qemu: &'static str,
    pub qemu_args: &'static [&'static str],
    pub loader_package: &'static str,
    pub kernel_features: &'static [&'static str],
}

pub const TARGETS: &[Target] = &[
    Target {
        name: "rv64",
        triple: "riscv64imac-unknown-none-elf",
        machine: Ok(Machine {
            qemu: "qemu-system-riscv64",
            qemu_args: &["-machine", "virt", "-m", "256M"],
            loader_package: "loader",
            kernel_features: &["qemu-virt"],
        }),
    },
    Target {
        name: "rv32",
        triple: "riscv32imac-unknown-none-elf",
        machine: Ok(Machine {
            qemu: "qemu-system-riscv32",
            qemu_args: &["-machine", "virt", "-m", "256M"],
            loader_package: "loader",
            kernel_features: &["qemu-virt"],
        }),
    },
];

pub fn find(name: &str) -> Option<&'static Target> { TARGETS.iter().find(|t| t.name == name) }
