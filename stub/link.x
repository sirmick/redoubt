/* The loader stub (WP-R2): a flat binary a launcher maps into every child at a fixed address
 * (this file's ORIGIN = stub::STUB_ENTRY), the same on rv32 and rv64. No dynamic relocations,
 * no writable statics (its own image is mapped read-only executable alongside the process's
 * real one -- INIT.md, Startup block -- so it never needs a `.bss`/`.data` of its own).
 */
ENTRY(_start)

MEMORY
{
    RAM : ORIGIN = 0x1FF00000, LENGTH = 256K
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

    /* Refuse a build that would need one: see the module doc on `stub`'s lack of statics. */
    .data : ALIGN(8) {
        *(.data .data.*)
        *(.sdata .sdata.*)
    } > RAM

    .bss (NOLOAD) : ALIGN(8) {
        *(.bss .bss.*)
        *(.sbss .sbss.*)
    } > RAM

    _stub_end = .;

    /DISCARD/ : { *(.eh_frame) *(.eh_frame_hdr) }
}
