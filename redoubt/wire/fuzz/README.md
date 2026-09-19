# redoubt-wire fuzz targets

Host-only (TENETS.md 6: fuzz what parses), built by `cargo-fuzz` with the nightly pinned in
`rust-toolchain.toml`, and outside the workspace build. A finding is a panic, a hang, or a
disagreement with the target's own check:

| Target | Input | Checks |
| --- | --- | --- |
| `ninep` | a 9P2000 message or directory data | re-encoding gives the same bytes; encoding into a short buffer fails and leaves it zeroed; directory entries that do not fit are not written |
| `typed` | a request, reply or file of the fixture protocol (`tables/example.md`) | re-encoding gives the same words, buffer prefix or file bytes; handle counts; error replies |
| `json` | a file | agrees with serde_json (a host-only oracle) whenever we accept; plain syntax errors agree too; the unknown-member check refuses exactly the members not taken |

Run one for an hour, seeded from the vector files (`seeds/`, made from `vectors/`):

```
cd redoubt/wire/fuzz
cargo fuzz run typed corpus/typed seeds/typed -- -max_total_time=3600 -max_len=70000 -timeout=10
```

`-max_len=70000` lets inputs reach a 64 KiB message or file (libFuzzer's default is 4096), and
`-timeout=10` counts a slow input as a hang.

## Results
All runs on one x86-64 host, the three targets in parallel.

| Date | Code | Target | Time | Executions | Findings |
| --- | --- | --- | --- | --- | --- |
| 2026-09-19 | first version | `ninep` | 1 h | 888,025,121 | none |
| 2026-09-19 | first version | `typed` | 1 h | 1,974,355,649 | none |
| 2026-09-19 | first version | `json` | 1 h | 50,706,649 | none (a 60 s run before it found `-0` read as 0 where serde_json reads -0.0; `-0` is now refused, with a regression test in `json.rs`) |
| 2026-09-19 | after review (replies, file framing, atomic writes, table-driven 9P) | `ninep` | 10 min | 75,019,956 | none |
| 2026-09-19 | after review | `typed` | 10 min | 63,651,585 | none |
| 2026-09-19 | after review | `json` | 10 min | 3,604,251 | none |
| 2026-09-19 | WP-W2 (`Malformed` as code 1, handle kinds) | `ninep` | 10 min | 83,412,410 | none |
| 2026-09-19 | WP-W2 | `typed` | 10 min | 74,061,179 | none |
| 2026-09-19 | WP-W2 | `json` | 10 min | 11,120,945 | none |
