# redoubt-wire fuzz targets

The `ninep`, `typed` and `json` targets of [the wire formats](../../../docs/servers/wire.md), run
from here with `cargo +nightly fuzz run <target> corpus/<target> seeds/<target> -- -max_len=70000`.
