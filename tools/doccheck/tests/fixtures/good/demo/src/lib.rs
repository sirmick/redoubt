// Messages follow R1 (flow).
#[cfg(test)]
mod tests {
    #[test]
    fn flows() {}

    #[test]
    #[cfg_attr(
        debug_assertions,
        ignore = "slow"
    )]
    fn slowly() {}
}
const URL: &str = "https://example.org/R99"; // Follows R1 (flow).
