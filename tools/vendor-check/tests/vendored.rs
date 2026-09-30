//! The vendored crates are the published ones, unmodified, and they are the ones that build
//! (vendor/README.md).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use redoubt_keyd::sha256;

/// Each vendored crate: its directory under `vendor/`, its version, and the SHA-256 of the
/// `.crate` file crates.io published, as its index records it.
const VENDORED: &[(&str, &str, &str)] = &[
    ("smoltcp", "0.14.0", "b6f8b28ad56c6e35524a37dd492af5d1a47e31e1a4d175cd12f89c075f01980f"),
    ("managed", "0.8.0", "0ca88d725a0a943b096803bd34e73a4437208b6077654cc4ecb2947a5f91618d"),
    ("heapless", "0.9.3", "25ba4bd83f9415b58b4ed8dc5714c76e626a105be4646c02630ad730ad3b5aa4"),
    ("hash32", "0.3.1", "47d60b12902ba28e2730cd37e95b8c9223af2808df9e902d4df49588d1470606"),
    ("stable_deref_trait", "1.2.1", "6ce2be8dc25455e1f91df71bfa12ad37d7af1092ae736f3a6cd0e37bc7810596"),
    ("byteorder", "1.5.0", "1fd0f2584146f6f2ef48085050886acf353beff7305ebd1ae69500e27c67f64b"),
    ("ed25519-compact", "2.4.2", "f05391a505666bdf2b5d2626f41b7f0f49052b1e33cceac960eaa818008141da"),
    ("sunset", "0.6.0", "3b52312b804ac95f10d3963f6ef5889117ad678bd6ce977be4aa4cf491bad78f"),
    ("sunset-sshwire-derive", "0.3.0", "d40354cdc622342c11b742e91a5c151ebd59cbd3bc507f6146e9e6de6791f75f"),
    ("aes", "0.9.3", "35f0f96ce78e38c3dc6d8948aa8163d06385be74000f3c7a95bf1eef35d3ea32"),
    ("ctr", "0.10.1", "baaca1c4b237092596f64d571e9db6ce4109c4ef9742e27590f1709594461f21"),
    ("chacha20", "0.10.2", "65c35e4b699c7e15ccbe7ee35c005e4fc0a278d22238a2857e6ce2dadeda1b06"),
    ("poly1305", "0.9.1", "6e2d0073b297041425c7c3df6eb4792d598a15323fe63346852b092eca02904c"),
    ("universal-hash", "0.6.1", "f4987bdc12753382e0bec4a65c50738ffaabc998b9cdd1f952fb5f39b0048a96"),
    ("hmac", "0.13.0", "6303bc9732ae41b04cb554b844a762b4115a61bfaa81e3e83050991eeb56863f"),
    ("sha2", "0.11.0", "446ba717509524cb3f22f17ecc096f10f4822d76ab5c0b9822c5f9c284e825f4"),
    ("digest", "0.11.3", "f1dd6dbb5841937940781866fa1281a1ff7bd3bf827091440879f9994983d5c2"),
    ("cipher", "0.5.2", "e8cf2a2c93cd704877c0858356ed03480ff301ee950b43f1cbe4573b088bfa6c"),
    ("crypto-common", "0.2.2", "ce6e4c961d6cd6c9a86db418387425e8bdeaf05b3c8bc1411e6dca4c252f1453"),
    ("inout", "0.2.2", "4250ce6452e92010fdf7268ccc5d14faa80bb12fc741938534c58f16804e03c7"),
    ("block-buffer", "0.12.1", "d2f6c7dbe95a6ed67ad9f18e57daf93a2f034c524b99fd2b76d18fdfeb6660aa"),
    ("hybrid-array", "0.4.15", "27f864f10dfb56725ce5ce5472bc52252c8f93a4ab86327122cebf62c5f59a17"),
    ("typenum", "1.20.1", "b6f5e870be6c3b371b77fe0ee0bafb859fa4964b4404c27de1d380043c4dda20"),
    ("ctutils", "0.4.2", "7d5515a3834141de9eafb9717ad39eea8247b5674e6066c404e8c4b365d2a29e"),
    ("cmov", "0.5.4", "0c9ea0ac24bc397ab3c98583a3c9ba74fa56b09a4449bbe172b9b1ddb016027a"),
    ("cpubits", "0.1.1", "15b85f9c39137c3a891689859392b1bd49812121d0d61c9caf00d46ed5ce06ae"),
    ("subtle", "2.6.1", "13c2bddecc57b384dee18652358fb23172facb8a2c51ccc10d74c157bdea3292"),
    ("zeroize", "1.9.0", "e13c156562582aa81c60cb29407084cdb54c4164760106ab78e6c5b0858cf64e"),
    ("zeroize_derive", "1.5.0", "3c50655cbb0fe3fc43170059e702f1ce5e19b84cec58dc87b037a09935c2f328"),
    ("ascii", "1.1.0", "d92bec98840b8f03a5ff5413de5293bfcd8bf96467cf5452609f939ec6f5de16"),
    ("snafu", "0.9.2", "e45cb604038abb7b926b679887b3226d8d0f23874b66623625a0454be425a4b7"),
    ("snafu-derive", "0.9.2", "287f59010008f0d7cf5e3b03196d666c1acc46c8d3e9cf34c28a1a7157601e72"),
    ("virtue", "0.0.17", "7302ac74a033bf17b6e609ceec0f891ca9200d502d31f02dc7908d3d98767c9d"),
    ("getrandom", "0.4.3", "300e883d756b2e4ec94e02791f39b04b522276138852cfc41d9fb7e904106099"),
];

/// The vendored crates' dependencies left to `Cargo.lock` (vendor/README.md says why): already
/// locked from crates.io for other packages, which a patch would move too. Pinned here to the
/// version and checksum they have now.
const LOCKED: &[(&str, &str, &str)] = &[
    ("cfg-if", "1.0.0", "baf1de4339761588bc0619e3cbc0120ee582ebb74b53b4efbf79117bd2da40fd"),
    ("bitflags", "1.3.2", "bef38d45163c2f1dde094a7dfd33ccf595c92905c8f8f4fdc18d06fb1037718a"),
    ("log", "0.4.22", "a7a70ba024b9dc04c27ea2f0c0548feb474ec5c54bba33a7f72f873a39d07b24"),
    ("proc-macro2", "1.0.86", "5e719e8df665df0d1c8fbfd238015744736151d4445ec0836b8e628aae103b77"),
    ("quote", "1.0.35", "291ec9ab5efd934aaf503a6466c5d5251535d108ee747472c3977cc5acc868ef"),
    ("syn", "2.0.87", "25aa4ce346d03a6dcd68dd8b4010bcb74e54e62c90c573f394c46eae99aba32d"),
    ("unicode-ident", "1.0.12", "3354b9ac3fae1ff6755cb6db53683adb661634f67557942dea4facebec0fee4b"),
    ("heck", "0.5.0", "2304e00983f87ffb38b55b444b5e3b60a884b5d30c0fca7d82fe33449bbe55ea"),
];

/// beamlet's own vendored crates (vendor/README.md, "beamlet's crates"), built by its own
/// workspace, `userland/otp`, through the patches in its manifest.
const BEAMLET: &[(&str, &str, &str)] = &[
    ("adler2", "2.0.1", "320119579fcad9c21884f5c4861d16174d0e06250625266f50fe6898340abefa"),
    ("aead", "0.6.1", "1973cfbc1a2daf9cf550e74e1f088c28e7f7d8c1e1418fb6c9dc5184b7e84c99"),
    ("aes-gcm", "0.11.1", "7f2b8006a0c83f52b62ba44a97b58bf76fe2f70a329e588f67f89691d93d498f"),
    ("base16ct", "1.0.0", "fd307490d624467aa6f74b0eabb77633d1f758a7b25f12bceb0b22e08d9726f6"),
    ("cbc", "0.2.1", "ce2dc9ee5f88d11e0beb842c88b33c8a5cf0d1329c4b19494af42b07dbfe8896"),
    ("chacha20poly1305", "0.11.0", "9b89e1c441e926b9c82a8d023f6e1b7ae0adcfaa7d621814e4d60789bac751cb"),
    ("const-oid", "0.10.2", "a6ef517f0926dd24a1582492c791b6a4818a4d94e789a334894aa15b0d12f55c"),
    ("crypto-bigint", "0.7.5", "1a52aa3fcda4e6302a9f48734f234d35d4721b96f8fe07d073f07ce9df4f0271"),
    ("crypto-primes", "0.7.2", "3633a51a39c69ebbaa4feaa694bd83d241e4093901c84a0963b19d9bb3f0cf8f"),
    ("der", "0.8.2", "a878c850e9e421b20262e9b41f9c860e4785fa07541c266b62ff9d1ef998a80a"),
    ("ecdsa", "0.17.0", "c0681a4fc24c767085329728d8dfba959af91228aa4610cca4f8ce317ba46ae0"),
    ("elf", "0.8.0", "55dd888a213fc57e957abf2aa305ee3e8a28dbe05687a251f33b637cd46b0070"),
    ("elliptic-curve", "0.14.1", "9d65aa39b3a5c1c9c1b745c9a019234bb7a21b77abcb4f4d266d706e2d577d65"),
    ("ff", "0.14.0", "a1f686ab92a9fb0eaf188f6c6c87b89490baa6fdb0db4544ba4dc47f7942489f"),
    ("ghash", "0.6.0", "2eecf2d5dc9b66b732b97707a0210906b1d30523eb773193ab777c0c84b3e8d5"),
    ("group", "0.14.0", "7fd1a1c7a5206c5b7a3f5a0d7ccd3ff85d0c8f5133d62a02680255b0004af5f4"),
    ("hkdf", "0.13.0", "4aaa26c720c68b866f2c96ef5c1264b3e6f473fe5d4ce61cd44bbe913e553018"),
    ("keccak", "0.2.2", "d8f198d1db720e4940b5a493201d199d9f24f568f8f746bd13706243a2f71598"),
    ("libm", "0.2.16", "b6d2cec3eae94f9f509c767b45932f1ada8350c4bdb85af2fcab4a3c14807981"),
    ("md-5", "0.11.0", "69b6441f590336821bb897fb28fc622898ccceb1d6cea3fde5ea86b090c4de98"),
    ("miniz_oxide", "0.9.1", "b63fbc4a50860e98e7b2aa7804ded1db5cbc3aff9193adaff57a6931bf7c4b4c"),
    ("num-bigint", "0.4.8", "c89e69e7e0f03bea5ef08013795c25018e101932225a656383bd384495ecc367"),
    ("num-integer", "0.1.47", "7ce2d95d4b3734dc35aa2f45e1aa22cd416814592a4f9d9205e11affd5b8e10b"),
    ("num-traits", "0.2.19", "071dfc062690e90b734c0b2273ce72ad0ffa95f0c74596bc250dcfd960262841"),
    ("p256", "0.14.0", "d2c9239b2dbc807adbbe147e8cf72ea7450c3a0aabe62cb8e75ff4ec22e1f72a"),
    ("p384", "0.14.0", "d17b851e6b3e378ab4ecb07fa2ed23f4d15f075735f8fec9fa1e7bdce5f8301f"),
    ("pbkdf2", "0.13.0", "112d82ceb8c5bf524d9af484d4e4970c9fd5a0cc15ba14ad93dccd28873b0629"),
    ("polyval", "0.7.3", "f0fa31d631f2b2cb2a544d0aa321ce847a94764d701ca2becc411138b93d49cd"),
    ("primefield", "0.14.0", "c555a6e4eb7d4e158fcb028c835c3b8642206ddc279b5c6b202ef9a8bdb592f4"),
    ("primeorder", "0.14.0", "5c9f42978c78a00e3d68f69fc03e57a234debae69da4020a4fb588fcdcd07b06"),
    ("rand_core", "0.10.1", "63b8176103e19a2643978565ca18b50549f6101881c443590420e4dc998a3c69"),
    ("regex-automata", "0.4.18", "ad8553b9b26413251cbf30e620595c7a41b3887f03da04579c0e6b0d6a06b4b2"),
    ("regex-syntax", "0.8.11", "d6f6ff9a378485b298a5286656da665ba74413d36db0979633275d2e708145d4"),
    ("rfc6979", "0.6.0", "b4a459cddafb3fe76b31fd8f1108007566c40301feb64dc7b54656eb7388172b"),
    ("rsa", "0.10.0-rc.18", "30b2aa4ba0d89f73d1e332df05be0eeab8840351c36ca5654341dfdb57bb3caf"),
    ("ryu", "1.0.23", "9774ba4a74de5f7b1c1451ed6cd5285a32eddb5cccb8cc655a4e50009e06477f"),
    ("sec1", "0.8.1", "d56d437c2f19203ce5f7122e507831de96f3d2d4d3be5af44a0b0a09d8a80e4d"),
    ("sha1", "0.11.0", "aacc4cc499359472b4abe1bf11d0b12e688af9a805fa5e3016f9a386dc2d0214"),
    ("sha3", "0.12.0", "bc9bad02c26382724b2d2692c6f179285e4b54eeecd7968f52a50059c3c11759"),
    ("signature", "3.0.0", "28d567dcbaf0049cb8ac2608a76cd95ff9e4412e1899d389ee400918ca7537f5"),
    ("sponge-cursor", "0.1.0", "3a0219bd7d979d58245a4f41f695e1ac9f8befdffadd7f61f1bae9e39abc6620"),
    ("wnaf", "0.14.1", "795ca18b3fdb5e62bf982199278341ddcf7ebf7d32e25e212ad05d496e95f6fa"),
];

/// beamlet's crates left to its `Cargo.lock`, pinned.
const BEAMLET_LOCKED: &[(&str, &str, &str)] =
    &[("cfg-if", "1.0.5", "4e7648175b45a9a48536d676f68d918270699102aa8dab5496df06904c914600")];

/// Every vendored crate, the root's users' and beamlet's.
fn all() -> impl Iterator<Item = &'static (&'static str, &'static str, &'static str)> {
    VENDORED.iter().chain(BEAMLET)
}

/// beamlet's workspace's vendored crates: its own, and those of the root's it builds too.
fn in_beamlet(lock: &str) -> Vec<&'static (&'static str, &'static str, &'static str)> {
    all().filter(|(name, ..)| !locked(lock, name).is_empty()).collect()
}

fn root() -> PathBuf { Path::new(env!("CARGO_MANIFEST_DIR")).join("../..") }

fn hex(bytes: &[u8]) -> String { bytes.iter().map(|b| format!("{b:02x}")).collect() }

/// Every file under `dir`, as paths relative to `base`, with `/` separators.
fn files(base: &Path, dir: &Path, out: &mut BTreeSet<String>) {
    for entry in std::fs::read_dir(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display())) {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files(base, &path, out);
        } else {
            let rel = path.strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/");
            out.insert(rel);
        }
    }
}

/// A scratch directory under the system's temporary directory, removed when dropped. Each has its
/// own name, since the tests run in parallel.
struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("vendor-check-{tag}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); }
}

/// Copies the directory `from` to `to`, which must not exist yet.
fn copy_dir(from: &Path, to: &Path) {
    let status = std::process::Command::new("cp").arg("-R").arg(from).arg(to).status().expect("run cp");
    assert!(status.success(), "copying {} failed", from.display());
}

/// Where crate `name`'s published bytes are, under `vendor`: `vendor/<name>` itself, or for a
/// patched crate (one with `vendor/patches/<name>.patch`) a copy in `scratch` with the patch
/// reversed.
fn published(vendor: &Path, name: &str, scratch: &Path) -> Result<PathBuf, String> {
    let patch = vendor.join("patches").join(format!("{name}.patch"));
    if !patch.exists() {
        return Ok(vendor.join(name));
    }
    let copy = scratch.join(name);
    copy_dir(&vendor.join(name), &copy);
    // --force never asks; a patch that does not reverse cleanly fails instead.
    let out = std::process::Command::new("patch")
        .args(["--reverse", "--force", "--silent", "--no-backup-if-mismatch", "-p1", "-d"])
        .arg(&copy)
        .arg("-i")
        .arg(&patch)
        .output()
        .expect("run patch");
    if !out.status.success() {
        return Err(format!(
            "vendor/patches/{name}.patch does not reverse cleanly on vendor/{name}: {}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(copy)
}

/// Checks `vendor`'s crates `names` against `sums` (the text of `SHA256SUMS`): every listed
/// file is the published file, no file is listed twice, missing or unlisted, and each crate has
/// files listed. A patched crate is checked with its patch reversed, so its patch file is the
/// whole difference from the published bytes.
fn check_published(vendor: &Path, names: &[&str], sums: &str) -> Result<(), String> {
    let scratch = Scratch::new("published");
    let mut dirs = std::collections::BTreeMap::new();
    for name in names {
        dirs.insert(name.to_string(), published(vendor, name, &scratch.0)?);
    }
    let mut listed = BTreeSet::new();
    for line in sums.lines() {
        let (sum, path) = line.split_once("  ").ok_or(format!("malformed line {line:?}"))?;
        let (name, rest) = path.split_once('/').ok_or(format!("{path}: not in a crate"))?;
        let dir = dirs.get(name).ok_or(format!("vendor/{path}: {name} is not a vendored crate"))?;
        let bytes = std::fs::read(dir.join(rest)).map_err(|e| format!("vendor/{path}: {e}"))?;
        if hex(&sha256::hash(&bytes)) != sum {
            return Err(format!("vendor/{path} is not the published file"));
        }
        if !listed.insert(path.to_string()) {
            return Err(format!("vendor/{path} listed twice"));
        }
    }
    let mut present = BTreeSet::new();
    for (name, dir) in &dirs {
        let mut crate_files = BTreeSet::new();
        files(dir, dir, &mut crate_files);
        present.extend(crate_files.into_iter().map(|f| format!("{name}/{f}")));
        if !listed.iter().any(|p| p.starts_with(&format!("{name}/"))) {
            return Err(format!("nothing listed for {name}"));
        }
    }
    let added: Vec<_> = present.difference(&listed).collect();
    let removed: Vec<_> = listed.difference(&present).collect();
    if !added.is_empty() {
        return Err(format!("files not in vendor/SHA256SUMS: {added:?}"));
    }
    if !removed.is_empty() {
        return Err(format!("files listed but missing: {removed:?}"));
    }
    Ok(())
}

/// `vendor/SHA256SUMS` lists every vendored file with its SHA-256, each file is exactly that,
/// and no file is missing from the list or from the tree; a patched crate differs from those
/// bytes by its patch and nothing else. The sums were taken from the published crates when they
/// were vendored, so this is integrity since vendoring; `provenance.sh` checks against crates.io.
#[test]
fn vendored_files_are_the_published_bytes() {
    let vendor = root().join("vendor");
    let sums = std::fs::read_to_string(vendor.join("SHA256SUMS")).expect("vendor/SHA256SUMS");
    let names: Vec<&str> = all().map(|(name, ..)| *name).collect();
    check_published(&vendor, &names, &sums).unwrap();
}

/// A scratch `vendor/` holding one patched crate, `ascii`, with its patch and its sums; the
/// function `change` edits it before the check.
fn check_patched_ascii(tag: &str, change: impl FnOnce(&Path)) -> Result<(), String> {
    let real = root().join("vendor");
    let scratch = Scratch::new(tag);
    let vendor = scratch.0.join("vendor");
    std::fs::create_dir_all(vendor.join("patches")).unwrap();
    copy_dir(&real.join("ascii"), &vendor.join("ascii"));
    std::fs::copy(real.join("patches/ascii.patch"), vendor.join("patches/ascii.patch")).unwrap();
    let sums = std::fs::read_to_string(real.join("SHA256SUMS")).unwrap();
    let sums: String = sums.lines().filter(|l| l.contains("  ascii/")).map(|l| format!("{l}\n")).collect();
    change(&vendor);
    check_published(&vendor, &["ascii"], &sums)
}

/// A patched crate passes as vendored, and fails when it differs from its published bytes by
/// anything but its patch: an edit elsewhere in the crate, an edit to what the patch changed, or
/// its patch file gone.
#[test]
fn a_patched_crate_differs_only_by_its_patch() {
    check_patched_ascii("as-is", |_| {}).unwrap();

    let err = check_patched_ascii("elsewhere", |v| {
        let readme = v.join("ascii/README.md");
        let mut text = std::fs::read_to_string(&readme).unwrap();
        text.push_str("an unrecorded change\n");
        std::fs::write(readme, text).unwrap();
    })
    .unwrap_err();
    assert!(err.contains("vendor/ascii/README.md is not the published file"), "{err}");

    let err = check_patched_ascii("in-the-patch", |v| {
        let file = v.join("ascii/src/ascii_char.rs");
        let text = std::fs::read_to_string(&file).unwrap();
        std::fs::write(&file, text.replacen("fn from(ch: AsciiChar)", "fn from(chr: AsciiChar)", 1)).unwrap();
    })
    .unwrap_err();
    assert!(err.contains("vendor/patches/ascii.patch does not reverse cleanly"), "{err}");

    let err =
        check_patched_ascii("no-patch", |v| std::fs::remove_file(v.join("patches/ascii.patch")).unwrap())
            .unwrap_err();
    assert!(err.contains("vendor/ascii/src/ascii_char.rs is not the published file"), "{err}");
}

/// Every patch in `vendor/patches/` is `<crate>.patch` for a vendored crate, and vendor/README.md
/// has that crate's section, saying what the patch changes and why.
#[test]
fn every_patch_is_a_vendored_crates_and_recorded() {
    let readme = std::fs::read_to_string(root().join("vendor/README.md")).expect("vendor/README.md");
    let mut patched = 0;
    for entry in std::fs::read_dir(root().join("vendor/patches")).expect("vendor/patches") {
        let file = entry.unwrap().file_name().to_string_lossy().into_owned();
        let name =
            file.strip_suffix(".patch").unwrap_or_else(|| panic!("vendor/patches/{file}: not a .patch"));
        assert!(VENDORED.iter().any(|(n, ..)| *n == name), "vendor/patches/{file}: {name} is not vendored");
        let heading = format!("### `{name}`");
        assert!(readme.lines().any(|l| l == heading), "vendor/README.md has no section {heading:?}");
        patched += 1;
    }
    assert!(patched > 0, "vendor/patches/ holds no patch");
}

/// Every vendored file is in git's index. A crate's own `.gitignore` (many ignore `Cargo.lock`)
/// applies inside `vendor/`, so a file can be on disk, pass the checks above, and still be left
/// out of a commit.
#[test]
fn every_vendored_file_is_tracked() {
    let out = std::process::Command::new("git")
        .current_dir(root())
        .args(["ls-files", "-z", "--", "vendor"])
        .output()
        .expect("run git ls-files");
    assert!(out.status.success(), "git ls-files failed");
    let tracked: BTreeSet<String> = String::from_utf8(out.stdout)
        .expect("UTF-8 paths")
        .split('\0')
        .filter_map(|p| p.strip_prefix("vendor/"))
        .map(String::from)
        .collect();
    let vendor = root().join("vendor");
    let mut present = BTreeSet::new();
    for (name, ..) in all() {
        files(&vendor, &vendor.join(name), &mut present);
    }
    let untracked: Vec<_> = present.difference(&tracked).collect();
    assert!(
        untracked.is_empty(),
        "{} vendored files git does not track (git add -f them), the first: {:?}",
        untracked.len(),
        &untracked[..untracked.len().min(10)]
    );
}

/// The `[[package]]` blocks of `Cargo.lock` named `name`, each as its lines.
fn locked(lock: &str, name: &str) -> Vec<Vec<String>> {
    lock.split("[[package]]")
        .map(|block| block.lines().map(str::trim).map(String::from).collect::<Vec<_>>())
        .filter(|lines| lines.iter().any(|l| *l == format!("name = \"{name}\"")))
        .collect()
}

/// `Cargo.lock` builds each vendored crate from its path (a path package has no `source`),
/// at the vendored version, and has no other copy of it; the crates left to the lockfile are
/// the pinned registry versions. So does beamlet's, for its crates and the root's it builds.
#[test]
fn the_vendored_copies_are_the_ones_that_build() {
    let lock = std::fs::read_to_string(root().join("Cargo.lock")).expect("Cargo.lock");
    built_from_vendor(&lock, VENDORED.iter(), LOCKED);
    let beamlet =
        std::fs::read_to_string(root().join("userland/otp/Cargo.lock")).expect("userland/otp/Cargo.lock");
    assert!(
        BEAMLET.iter().all(|(name, ..)| !locked(&beamlet, name).is_empty()),
        "a crate of BEAMLET is not in beamlet's lock"
    );
    built_from_vendor(&beamlet, in_beamlet(&beamlet).into_iter(), BEAMLET_LOCKED);
}

fn built_from_vendor<'a>(
    lock: &str,
    vendored: impl Iterator<Item = &'a (&'a str, &'a str, &'a str)>,
    left: &[(&str, &str, &str)],
) {
    for (name, version, _) in vendored {
        let blocks = locked(lock, name);
        assert_eq!(
            blocks.len(),
            1,
            "{name}: expected exactly one copy in Cargo.lock, found {}",
            blocks.len()
        );
        let block = &blocks[0];
        assert!(block.contains(&format!("version = \"{version}\"")), "{name}: not version {version}");
        assert!(
            !block.iter().any(|l| l.starts_with("source") || l.starts_with("checksum")),
            "{name}: Cargo.lock takes it from a registry, not vendor/{name}"
        );
    }
    for (name, version, checksum) in left {
        let blocks = locked(lock, name);
        let pinned = blocks.iter().any(|b| {
            b.contains(&format!("version = \"{version}\""))
                && b.contains(
                    &"source = \"registry+https://github.com/rust-lang/crates.io-index\"".to_string(),
                )
                && b.contains(&format!("checksum = \"{checksum}\""))
        });
        assert!(pinned, "{name} {version} is not locked from crates.io with checksum {checksum}");
    }
}

/// Every `"manifest_path":"..."` in `cargo metadata`'s JSON: the resolved graph's packages.
fn manifest_paths(metadata: &str) -> BTreeSet<String> {
    let key = "\"manifest_path\":\"";
    metadata
        .match_indices(key)
        .map(|(at, _)| {
            let rest = &metadata[at + key.len()..];
            rest[..rest.find('"').expect("unterminated manifest_path")].to_string()
        })
        .collect()
}

/// A path package has no source in `Cargo.lock` wherever its directory is, so the lockfile
/// alone cannot show that the patches point at `vendor/`. The resolved graph can: each
/// vendored crate's manifest is `vendor/<name>/Cargo.toml` in this tree, and no registry copy
/// (`.../<name>-<version>/Cargo.toml`) of it is in the graph at all. The riscv64 filter keeps
/// `--offline` from needing host-only crates the registry cache may lack.
#[test]
fn the_patches_point_at_vendor() {
    let root = root().canonicalize().expect("repository root");
    graph_from_vendor(&root, &root, VENDORED.iter(), true);
    // beamlet's own crates are all compiled for the target; of the root's it shares, some are
    // compiled only on the host, so those need only have no registry copy in its graph.
    graph_from_vendor(&root, &root.join("userland/otp"), BEAMLET.iter(), true);
    graph_from_vendor(&root, &root.join("userland/otp"), VENDORED.iter(), false);
}

/// In `workspace`'s graph for the target, each of `vendored` has no registry copy, and, when
/// `required`, is built from `vendor/`.
fn graph_from_vendor<'a>(
    root: &Path,
    workspace: &Path,
    vendored: impl Iterator<Item = &'a (&'a str, &'a str, &'a str)>,
    required: bool,
) {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let out = std::process::Command::new(cargo)
        .current_dir(workspace)
        .args([
            "metadata",
            "--format-version",
            "1",
            "--offline",
            "--filter-platform",
            "riscv64imac-unknown-none-elf",
        ])
        .output()
        .expect("run cargo metadata");
    assert!(out.status.success(), "cargo metadata failed: {}", String::from_utf8_lossy(&out.stderr));
    let paths = manifest_paths(&String::from_utf8(out.stdout).expect("UTF-8 metadata"));
    for (name, version, _) in vendored {
        let vendored = root.join("vendor").join(name).join("Cargo.toml");
        assert!(
            !required || paths.contains(vendored.to_str().unwrap()),
            "{name}: not built from {}",
            vendored.display()
        );
        let registry = format!("/{name}-{version}/Cargo.toml");
        let copies: Vec<_> = paths.iter().filter(|p| p.ends_with(&registry)).collect();
        assert!(copies.is_empty(), "{name}: a registry copy is in the graph: {copies:?}");
    }
}

/// The versions and checksums above are the ones vendor/README.md records, so the two cannot
/// drift apart.
#[test]
fn the_readme_records_each_crate() {
    let readme = std::fs::read_to_string(root().join("vendor/README.md")).expect("vendor/README.md");
    for (name, version, checksum) in all().chain(LOCKED).chain(BEAMLET_LOCKED) {
        let row = format!("| `{name}` | {version} |");
        let line = readme.lines().find(|l| l.starts_with(&row)).unwrap_or_else(|| panic!("no row {row:?}"));
        assert!(line.contains(checksum), "vendor/README.md's row for {name} lacks {checksum}");
    }
}
