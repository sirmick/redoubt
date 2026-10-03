//! The bound's prices are the kernel's (kernel/objects.md, "What objects cost"): each constant
//! `init` names is read back from the page, so a change to the cost table fails here until the
//! bound follows it.

use redoubt_init::bound::{ENDPOINT_PAGES, HANDLES_PER_TABLE_PAGE, PROCESS_OBJECT_PAGES, THREAD_IPC_PAGES};

const OBJECTS: &str = include_str!("../../../docs/kernel/objects.md");

/// The pages column of the cost table's row whose first cell is `what`.
fn pages(what: &str) -> u64 {
    let table = &OBJECTS[OBJECTS.find("### What objects cost").expect("objects.md's cost table")..];
    let row = table
        .lines()
        .find(|l| l.starts_with(&format!("| {what} |")))
        .unwrap_or_else(|| panic!("objects.md has no cost row {what:?}"));
    row.split('|').nth(2).unwrap().trim().parse().unwrap_or_else(|_| panic!("{row}"))
}

#[test]
fn the_bound_s_prices_are_the_cost_table_s() {
    assert_eq!(pages("endpoint"), ENDPOINT_PAGES);
    assert_eq!(pages("process object (it holds the exit notice)"), PROCESS_OBJECT_PAGES);
    let ipc =
        OBJECTS.lines().find(|l| l.starts_with("| thread IPC page |")).expect("objects.md's IPC page row");
    assert_eq!(ipc.split('|').nth(2).unwrap().trim(), format!("{THREAD_IPC_PAGES} per thread"));
    let first = "Table page 0 holds indices 1 to ";
    let at = OBJECTS.find(first).expect("objects.md says what table page 0 holds") + first.len();
    let last: String = OBJECTS[at..].chars().take_while(char::is_ascii_digit).collect();
    assert_eq!(last.parse::<u64>().unwrap(), HANDLES_PER_TABLE_PAGE);
}
