/* RustSBI enters S-mode payloads here on QEMU virt and supported boards. */
ENTRY(_start)

MEMORY
{
    RAM : ORIGIN = 0x80200000, LENGTH = 16M
}

SECTIONS
{
    .text : {
        KEEP(*(.text.init))
        *(.text .text.*)
    } > RAM

    .rodata : ALIGN(8) {
        *(.rodata .rodata.*)
        *(.srodata .srodata.*)
    } > RAM

    .data : ALIGN(8) {
        *(.data .data.*)
        *(.sdata .sdata.*)
    } > RAM

    .bss (NOLOAD) : ALIGN(8) {
        _sbss = .;
        *(.bss .bss.*)
        *(.sbss .sbss.*)
        . = ALIGN(8);
        _ebss = .;
    } > RAM

    .stack (NOLOAD) : ALIGN(4096) {
        . += 64K;
        _stack_top = .;
    } > RAM

    _loader_end = .;

    /DISCARD/ : { *(.eh_frame) *(.eh_frame_hdr) }
}
