# Image recipes

This directory is **source**: what goes into the signed boot bundle and the disk image, plus
the static files seeded into the filesystem. Everything it produces is written under
`target/image/` (gitignored).

| Source | What |
| --- | --- |
| `boot.toml` | Proposed boot recipe (not consumed yet): kernel, servers and the `grants` manifest. |
| `disk.toml` | Proposed disk recipe (not consumed yet): partition table and littlefs volume. |
| `root/` | Planned static filesystem contents; not generated or installed yet. |

| Output (`target/image/`) | What |
| --- | --- |
| `redoubt.bundle` | `signature ‖ ustar`, the archive the loader verifies. Built by `./mkimage`. |
| `redoubt.img` | The disk image (partition table + littlefs), served by `blkd`/`fsd`. Not built yet. |
| `stage/` | Planned staging tree for `redoubt.img`; not built yet. |

The loader verifies `redoubt.bundle` and starts its entries; the disk image is what the block
and filesystem servers expose once they exist (`docs/NAMESPACES.md`,
`docs/IO-ARCHITECTURE.md`). Today `./mkimage` builds a kernel-only bundle through the bench's
builder; `boot.toml` here is the intended source of truth for the fuller bundle.
