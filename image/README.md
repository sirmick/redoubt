# Image recipes

This directory is **source**: what goes into the signed boot bundle and the disk image, plus
the static files seeded into the filesystem. Everything it produces is written under
`target/image/` (gitignored).

| Source | What |
| --- | --- |
| `boot.toml` | The boot bundle's entries: the kernel, the servers and the `grants` manifest. |
| `disk.toml` | The virtio-blk disk: partition table and the littlefs volume. |
| `root/` | Static files seeded into the filesystem image (config, default layout). |

| Output (`target/image/`) | What |
| --- | --- |
| `redoubt.bundle` | `signature ‖ ustar`, the archive the loader verifies. Built by `./mkimage`. |
| `redoubt.img` | The disk image (partition table + littlefs), served by `blkd`/`fsd`. Not built yet. |
| `stage/` | The tree `redoubt.img` is packed from, after built artifacts are copied in. |

The loader verifies `redoubt.bundle` and starts its entries; the disk image is what the block
and filesystem servers expose once they exist (`docs/NAMESPACES.md`,
`docs/IO-ARCHITECTURE.md`). Today `./mkimage` builds a kernel-only bundle through the bench's
builder; `boot.toml` here is the intended source of truth for the fuller bundle.