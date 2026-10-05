//! OpenSSH's server for the reference case, as a minimal Linux guest under the bench's own QEMU
//! (docs/testbench.md, "Sessions and the loopback server"): its image, built once per recipe from
//! Debian's packages, and each case's initramfs, that image with the case's files appended.
//!
//! The image is a kernel and a `newc` initramfs. The archive is written here: the bench's host
//! may have no `cpio`, and the format is a header per file and a trailer.

use std::io::Write;
use std::net::{TcpStream, ToSocketAddrs};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;

use crate::ssh::Unusable;
use crate::userland::name;

/// The guest's recipe, relative to the workspace.
const RECIPE: &str = "tests/ssh-reference/guest.toml";
/// Where built images live, relative to the workspace: one directory per recipe hash.
const IMAGES: &str = "target/ssh-reference";
/// Where the recipe's packages come from: a host that cannot reach it lacks the network.
const SOURCE: &str = "snapshot.debian.org:443";
/// The QEMU that boots the guest: the one the bench boots rv64 Redoubt with.
pub const QEMU: &str = "qemu-system-riscv64";
/// The builder's own part in the image, hashed into its tag with the recipe: any change to what
/// `build` or `Cpio` make of a recipe bumps it, so that no image built the old way is reused.
const BUILDER_VERSION: u32 = 1;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Recipe {
    /// A snapshot of Debian's archive, ending in '/': each package's `path` is under it.
    snapshot: String,
    /// The package whose `boot/vmlinux-*` is the kernel.
    kernel: String,
    /// The kernel's modules the guest loads, relative to its module directory.
    modules: Vec<String>,
    /// What the guest leaves out of the other packages' files.
    omit: Vec<String>,
    dirs: Vec<String>,
    /// `[link, target]`.
    links: Vec<[String; 2]>,
    file: Vec<File>,
    package: Vec<Package>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    path: String,
    mode: u32,
    text: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Package {
    name: String,
    version: String,
    path: String,
    sha256: String,
}

impl Recipe {
    fn parse(text: &str) -> Result<Recipe> {
        let recipe: Recipe = toml::from_str(text)?;
        ensure!(
            recipe.snapshot.starts_with("https://snapshot.debian.org/archive/")
                && recipe.snapshot.ends_with('/'),
            "snapshot {:?} is not a directory of snapshot.debian.org's archive",
            recipe.snapshot
        );
        ensure!(recipe.package.iter().any(|p| p.name == recipe.kernel), "no package {:?}", recipe.kernel);
        for package in &recipe.package {
            ensure!(
                package.sha256.len() == 64 && package.sha256.bytes().all(|b| b.is_ascii_hexdigit()),
                "{}: sha256 {:?} is not 64 hex digits",
                package.name,
                package.sha256
            );
            relative(&package.path)?;
        }
        let guest_paths = recipe.modules.iter().chain(&recipe.omit).chain(&recipe.dirs);
        let guest_paths = guest_paths.chain(recipe.links.iter().map(|[link, _]| link));
        for path in guest_paths.chain(recipe.file.iter().map(|f| &f.path)) {
            relative(path)?;
        }
        Ok(recipe)
    }
}

/// A path inside the guest or the archive: relative, and never out of it.
fn relative(path: &str) -> Result<&str> {
    ensure!(
        !path.is_empty()
            && !path.starts_with('/')
            && path.split('/').all(|part| !matches!(part, "" | "." | "..")),
        "{path:?} is not a plain relative path"
    );
    Ok(path)
}

/// The first 16 hex digits of the sha256 of the builder's version and the recipe: a changed
/// recipe, or a changed builder, is a new image.
fn tag(recipe: &[u8]) -> String {
    name(&[format!("builder {BUILDER_VERSION}\n").as_bytes(), recipe].concat())[..16].into()
}

/// The guest image of the workspace's recipe, built or not.
pub struct Image {
    dir: PathBuf,
}

impl Image {
    pub fn locate(workspace: &Path) -> Result<Image> {
        let recipe = workspace.join(RECIPE);
        let text = std::fs::read(&recipe).with_context(|| format!("reading {}", recipe.display()))?;
        Ok(Image { dir: workspace.join(IMAGES).join(tag(&text)) })
    }

    pub fn kernel(&self) -> PathBuf { self.dir.join("vmlinuz") }

    pub fn initrd(&self) -> PathBuf { self.dir.join("initrd.img") }

    /// Builds the image unless it exists: the one step that needs the network. What the host lacks
    /// for it (a tool, or the network to fetch with) is named as such; anything else, a package
    /// whose sha256 differs from the recipe's, say, is the recipe's fault, and the case fails.
    pub fn ensure(&self, workspace: &Path) -> Result<(), Unusable> {
        if self.kernel().exists() && self.initrd().exists() {
            return Ok(());
        }
        let building = self.dir.with_extension(format!("building-{}", std::process::id()));
        let built = build(workspace, &building).and_then(|()| {
            // Another bench may have built the same image meanwhile: either is the recipe's.
            match std::fs::rename(&building, &self.dir) {
                Err(_) if self.initrd().exists() => Ok(()),
                renamed => renamed.with_context(|| format!("renaming {}", building.display())),
            }
        });
        std::fs::remove_dir_all(&building).ok();
        built.map_err(|e| match e.downcast::<Unusable>() {
            Ok(unusable) => unusable,
            Err(e) => Unusable::Broken(format!("building the reference sshd's guest failed: {e:#}")),
        })
    }
}

/// Runs a host tool the build needs: one that is not installed is the host's lack.
fn tool(command: &mut Command) -> Result<std::process::Output> {
    let program = command.get_program().to_string_lossy().into_owned();
    let output = command.stdin(Stdio::null()).output().map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => {
            anyhow::Error::new(Unusable::Host(format!("the reference sshd's guest needs `{program}`")))
        }
        _ => anyhow::Error::new(e).context(format!("running {program}")),
    })?;
    ensure!(
        output.status.success(),
        "{program} failed: {}",
        String::from_utf8_lossy(&output.stderr).lines().last().unwrap_or("").trim()
    );
    Ok(output)
}

/// Builds the image into `out`: fetch and check each package, unpack it, shape the guest's root
/// and write the archive.
fn build(workspace: &Path, out: &Path) -> Result<()> {
    let recipe = Recipe::parse(&std::fs::read_to_string(workspace.join(RECIPE))?)?;
    std::fs::remove_dir_all(out).ok();
    let root = out.join("root");
    let kernel = out.join("kernel");
    std::fs::create_dir_all(&root)?;
    std::fs::create_dir_all(&kernel)?;
    let debs = workspace.join(IMAGES).join("debs");
    std::fs::create_dir_all(&debs)?;
    for package in &recipe.package {
        let deb = fetch(&format!("{}{}", recipe.snapshot, package.path), &package.sha256, &debs)?;
        let version = tool(Command::new("dpkg-deb").arg("-f").arg(&deb).arg("Version"))?;
        ensure!(
            String::from_utf8_lossy(&version.stdout).trim() == package.version,
            "{} is not {} {}",
            package.path,
            package.name,
            package.version
        );
        let into = if package.name == recipe.kernel { &kernel } else { &root };
        tool(Command::new("dpkg-deb").arg("-x").arg(&deb).arg(into))?;
    }

    let only = |dir: &Path, prefix: &str| -> Result<PathBuf> {
        let found: Vec<_> = std::fs::read_dir(dir)
            .with_context(|| format!("reading {}", dir.display()))?
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| {
                path.file_name().is_some_and(|name| name.as_bytes().starts_with(prefix.as_bytes()))
            })
            .collect();
        ensure!(found.len() == 1, "{} has {} entries {prefix}*, not one", dir.display(), found.len());
        Ok(found.into_iter().next().unwrap())
    };
    std::fs::copy(only(&kernel.join("boot"), "vmlinux-")?, out.join("vmlinuz"))?;
    let modules = only(&kernel.join("usr/lib/modules"), "")?;
    for omitted in &recipe.omit {
        let path = root.join(omitted);
        match path.symlink_metadata() {
            Ok(meta) if meta.is_dir() => std::fs::remove_dir_all(&path)?,
            Ok(_) => std::fs::remove_file(&path)?,
            Err(_) => bail!("omit names {omitted}, which no package has"),
        }
    }
    for dir in &recipe.dirs {
        std::fs::create_dir_all(root.join(dir))?;
    }
    for module in &recipe.modules {
        let name = Path::new(module).file_name().unwrap().to_string_lossy();
        let into = root.join("usr/lib/modules").join(name.strip_suffix(".xz").unwrap_or(&name));
        let from = modules.join(module);
        match module.ends_with(".xz") {
            true => std::fs::write(&into, tool(Command::new("xz").arg("-dc").arg(&from))?.stdout)?,
            false => std::fs::copy(&from, &into).map(drop)?,
        }
    }
    for [link, target] in &recipe.links {
        std::os::unix::fs::symlink(target, root.join(link)).with_context(|| format!("linking {link}"))?;
    }
    for file in &recipe.file {
        let path = root.join(&file.path);
        std::fs::write(&path, &file.text)?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(file.mode))?;
    }

    let mut archive = Cpio::new(std::io::BufWriter::new(std::fs::File::create(out.join("initrd.img"))?));
    archive.tree(&root, b"")?;
    // The kernel opens it for /init's output; without it, init has none.
    archive.char_device("dev/console", 0o600, 5, 1)?;
    archive.finish()?.flush()?;
    std::fs::remove_dir_all(&root)?;
    std::fs::remove_dir_all(&kernel)?;
    Ok(())
}

/// Fetches `url` into `cache`, named by its sha256, unless it is there: a file the recipe pins
/// never changes. A file that is not what the recipe says is the recipe's fault, or the
/// source's; a fetch that fails while the source does not answer is the host's.
fn fetch(url: &str, sha256: &str, cache: &Path) -> Result<PathBuf> {
    let path = cache.join(format!("{sha256}.deb"));
    if path.exists() {
        return Ok(path);
    }
    let partial = cache.join(format!(".{sha256}.{}", std::process::id()));
    let fetched =
        tool(Command::new("curl").args(["-fsSL", "--max-time", "900", "-o"]).arg(&partial).arg(url));
    if let Err(e) = fetched {
        std::fs::remove_file(&partial).ok();
        return Err(match e.downcast::<Unusable>() {
            Ok(unusable) => unusable.into(),
            Err(e) => fetch_failed(url, &format!("{e:#}"), SOURCE).into(),
        });
    }
    let found = name(&std::fs::read(&partial)?);
    if !found.eq_ignore_ascii_case(sha256) {
        std::fs::remove_file(&partial).ok();
        bail!(Unusable::Broken(format!("{url} has sha256 {found}, the recipe says {sha256}")));
    }
    std::fs::rename(&partial, &path)?;
    Ok(path)
}

/// Why fetching `url` failed: the host's lack if `source` does not answer, else the recipe's.
fn fetch_failed(url: &str, why: &str, source: &str) -> Unusable {
    match reachable(source) {
        false => Unusable::Host(format!("the reference sshd's guest: no network: {source} does not answer")),
        true => Unusable::Broken(format!("fetching {url}: {why}")),
    }
}

/// Whether `source` (host and port) resolves and accepts a TCP connection within five seconds.
fn reachable(source: &str) -> bool {
    let timeout = Duration::from_secs(5);
    source
        .to_socket_addrs()
        .is_ok_and(|mut addrs| addrs.any(|addr| TcpStream::connect_timeout(&addr, timeout).is_ok()))
}

/// The case's own initramfs: the image's, then an archive of the case's files under /case, which
/// the kernel unpacks over it. `files` are (name, mode, contents).
pub fn case_initrd(image: &Image, files: &[(&str, u32, &[u8])], into: &Path) -> Result<()> {
    let base =
        std::fs::read(image.initrd()).with_context(|| format!("reading {}", image.initrd().display()))?;
    let mut archive = Cpio::new(base);
    for (name, mode, data) in files {
        archive.file(&format!("case/{}", relative(name)?), *mode, data)?;
    }
    std::fs::write(into, archive.finish()?).with_context(|| format!("writing {}", into.display()))
}

/// A `newc` archive (the kernel's initramfs format): per entry a header of "070701" and thirteen
/// 8-digit hex fields, the name and a NUL padded to four bytes, the data padded to four bytes;
/// then the entry `TRAILER!!!`. Every entry is root's, dated 0, with a link count of one.
struct Cpio<W: Write> {
    out: W,
    inode: u32,
}

impl<W: Write> Cpio<W> {
    fn new(out: W) -> Cpio<W> { Cpio { out, inode: 0 } }

    fn entry(&mut self, name: &[u8], mode: u32, device: (u32, u32), data: &[u8]) -> std::io::Result<()> {
        self.inode += 1;
        self.write(self.inode, name, mode, device, data)
    }

    fn write(
        &mut self,
        inode: u32,
        name: &[u8],
        mode: u32,
        device: (u32, u32),
        data: &[u8],
    ) -> std::io::Result<()> {
        let size = u32::try_from(data.len()).map_err(std::io::Error::other)?;
        let name_size = name.len() as u32 + 1;
        let fields = [inode, mode, 0, 0, 1, 0, size, 0, 0, device.0, device.1, name_size, 0];
        let mut header = String::from("070701");
        for field in fields {
            header += &format!("{field:08X}");
        }
        self.out.write_all(header.as_bytes())?;
        self.out.write_all(name)?;
        self.out.write_all(&[0; 4][..1 + pad(110 + name_size as usize)])?;
        self.out.write_all(data)?;
        self.out.write_all(&[0; 3][..pad(data.len())])
    }

    fn file(&mut self, name: &str, mode: u32, data: &[u8]) -> std::io::Result<()> {
        self.entry(name.as_bytes(), libc::S_IFREG | mode, (0, 0), data)
    }

    fn char_device(&mut self, name: &str, mode: u32, major: u32, minor: u32) -> std::io::Result<()> {
        self.entry(name.as_bytes(), libc::S_IFCHR | mode, (major, minor), &[])
    }

    /// Everything under `dir`, as `prefix` followed by its path there, in name order.
    fn tree(&mut self, dir: &Path, prefix: &[u8]) -> Result<()> {
        let mut entries: Vec<_> = std::fs::read_dir(dir)?.collect::<std::io::Result<_>>()?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let name = [prefix, entry.file_name().as_bytes()].concat();
            let meta = entry.path().symlink_metadata()?;
            let mode = meta.permissions().mode() & 0o7777;
            if meta.is_symlink() {
                let target = std::fs::read_link(entry.path())?;
                self.entry(&name, libc::S_IFLNK | 0o777, (0, 0), target.as_os_str().as_bytes())?;
            } else if meta.is_dir() {
                // Whatever the umask made of it: sshd refuses a privilege-separation directory
                // that anyone but root may write, or that is under one.
                self.entry(&name, libc::S_IFDIR | 0o755, (0, 0), &[])?;
                self.tree(&entry.path(), &[&name[..], b"/"].concat())?;
            } else if meta.is_file() {
                self.entry(&name, libc::S_IFREG | mode, (0, 0), &std::fs::read(entry.path())?)?;
            } else {
                bail!("{} is neither a file, a directory nor a link", entry.path().display());
            }
        }
        Ok(())
    }

    fn finish(mut self) -> std::io::Result<W> {
        self.write(0, b"TRAILER!!!", 0, (0, 0), &[])?;
        Ok(self.out)
    }
}

/// Bytes after `len` to the next multiple of four.
fn pad(len: usize) -> usize { (4 - len % 4) % 4 }

#[cfg(test)]
mod tests {
    use super::*;

    /// A two-entry archive is exactly its headers, names, data, padding and trailer; padding
    /// reaches the next multiple of four.
    #[test]
    fn the_cpio_writer_writes_newc() {
        let mut archive = Cpio::new(Vec::new());
        archive.file("case/a", 0o600, b"hello").unwrap();
        archive.char_device("dev/console", 0o600, 5, 1).unwrap();
        let bytes = archive.finish().unwrap();
        let expected = concat!(
            // inode, mode, uid, gid, links, mtime, size, device major and minor, its rdev major
            // and minor, name size, check; 110 + 7 bytes, then 3 of padding; 5 of data, then 3.
            "070701",
            "00000001",
            "00008180",
            "00000000",
            "00000000",
            "00000001",
            "00000000",
            "00000005",
            "00000000",
            "00000000",
            "00000000",
            "00000000",
            "00000007",
            "00000000",
            "case/a\0",
            "\0\0\0",
            "hello",
            "\0\0\0",
            // 110 + 12, then 2; no data.
            "070701",
            "00000002",
            "00002180",
            "00000000",
            "00000000",
            "00000001",
            "00000000",
            "00000000",
            "00000000",
            "00000000",
            "00000005",
            "00000001",
            "0000000C",
            "00000000",
            "dev/console\0",
            "\0\0",
            // 110 + 11, then 3.
            "070701",
            "00000000",
            "00000000",
            "00000000",
            "00000000",
            "00000001",
            "00000000",
            "00000000",
            "00000000",
            "00000000",
            "00000000",
            "00000000",
            "0000000B",
            "00000000",
            "TRAILER!!!\0",
            "\0\0\0",
        );
        assert_eq!(String::from_utf8_lossy(&bytes), expected);
        assert_eq!([0, 1, 2, 3, 4, 5].map(pad), [0, 3, 2, 1, 0, 3]);
    }

    /// The committed recipe parses, pins every package by sha256 and names its kernel; its tag is
    /// 16 hex digits that change with any byte of it; a path out of the guest is refused.
    #[test]
    fn the_guest_recipe_parses_and_hashes() {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let text = std::fs::read_to_string(workspace.join(RECIPE)).unwrap();
        let recipe = Recipe::parse(&text).unwrap();
        assert!(recipe.package.iter().any(|p| p.name == "openssh-server"));
        assert!(recipe.file.iter().any(|f| f.path == "init" && f.mode == 0o755));
        let tag_of = tag(text.as_bytes());
        assert_eq!(tag_of.len(), 16);
        assert!(tag_of.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_ne!(tag(format!("{text} ").as_bytes()), tag_of);
        for bad in ["../etc", "/etc", "a//b", "a/./b", ""] {
            let changed = text.replacen("\"usr/share\"", &format!("{bad:?}"), 1);
            assert!(Recipe::parse(&changed).is_err(), "{bad:?}");
        }
        let unpinned = text.replacen("sha256 = \"", "sha256 = \"x", 1);
        assert!(Recipe::parse(&unpinned).is_err());
    }

    /// A package whose sha256 is not the recipe's fails the case, and is not kept; one that is
    /// is kept under its sha256. A fetch that fails while its source does not answer is the
    /// host's lack, and while it answers, the recipe's fault.
    #[test]
    fn a_bad_package_is_broken_and_no_network_is_the_hosts() {
        let dir = std::env::temp_dir().join(format!("ssh-guest-fetch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let package = dir.join("package.deb");
        std::fs::write(&package, b"package").unwrap();
        let url = format!("file://{}", package.display());
        let (right, wrong) = (name(b"package"), name(b"other"));
        let refused = fetch(&url, &wrong, &dir).unwrap_err().downcast::<Unusable>().unwrap();
        assert!(matches!(&refused, Unusable::Broken(why) if why.contains("the recipe says")), "{refused:?}");
        assert!(!dir.join(format!("{wrong}.deb")).exists());
        assert_eq!(fetch(&url, &right, &dir).unwrap(), dir.join(format!("{right}.deb")));
        std::fs::remove_dir_all(&dir).unwrap();
        // Nothing listens on a port just closed; something listens on one just opened.
        let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap();
        let open = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let open_addr = open.local_addr().unwrap();
        assert!(
            matches!(fetch_failed("u", "404", &closed.to_string()), Unusable::Host(why) if why.contains("no network"))
        );
        assert!(
            matches!(fetch_failed("u", "404", &open_addr.to_string()), Unusable::Broken(why) if why.contains("404"))
        );
    }
}
