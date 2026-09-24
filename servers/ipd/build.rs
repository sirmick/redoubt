//! smoltcp's build script sizes its buffers from `SMOLTCP_*` environment variables. `ipd` is
//! built only with the sizes in its source (vendor/README.md, build scripts): with any such
//! variable set, the build stops here.

fn main() {
    let set: Vec<String> = std::env::vars().map(|(k, _)| k).filter(|k| k.starts_with("SMOLTCP_")).collect();
    if !set.is_empty() {
        panic!("ipd refuses to build with smoltcp's sizing variables set: {}", set.join(", "));
    }
    // Rebuild if one is set later. Cargo has no wildcard, so the ones smoltcp 0.14.0 reads
    // (its build.rs, CONFIGS) are named.
    for var in [
        "SMOLTCP_IFACE_MAX_ADDR_COUNT",
        "SMOLTCP_IFACE_MAX_MULTICAST_GROUP_COUNT",
        "SMOLTCP_IFACE_MAX_SIXLOWPAN_ADDRESS_CONTEXT_COUNT",
        "SMOLTCP_IFACE_NEIGHBOR_CACHE_COUNT",
        "SMOLTCP_IFACE_MAX_ROUTE_COUNT",
        "SMOLTCP_IFACE_MAX_PREFIX_COUNT",
        "SMOLTCP_FRAGMENTATION_BUFFER_SIZE",
        "SMOLTCP_ASSEMBLER_MAX_SEGMENT_COUNT",
        "SMOLTCP_REASSEMBLY_BUFFER_SIZE",
        "SMOLTCP_REASSEMBLY_BUFFER_COUNT",
        "SMOLTCP_IPV6_HBH_MAX_OPTIONS",
        "SMOLTCP_DNS_MAX_RESULT_COUNT",
        "SMOLTCP_DNS_MAX_SERVER_COUNT",
        "SMOLTCP_DNS_MAX_NAME_SIZE",
        "SMOLTCP_RPL_RELATIONS_BUFFER_COUNT",
        "SMOLTCP_RPL_PARENTS_BUFFER_COUNT",
    ] {
        println!("cargo:rerun-if-env-changed={var}");
    }
}
