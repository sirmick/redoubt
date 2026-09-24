//! `stub::plan` on hostile ELF bytes: never panics, and every segment it accepts satisfies the
//! bounds/flags invariants `validate` claims (TENETS.md 6: "fuzz what parses").
#![no_main]

use libfuzzer_sys::fuzz_target;
use stub::{BadImage, Either};

/// A page-aligned, plausible base: fixed, since fuzzing it too would only multiply inputs
/// without exercising different logic (every check is relative to it).
const IMAGE_ADDR: usize = 0x1000_0000;
const PAGE_SIZE: usize = 4096;
/// A page range disjoint from `IMAGE_ADDR` for a fixed 1-page 'excluded' region (the startup
/// page, or the stub's own, in real use), so overlap-refusal has something to bite on.
const EXCLUDE: (usize, usize) = (0x0f00_0000, 0x0f00_0000 + PAGE_SIZE);

fuzz_target!(|data: &[u8]| {
    let result = stub::plan::<()>(data, IMAGE_ADDR, &[EXCLUDE], |segment| {
        // Every accepted segment: page-aligned, non-overlapping with EXCLUDE, and never both
        // writable and executable nor writable without readable (R11, TENETS.md 2).
        assert_eq!(segment.first_page % PAGE_SIZE, 0);
        let end = segment.first_page + segment.pages * PAGE_SIZE;
        assert!(segment.first_page >= EXCLUDE.1 || end <= EXCLUDE.0);
        let bits = segment.flags.bits();
        assert!(bits & 6 != 6, "writable and executable together"); // WRITE=2, EXECUTE=4
        assert!(bits & 2 == 0 || bits & 1 != 0, "writable without readable");
        assert!(bits != 0, "no permission at all");
        assert!(segment.file_offset < segment.pages * PAGE_SIZE);
        Ok(())
    });
    // Never anything but the defined `BadImage` refusals, or success.
    match result {
        Ok(_entry) => {}
        Err(Either::A(
            BadImage::Malformed
            | BadImage::Overflow
            | BadImage::OutOfImage
            | BadImage::Overlaps
            | BadImage::BadFlags
            | BadImage::WrongMachine
            | BadImage::BadAlign
            | BadImage::EntryNotExecutable,
        )) => {}
        Err(Either::B(())) => unreachable!("on_segment above never returns Err"),
    }
});
