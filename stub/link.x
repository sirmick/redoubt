/* The loader stub: a flat binary a launcher maps into every child at a fixed address
 * (this file's ORIGIN = stub::STUB_ENTRY), the same on rv32 and rv64. No dynamic relocations,
 * no writable statics (its own image is mapped read-only executable alongside the process's
 * real one -- servers/init.md, The startup block -- so it never needs a `.bss`/`.data` of its own).
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

/* Every launcher calls process_start(..., STUB_ENTRY, ...) (servers/init.md, launch step 3): if `_start`
 * ever landed anywhere else in `.text`, every child would run whatever code the linker put at
 * STUB_ENTRY instead, silently. `_start`'s `.text.init` section (main.rs) is `KEEP`'d first, so
 * this should always hold; the assert catches a future linker-layout change that breaks it.
 */
ASSERT(_start == ORIGIN(RAM), "_start must be the stub's first byte (STUB_ENTRY)");

/* The module doc's "no `.bss`/`.data` of its own" claim (top of this file) was only a comment,
 * not enforced: a `.bss` is NOLOAD and silently dropped by `objcopy`, so a future writable
 * static here would compile clean and only fail once faulted into (this page is read-only
 * executable) -- catch it at link time instead.
 */
ASSERT(SIZEOF(.data) == 0, "the stub must have no .data (see this file's module doc)");
ASSERT(SIZEOF(.bss) == 0, "the stub must have no .bss (see this file's module doc)");
