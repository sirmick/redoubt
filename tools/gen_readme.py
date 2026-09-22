#!/usr/bin/env python3
"""Generate docs/README.html — one self-contained page with Graphviz-rendered SVG diagrams.

Usage:  python3 tools/gen_readme.py
Needs:  graphviz (`dot` on PATH).  Installed in the dev image (Dockerfile).

The DOT sources, the page template and the diagram ids all live here, so the
HTML and its diagrams are reproducible from this one file. No network, no JS.
"""
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "docs" / "README.html"

# --- status palette -------------------------------------------------------
BUILT = ("#d9f2e0", "#1a7f37")   # green
PROG = ("#fff3cd", "#9a6700")    # amber
DES = ("#ffe0e0", "#b42318")     # red
DEFER = ("#eef0f2", "#6b7280")   # grey
KERN = ("#dbeafe", "#1d4ed8")    # blue
ACC = ("#ede9fe", "#6d28d9")     # violet


def node(name, label, palette, **kw):
    fill, line = palette
    attrs = {"label": label, "fillcolor": fill, "color": line}
    attrs.update(kw)
    body = ", ".join(f'{k}="{v}"' for k, v in attrs.items())
    return f"  {name} [{body}];"


BASE = (
    "rankdir={rank}; bgcolor=\"transparent\"; fontname=\"Helvetica\"; "
    "splines={splines}; nodesep=0.35; ranksep=0.5; "
    "node [shape=box, style=\"rounded,filled\", fontname=\"Helvetica\", fontsize=11, "
    "penwidth=1.5, margin=\"0.16,0.09\"]; "
    "edge [color=\"#94a3b8\", penwidth=1.3, arrowsize=0.7, fontname=\"Helvetica\", fontsize=9]; "
)


def digraph(name, body, rank="TB", splines="spline"):
    head = f"digraph {name} {{\n" + BASE.format(rank=rank, splines=splines)
    return head + "\n".join(body) + "\n}\n"


DIAGRAMS = {}

DIAGRAMS["stack"] = digraph("stack", [
    node("hw", "HARDWARE\\nQEMU virt RV64 + RV32 (built)   ·   FPGA cards (planned)", DEFER),
    node("fw", "BIOS / firmware (M-mode)\\nRustSBI Prototyper — rv64 + rv32", BUILT),
    node("ld", "LOADER (S-mode)\\nverify the Ed25519 bundle  ·  build Sv32/Sv39  ·  argument block", BUILT),
    node("ke", "KERNEL (S-mode) — the TCB\\nmemory · threads · IPC · budgets · handles · endpoints · devices · timer", KERN),
    node("sv", "SERVERS (U-mode, unprivileged)\\nkeyd  ·  steward  ·  sshd  ·  fsd  ·  blkd  ·  netd  ·  ipd  ·  bootfsd  ·  consoled", DES),
    node("ul", "USERLAND\\nbeamlet — the BEAM/OTP runtime  ·  sessions & agents (one VM = one trust domain)", BUILT),
    "  hw -> fw -> ld -> ke;",
    "  ke -> sv;",
    "  ke -> ul;",
    "  { rank=same; sv; ul; }",
    "  sv -> ul [label=\"IPC: call / send · 9P · handles\", dir=both, color=\"#6d28d9\", fontcolor=\"#6d28d9\"];",
])

DIAGRAMS["boot"] = digraph("boot", [
    node("rom", "ROM / firmware\\nM-mode", BUILT),
    node("loader", "LOADER (S-mode, MMU off)\\n1  read the device tree\\n2  VERIFY the bundle — fail ⇒ power off\\n3  build Sv32/Sv39 address spaces\\n4  write the argument block", BUILT),
    node("kernel", "KERNEL (S-mode)\\nroot / system / users budgets\\nhand every device to init", KERN),
    node("init", "init (U-mode)\\nstart servers from the\\nsigned boot manifest", DES),
    node("run", "servers + sessions", DES),
    "  rom -> loader -> kernel -> init -> run;",
], rank="LR")

DIAGRAMS["kernel"] = digraph("kernel", [
    node("proc", "process\\nhandle table (≤ MAX_HANDLES = 4096)\\nopen calls (≤ 64) · current call · exit endpoint", KERN),
    node("budget", "BUDGET\\npages · processes · weight\\nclass · labels · account · deadline", BUILT),
    node("endpoint", "ENDPOINT\\noutlives its server\\nfair waiting (R2)", BUILT),
    node("mmio", "MMIO (+ DMA flag)", BUILT),
    node("irq", "IRQ", BUILT),
    node("reset", "Reset", BUILT),
    "  proc -> budget [label=\"handle\"];",
    "  proc -> endpoint [label=\"handle   (badge 0 = receive right)\"];",
    "  proc -> mmio;",
    "  proc -> irq;",
    "  proc -> reset;",
    "  endpoint -> proc [label=\"message (badge, account, labels, id)\", color=\"#6d28d9\", fontcolor=\"#6d28d9\"];",
], rank="LR")

DIAGRAMS["ipc"] = digraph("ipc", [
    node("client", "client", BUILT),
    node("ep", "ENDPOINT", ACC, shape="ellipse"),
    node("server", "server thread", BUILT),
    "  client -> ep [label=\"call(handle, lend buffer)\"];",
    "  ep -> server [label=\"receive()  →  a call or a send\"];",
    "  server -> ep [label=\"reply(msg_id)\"];",
    "  ep -> client [label=\"reply + lend returned\"];",
    "  server -> server [label=\"serve(msg_id)\\nsets the current call (blame)\", color=\"#9a6700\", fontcolor=\"#9a6700\", style=dashed];",
    "  ep -> client [label=\"abandoned-call notice if the caller dies\", color=\"#b42318\", fontcolor=\"#b42318\", style=dashed];",
], rank="LR")

DIAGRAMS["security"] = digraph("security", [
    node("cap", "CAPABILITY\\nhandle = (object, badge, stamp)\\nunforgeable · revocable by stamp", ACC),
    node("budget", "BUDGET\\ncontainer charged in pages\\nCPU weight · account · deadline", BUILT),
    node("label", "LABEL\\n64-bit name, fixed at creation\\ninformation flow", PROG),
    node("rule", "a flow A → B is allowed iff\\nB is system-class  OR  labels(B) ⊇ labels(A)", KERN, shape="diamond"),
    node("read", "reads above your labels FAIL\\nwrites need EQUAL labels", DES),
    "  cap -> rule;",
    "  budget -> rule;",
    "  label -> rule;",
    "  rule -> read;",
], rank="LR")

DIAGRAMS["servers"] = digraph("servers", [
    node("client", "sessions & agents\\n(beamlet VMs)", BUILT),
    node("steward", "steward\\nprincipals · sessions\\npowerbox · audit", DES),
    node("sshd", "sshd\\nSSH front door", DES),
    node("keyd", "keyd\\nkeys that sign\\nnever export", BUILT),
    node("fsd", "fsd\\nlittlefs, one per volume", DES),
    node("ipd", "ipd\\nsmoltcp, serves /net", DES),
    node("bootfsd", "bootfsd\\nread-only /boot", PROG),
    node("consoled", "consoled\\n/dev/cons", PROG),
    node("blkd", "blkd\\nvirtio-blk", PROG),
    node("netd", "netd\\nvirtio-net", PROG),
    "  client -> steward;",
    "  client -> sshd;",
    "  sshd -> steward;",
    "  sshd -> keyd;",
    "  client -> fsd;",
    "  client -> ipd;",
    "  client -> bootfsd;",
    "  client -> consoled;",
    "  fsd -> blkd;",
    "  ipd -> netd;",
    "  { rank=same; fsd; ipd; bootfsd; consoled; }",
    "  { rank=same; blkd; netd; }",
], rank="LR")

DIAGRAMS["beamlet"] = digraph("beamlet", [
    node("elixir", "Elixir / OTP stdlib / IEx   (unchanged OTP code)", BUILT),
    node("vm", "beamlet VM\\nper-process heaps · copying GC · several schedulers", BUILT),
    node("nat", "Rust natives\\ncrypto (RustCrypto) · re · zlib · prim_file · console", BUILT),
    node("plat", "Platform trait: ONE asynchronous 9P client", ACC),
    node("os", "Redoubt: files · /net · /dev/cons are namespace walks (resource terms)", PROG),
    "  elixir -> vm -> nat -> plat -> os;",
    "  nat -> nat [label=\"differential tests vs the real BEAM\", color=\"#1a7f37\", fontcolor=\"#1a7f37\", style=dashed];",
])

DIAGRAMS["testing"] = digraph("testing", [
    node("case", "tests/&lt;case&gt;.toml\\narch · programs · expect · forbid", BUILT),
    node("bench", "testbench\\nbuild kernel + loader + programs", BUILT),
    node("sign", "sign the bundle\\nredoubt.bundle.v1 domain", BUILT),
    node("qemu", "QEMU virt\\nrv64 · rv32", BUILT),
    node("assert", "assert on the console\\n+ SSH sessions + virtio disk/net", BUILT),
    node("verdict", "attack verdict comes from the SYSTEM\\nkernel · victim · clean power-off", PROG),
    "  case -> bench -> sign -> qemu -> assert;",
    "  assert -> case [label=\"PASS / FAIL\", color=\"#1a7f37\", fontcolor=\"#1a7f37\", constraint=false];",
    "  assert -> verdict [style=dashed];",
], rank="LR")

DIAGRAMS["milestones"] = digraph("milestones", [
    node("m1", "M1  (in progress)\\nseparation & containment\\nAlice, Bob, a contained agent", PROG),
    node("m2", "M2  (designed)\\ninstall · share · persist\\npackages · projects · A/B updates", DES),
    node("m3", "M3  (designed)\\nself-hosted development\\nreal agent harness · compilers", DES),
    node("after", "after M3\\nrv32 returned · SMP · FPGA", DEFER),
    "  m1 -> m2 -> m3 -> after;",
], rank="LR")

DIAGRAMS["repo"] = digraph("repo", [
    node("root", "redoubt/", KERN, shape="folder"),
    node("bios", "bios/\\nM-mode firmware (RustSBI)", BUILT, shape="folder"),
    node("loader", "loader/\\nS-mode boot loader", BUILT, shape="folder"),
    node("kernel", "kernel/\\nthe microkernel (TCB)", BUILT, shape="folder"),
    node("servers", "servers/\\nunprivileged servers", DES, shape="folder"),
    node("userland", "userland/\\notp/ = beamlet (BEAM)", BUILT, shape="folder"),
    node("libs", "libs/\\nsys rt wire signing littlefs paging", BUILT, shape="folder"),
    node("image", "image/\\nbundle & disk recipes", BUILT, shape="folder"),
    node("tools", "tools/\\ntestbench · generators", BUILT, shape="folder"),
    node("tests", "tests/\\ncases (*.toml) + programs", BUILT, shape="folder"),
    node("docs", "docs/\\nthe design of record", BUILT, shape="folder"),
    node("ext", "reference/ · toolchains/ · vendor/", DEFER, shape="folder"),
    "  root -> bios; root -> loader; root -> kernel; root -> servers; root -> userland;",
    "  root -> libs; root -> image; root -> tools; root -> tests; root -> docs; root -> ext;",
], rank="LR")


def render(name, src):
    out = subprocess.run(["dot", "-Tsvg"], input=src, capture_output=True, text=True)
    if out.returncode != 0:
        sys.stderr.write(f"dot failed for {name}:\n{out.stderr}\n")
        sys.exit(1)
    svg = out.stdout
    svg = re.sub(r"<\?xml[^>]*\?>\s*", "", svg)
    svg = re.sub(r"<!DOCTYPE[^>]*>\s*", "", svg)
    svg = re.sub(r"<!--.*?-->\s*", "", svg, flags=re.S)
    svg = re.sub(r'id="([^"]+)"', lambda m: f'id="{name}_{m.group(1)}"', svg)
    svg = svg.replace("url(#", f"url(#{name}_")
    return svg.strip()


SVG = {k: render(k, v) for k, v in DIAGRAMS.items()}


def put(html, token, key):
    return html.replace(f"@@{token}@@", SVG[key])


TEMPLATE = r"""<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Redoubt — system documentation</title>
<style>
:root{
  --bg:#f7f8fa; --card:#ffffff; --ink:#111827; --muted:#4b5563; --line:#e5e7eb;
  --built:#1a7f37; --built-bg:#e8f6ec; --prog:#9a6700; --prog-bg:#fff6dc;
  --des:#b42318; --des-bg:#fdeceb; --defer:#6b7280; --defer-bg:#eef0f2;
  --kern:#1d4ed8; --kern-bg:#e8effd; --acc:#6d28d9; --acc-bg:#f0eafd;
}
*{box-sizing:border-box}
html{scroll-behavior:smooth}
body{margin:0;background:var(--bg);color:var(--ink);
  font:16px/1.62 -apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,Helvetica,Arial,sans-serif;}
a{color:#1d4ed8;text-decoration:none}
a:hover{text-decoration:underline}
code,kbd,pre{font-family:"SFMono-Regular",Consolas,"Liberation Mono",Menlo,monospace}
code{background:#f0f2f5;padding:.08em .35em;border-radius:4px;font-size:.9em}
pre{background:#0f172a;color:#e2e8f0;padding:14px 16px;border-radius:10px;overflow:auto;font-size:13.5px;line-height:1.5}
pre code{background:none;padding:0;color:inherit}
.wrap{max-width:1120px;margin:0 auto;padding:0 22px}
header.hero{background:linear-gradient(160deg,#0f172a,#1e3a8a 65%,#3730a3);color:#fff;padding:46px 0 34px}
header.hero h1{margin:0 0 6px;font-size:40px;letter-spacing:-.02em}
header.hero .tag{font-size:18px;color:#c7d2fe;max-width:820px}
.legend{display:flex;flex-wrap:wrap;gap:8px;margin-top:20px}
.pill{display:inline-flex;align-items:center;gap:7px;padding:3px 11px;border-radius:999px;
  font-size:12.5px;font-weight:600;border:1px solid transparent;white-space:nowrap}
.dot{width:9px;height:9px;border-radius:50%}
.built{background:var(--built-bg);color:var(--built);border-color:#b7e0c2}.built .dot{background:var(--built)}
.prog{background:var(--prog-bg);color:var(--prog);border-color:#efd9a0}.prog .dot{background:var(--prog)}
.des{background:var(--des-bg);color:var(--des);border-color:#f3c0bc}.des .dot{background:var(--des)}
.defer{background:var(--defer-bg);color:var(--defer);border-color:#d5d8dd}.defer .dot{background:var(--defer)}
.kern{background:var(--kern-bg);color:var(--kern);border-color:#bcd0f5}.kern .dot{background:var(--kern)}
.acc{background:var(--acc-bg);color:var(--acc);border-color:#d6c8f7}.acc .dot{background:var(--acc)}
nav.toc{position:sticky;top:0;z-index:20;background:rgba(247,248,250,.92);
  backdrop-filter:blur(8px);border-bottom:1px solid var(--line)}
nav.toc ul{display:flex;flex-wrap:wrap;gap:2px 14px;list-style:none;margin:0;padding:10px 0;font-size:13.5px}
nav.toc a{color:var(--muted);font-weight:600}
main{padding:8px 0 60px}
section{background:var(--card);border:1px solid var(--line);border-radius:14px;
  padding:26px 28px;margin:22px 0;box-shadow:0 1px 2px rgba(16,24,40,.04)}
h2{margin:0 0 4px;font-size:26px;letter-spacing:-.01em}
h2 .num{color:#9ca3af;font-weight:700;margin-right:10px}
h3{margin:24px 0 4px;font-size:18px}
p{margin:10px 0;color:var(--ink)}
.lead{color:var(--muted)}
.diagram{background:#fbfcfe;border:1px solid var(--line);border-radius:12px;padding:14px;margin:16px 0;overflow:auto}
.diagram svg{max-width:100%;height:auto;display:block;margin:0 auto}
.grid2{display:grid;grid-template-columns:1fr 1fr;gap:16px}
@media(max-width:760px){.grid2{grid-template-columns:1fr}}
.card{border:1px solid var(--line);border-radius:10px;padding:14px 16px;background:#fcfcfd}
.card h4{margin:0 0 8px;font-size:14px;text-transform:uppercase;letter-spacing:.06em;color:var(--muted)}
ul.tight{margin:8px 0;padding-left:20px}ul.tight li{margin:4px 0}
table{border-collapse:collapse;width:100%;margin:14px 0;font-size:14.5px}
th,td{text-align:left;padding:9px 12px;border-bottom:1px solid var(--line);vertical-align:top}
th{background:#f3f4f6;font-size:12.5px;text-transform:uppercase;letter-spacing:.04em;color:var(--muted)}
tr:hover td{background:#fafbfc}
.note{border-left:4px solid var(--acc);background:var(--acc-bg);padding:10px 14px;border-radius:0 8px 8px 0;margin:14px 0}
.warn{border-left:4px solid var(--prog);background:var(--prog-bg);padding:10px 14px;border-radius:0 8px 8px 0;margin:14px 0}
footer{color:var(--muted);font-size:13.5px;padding:26px 0 50px}
.kv{display:grid;grid-template-columns:max-content 1fr;gap:4px 16px;font-size:14.5px}
.kv b{color:var(--muted)}
</style>
</head>
<body>

<header class="hero"><div class="wrap">
  <h1>Redoubt</h1>
  <p class="tag">A small, auditable <b>RISC-V microkernel</b> in pure Rust — and the runtimes that run on it.
  Today the runtime is <b>beamlet</b>, a safe-Rust BEAM (Erlang/Elixir) VM. The OS is runtime-neutral.</p>
  <div class="legend">
    <span class="pill built"><span class="dot"></span>Built — exercised by cargo testbench</span>
    <span class="pill prog"><span class="dot"></span>In progress — a work package is building</span>
    <span class="pill des"><span class="dot"></span>Designed, not built — in docs/</span>
    <span class="pill defer"><span class="dot"></span>Deferred — deliberately later</span>
  </div>
</div></header>

<nav class="toc"><div class="wrap"><ul>
  <li><a href="#overview">Overview</a></li>
  <li><a href="#boot">Boot</a></li>
  <li><a href="#kernel">Kernel</a></li>
  <li><a href="#servers">Servers</a></li>
  <li><a href="#beamlet">Userland</a></li>
  <li><a href="#security">Security model</a></li>
  <li><a href="#start">Getting started</a></li>
  <li><a href="#testing">Testing</a></li>
  <li><a href="#reading">Read the design</a></li>
  <li><a href="#milestones">Milestones</a></li>
</ul></div></nav>

<main class="wrap">

<section id="overview">
  <h2><span class="num">01</span>The system at a glance</h2>
  <p class="lead">Redoubt is a hard fork of <a href="https://github.com/betrusted-io/xous-core">Xous</a>, rebuilt for RV32 and RV64
  on one width-generic code path and aimed at a specific threat: a capable adversary that has read every line of the
  source and controls any code it is allowed to run. The design rests on <b>no ambient authority</b> — a process can
  touch only what it was explicitly given — enforced by a tiny kernel with capabilities, budgets and information-flow labels.</p>
  <div class="diagram">@@STACK@@</div>
  <div class="grid2">
    <div class="card"><h4>Built and tested</h4><ul class="tight">
      <li>rv64 + rv32 boot on QEMU virt under the vendored RustSBI firmware</li>
      <li>the kernel: memory, legacy threads, handles &amp; endpoints, budgets, devices, IRQ receive, verified boot, W^X; cooperative scheduling</li>
      <li><code>libs/</code>: sys, rt, wire, signing, littlefs, paging</li>
      <li>host-tested server components: keyd, bootfsd, consoled, blkd; full init boot wiring pending</li>
      <li><code>tools/testbench</code>; beamlet on the host — Elixir, its compiler, IEx, OTP crypto/ssl/ssh</li>
    </ul></div>
    <div class="card"><h4>Designed, not built</h4><ul class="tight">
      <li>init + boot manifest + loader stub</li>
      <li>steward, sshd, fsd, netd, ipd; end-to-end storage/network services</li>
      <li>new process/thread syscalls, timer-driven scheduling, beamlet's Redoubt platform</li>
      <li>packages, projects, sharing, A/B system updates</li>
      <li>FPGA cards, disk encryption, gatewayd, webd, linkd/routerd</li>
    </ul></div>
  </div>
  <p>A term to hold onto: a <b>budget</b> is a kernel container every process lives in. It is the unit of accounting,
  CPU share, revocation, <b>information-flow labels</b>, and identity (its <i>account</i> travels with every message) —
  one object replacing five mechanisms. <a href="RESOURCES.md">RESOURCES.md</a> · <a href="CONTAINMENT.md">CONTAINMENT.md</a></p>
</section>

<section id="boot">
  <h2><span class="num">02</span>Boot flow</h2>
  <p>Firmware verifies nothing on QEMU (QEMU loads the loader directly); the link that matters is that the
  <b>loader authenticates the boot bundle</b> before executing any of it. The initrd is <code>signature(64) ‖ tar</code>;
  the signature covers <code>"redoubt.bundle.v1\0" ‖ u64_le(len) ‖ tar</code>, and one crate builds the preimage for
  both the loader and the signer so they cannot drift.</p>
  <div class="diagram">@@BOOT@@</div>
  <ul class="tight">
    <li><b>Built:</b> vendored RustSBI and loader on both widths, Ed25519 verification, W^X, default-deny device grants.
      <a href="BOOT.md">BOOT.md</a> · <a href="VERIFIED-BOOT.md">VERIFIED-BOOT.md</a> · <a href="../libs/signing/src/lib.rs">libs/signing</a> · <a href="../loader/src/verify.rs">loader/src/verify.rs</a></li>
    <li><b>Designed change:</b> the loader will load only <code>kernel</code> + <code>init</code>; <code>init</code> launches every other
      process through a system-signed <b>loader stub</b> from the bundle's pages. <a href="PACKAGES.md">PACKAGES.md</a></li>
  </ul>
  <h3>Hardware</h3>
  <table>
    <tr><th>Target</th><th>Width</th><th>Firmware</th><th>Status</th></tr>
    <tr><td>QEMU <code>virt</code></td><td>RV64 (Sv39)</td><td>RustSBI Prototyper</td><td><span class="pill built"><span class="dot"></span>booted</span></td></tr>
    <tr><td>QEMU <code>virt</code></td><td>RV32 (Sv32)</td><td>RustSBI Prototyper</td><td><span class="pill built"><span class="dot"></span>booted</span></td></tr>
    <tr><td>FPGA PCIe cards (XC7K480T)</td><td>RV64GC Sv39, 32 hw threads</td><td>RustSBI</td><td><span class="pill des"><span class="dot"></span>planned</span></td></tr>
    <tr><td>Messy SoCs (e.g. Orange Pi RV2)</td><td>RV64</td><td>RustSBI domains; Linux on reserved cores</td><td><span class="pill defer"><span class="dot"></span>deferred</span></td></tr>
  </table>
  <p class="lead">Hardware differences are <b>capability features</b> (<code>sbi</code>, <code>plic</code>) composed by <b>board
  features</b> (<code>qemu-virt</code>), never <code>target_arch</code> checks. RAM, MMIO and the interrupt controller come from
  the device tree. <a href="PLATFORM-FPGA.md">PLATFORM-FPGA.md</a> · <a href="IO-ARCHITECTURE.md">IO-ARCHITECTURE.md</a></p>
</section>

<section id="kernel">
  <h2><span class="num">03</span>The kernel — objects and calls</h2>
  <p>The kernel keeps memory, threads, IPC, interrupt delivery and the timer. Nothing else. Its whole interface is a
  small set of objects reached through unforgeable handles, and a set of system calls whose errors and
  <b>order of checks</b> are normative so the executable model and the real kernel can be compared exactly.</p>
  <div class="diagram">@@KERNEL@@</div>
  <div class="diagram">@@IPC@@</div>
  <table>
    <tr><th>System call</th><th>What it does</th><th>Status</th></tr>
    <tr><td><code>map_anon unmap set_flags map_device dma_alloc</code></td><td>memory, W^X, DMA</td><td><span class="pill built"><span class="dot"></span>built</span></td></tr>
    <tr><td><code>thread_create thread_exit</code></td><td>new thread API (legacy path exists)</td><td><span class="pill des"><span class="dot"></span>not implemented</span></td></tr>
    <tr><td><code>endpoint_create mint</code></td><td>capabilities</td><td><span class="pill built"><span class="dot"></span>built</span></td></tr>
    <tr><td><code>call send receive reply serve</code></td><td>zero-copy IPC, lend / transfer</td><td><span class="pill built"><span class="dot"></span>implemented</span>; <a href="STATUS.md">IPC1 acceptance pending</a></td></tr>
    <tr><td><code>budget_create budget_destroy budget_usage</code></td><td>accounting, revocation, labels</td><td><span class="pill built"><span class="dot"></span>built</span></td></tr>
    <tr><td><code>time_now random system_reset</code></td><td>services</td><td><span class="pill built"><span class="dot"></span>built</span></td></tr>
    <tr><td><code>process_create process_map process_start process_exit</code></td><td>launch + exit notices</td><td><span class="pill prog"><span class="dot"></span>in progress</span></td></tr>
  </table>
  <p>The precise spec — constants, the cost table, rules R1–R12, invariants I1–I15, errors and the order of checks —
  is <a href="KERNEL-SPEC.md">KERNEL-SPEC.md</a>, frozen for milestone 1. Implementation:
  <a href="../kernel/src/main.rs">kernel/src/main.rs</a>, <a href="../kernel/src/syscall.rs">kernel/src/syscall.rs</a>,
  <a href="../kernel/src/services.rs">kernel/src/services.rs</a>, <a href="../kernel/src/mem.rs">kernel/src/mem.rs</a>.
  Page tables: <a href="../libs/paging/src/lib.rs">libs/paging/src/lib.rs</a>.</p>
</section>

<section id="servers">
  <h2><span class="num">04</span>Servers</h2>
  <p>Every server is an unprivileged process in its own budget. Its authority is a set of handles, never ambient.
  A launcher never passes its own connection to a child: it asks the server for a fresh one, so a hostile agent
  started by a shell cannot share the shell's file table.</p>
  <div class="diagram">@@SERVERS@@</div>
  <table>
    <tr><th>Server</th><th>Role</th><th>Status</th></tr>
    <tr><td><code>init</code></td><td>holds all authority at boot; starts, wires and restarts every OS process</td><td><span class="pill des"><span class="dot"></span>designed</span></td></tr>
    <tr><td><code>consoled</code></td><td>ns16550 UART driver, serves <code>/dev/cons</code></td><td>implemented, host-tested; full init wiring pending</td></tr>
    <tr><td><code>bootfsd</code></td><td>read-only 9P over the verified bundle (<code>/boot</code>)</td><td>implemented, host-tested; full init wiring pending</td></tr>
    <tr><td><code>blkd</code></td><td>virtio-blk driver + partitions + block-range handles</td><td>host-tested library; boot integration pending</td></tr>
    <tr><td><code>fsd</code></td><td>littlefs, one instance per volume, serves 9P</td><td><span class="pill des"><span class="dot"></span>designed</span></td></tr>
    <tr><td><code>netd</code> · <code>ipd</code></td><td>virtio-net driver; smoltcp stack serving <code>/net</code></td><td><span class="pill prog"><span class="dot"></span>in progress</span></td></tr>
    <tr><td><code>keyd</code></td><td>holds every private key; signs, never exports</td><td><span class="pill built"><span class="dot"></span>built</span> <a href="../servers/keyd/src/lib.rs">code</a></td></tr>
    <tr><td><code>steward</code> · <code>sshd</code></td><td>principals, sessions, powerbox, audit; SSH front door</td><td><span class="pill des"><span class="dot"></span>designed</span></td></tr>
  </table>
  <div class="note"><b>keyd's rule:</b> a badge names <b>one key and one purpose</b>. Every signature is over a 32-byte digest <b>keyd computed itself</b>, and no operation returns a private key. A session that crashes a shared server three times (same account + label set) is logged out for 10 minutes, taken from the kernel's exit notice — never the attacker's output. <a href="INIT.md">INIT.md</a> · <a href="CONTAINMENT.md">CONTAINMENT.md</a></div>
</section>

<section id="beamlet">
  <h2><span class="num">05</span>Userland: beamlet (the OTP runtime)</h2>
  <p>The OS is runtime-neutral. beamlet runs Erlang/Elixir bytecode from the pinned OTP 28, with its own scheduler,
  per-process heaps and copying GC. It is verified by differential tests: every test runs on the real BEAM and on
  beamlet and the output must be identical. OTP's <code>crypto</code>, <code>public_key</code>, <code>ssl</code> and
  <code>ssh</code> run unmodified on Rust natives.</p>
  <div class="diagram">@@BEAMLET@@</div>
  <ul class="tight">
    <li><b>Built and tested against the real BEAM:</b> <a href="../userland/otp/DESIGN.md">userland/otp/DESIGN.md</a></li>
    <li><b>Designed:</b> beamlet's <code>Platform</code> retargeted to the Redoubt 9P client (WP-B1/WP-B2: console, files, launching; then IEx on the UART). <a href="USERLAND.md">USERLAND.md</a></li>
    <li>On Redoubt, one VM = one trust domain; a new trust domain is a new VM under a new budget.</li>
  </ul>
</section>

<section id="security">
  <h2><span class="num">06</span>The security model in one diagram</h2>
  <p>Capabilities bound <b>authority</b> (what a process can do); labels bound <b>information</b> (what it can leak);
  budgets tie both together and are the only revocation mechanism.</p>
  <div class="diagram">@@SECURITY@@</div>
  <div class="grid2">
    <div class="card"><h4>Mechanisms</h4><ul class="tight">
      <li><b>No ambient authority</b> — devices default-deny; RAM is never nameable by address; pages are zeroed</li>
      <li><b>Verified boot</b> — every privileged byte is authenticated first</li>
      <li><b>W^X</b> everywhere, including the kernel's own mappings</li>
      <li><b>Fail closed</b> — a violated invariant stops the machine</li>
      <li><b>unsafe is a budget</b> that only falls</li>
    </ul></div>
    <div class="card"><h4>Agents and approvals</h4><ul class="tight">
      <li>Own principal, accountable sponsor, task-scoped <b>leases</b></li>
      <li>Delegation only narrows; assume every agent is compromised</li>
      <li>Approvals are <b>out of band</b>: only the steward talks to <code>ssh approve@box</code></li>
      <li>Three blamed crashes end a session for the window</li>
    </ul></div>
  </div>
  <p><a href="CAPABILITIES.md">CAPABILITIES.md</a> · <a href="CONTAINMENT.md">CONTAINMENT.md</a> ·
  <a href="TENETS.md">TENETS.md</a> (outranks everything) · <a href="DEVICE-GRANTS.md">DEVICE-GRANTS.md</a></p>
</section>

<section id="start">
  <h2><span class="num">07</span>Getting started</h2>
  <p>The OS build/test environment runs in Docker. Beamlet's differential tests additionally require separately installed OTP/Elixir. A concise version of this
  section also lives in <a href="GETTING-STARTED.md">GETTING-STARTED.md</a>.</p>
  <h3>1 · Toolchain</h3>
  <pre><code>./dev.sh                 # build the image (first time), then a shell in /work
./dev.sh --rebuild       # rebuild the image after editing the Dockerfile</code></pre>
  <p class="lead">Installs Rust with the RISC-V bare-metal targets (<code>riscv64imac</code>, <code>riscv32imac</code>,
  <code>riscv64gc</code>), QEMU for both widths, OpenSSH, graphviz (for this page's diagrams) and the agent CLIs.
  OTP 28.5.0.6 / Elixir 1.20.4 are not installed by the image; see the quickstart's prerequisites.</p>
  <h3>2 · Firmware (once)</h3>
  <pre><code>./scripts/build-bios.sh  # builds the vendored RustSBI in bios/ for both widths</code></pre>
  <h3>3 · Build the OS</h3>
  <pre><code>./build --arch rv64              # kernel + loader
./build --arch rv32
./build --arch rv64 --programs   # also the in-guest test programs</code></pre>
  <h3>4 · Launch it in a VM</h3>
  <pre><code>./launch --arch rv64                       # prints the exact QEMU line, then boots; Ctrl-A X quits
./launch --arch rv64 --program log-server
./launch --arch rv32 --smp 4
./launch --arch rv64 --print-only          # show the QEMU command and exit
./launch --arch rv64 --debug               # pause with a gdb stub on :1234</code></pre>
  <p class="lead"><code>launch</code> assembles and signs the boot bundle and attaches the guest serial console to your
  stdin/stdout. <a href="DEBUGGING.md">DEBUGGING.md</a></p>
  <h3>5 · Run the tests</h3>
  <pre><code>./test --arch rv64            # the whole bench
./test --arch rv64 timer      # cases whose name contains "timer"
./test --list</code></pre>
  <h3>6 · Build an image</h3>
  <pre><code>./mkimage                      # signed boot bundle → target/image/redoubt.bundle</code></pre>
  <p class="lead">Disk-image recipes (littlefs) live in <a href="../image/">image/</a> and are not built yet.</p>
  <h3>7 · beamlet</h3>
  <pre><code>cd userland/otp
. tools/env.sh                 # put the pinned OTP/Elixir on PATH
cargo test                     # unit + hostile-input tests
tools/difftest                 # differential tests against the real BEAM</code></pre>
  <h3>Regenerating this page</h3>
  <pre><code>python3 tools/gen_readme.py    # DOT → SVG → docs/README.html (needs graphviz)</code></pre>
</section>

<section id="testing">
  <h2><span class="num">08</span>Testing, and why a green run means something</h2>
  <p>One simple harness, real boots, no kernel mocks: <code>cargo testbench</code> boots the real kernel under QEMU
  with injected programs and asserts on the console. An <b>attack case passes only on a line the attacker cannot write</b> —
  the verdict comes from the kernel, a victim, or a clean power-off.</p>
  <div class="diagram">@@TESTING@@</div>
  <p class="lead">A build of the same sources with debug assertions and overflow checks (the <code>checked</code> profile)
  is a normal case option. 55 cases exercise IPC, budgets, devices, timers, loader rejections, verified-boot attacks,
  forged verdicts and SSH. <a href="TENETS.md">TENETS.md</a> tenet 6 · <a href="testbench.md">testbench.md</a> · <a href="../tests/">tests/</a></p>
</section>

<section id="reading">
  <h2><span class="num">09</span>Where to start reading</h2>
  <div class="diagram">@@REPO@@</div>
  <div class="grid2">
    <div class="card"><h4>Reading order</h4><ul class="tight">
      <li><a href="TENETS.md">TENETS.md</a> — outranks everything</li>
      <li><a href="README.md">README.md</a> — index + glossary + server roster</li>
      <li><a href="STATUS.md">STATUS.md</a> — where the code stands</li>
      <li><a href="PLAN.md">PLAN.md</a> — the three milestones</li>
      <li><a href="KERNEL-SPEC.md">KERNEL-SPEC.md</a> — the precise kernel</li>
      <li><a href="CAPABILITIES.md">CAPABILITIES.md</a> · <a href="CONTAINMENT.md">CONTAINMENT.md</a> · <a href="RESOURCES.md">RESOURCES.md</a></li>
      <li><a href="INIT.md">INIT.md</a> · <a href="NAMESPACES.md">NAMESPACES.md</a> · <a href="WIRE.md">WIRE.md</a></li>
    </ul></div>
    <div class="card"><h4>Code map</h4><ul class="tight">
      <li><a href="../bios/">bios/</a> · <a href="../loader/">loader/</a> · <a href="../kernel/">kernel/</a></li>
      <li><a href="../libs/sys/">libs/sys</a> (ABI) · <a href="../libs/rt/">libs/rt</a> · <a href="../libs/wire/">libs/wire</a></li>
      <li><a href="../libs/signing/">libs/signing</a> · <a href="../libs/littlefs/">libs/littlefs</a> · <a href="../libs/paging/">libs/paging</a></li>
      <li><a href="../servers/">servers/</a> · <a href="../userland/otp/">userland/otp</a></li>
      <li><a href="../tools/testbench/">tools/testbench</a> · <a href="../tests/">tests/</a></li>
    </ul></div>
  </div>
</section>

<section id="milestones">
  <h2><span class="num">10</span>Milestones</h2>
  <div class="diagram">@@MILESTONES@@</div>
  <ul class="tight">
    <li><b>M1 — separation and containment:</b> Alice and Bob logged in over SSH, separated; Alice's agent under a lease, contained. Every property backed by an attack test.</li>
    <li><b>M2 — install, share, persist:</b> packages and trust lists, projects, reboot memory, A/B system updates with rollback.</li>
    <li><b>M3 — self-hosted development:</b> a real agent harness through <code>gatewayd</code>, compilers on the box, the server APIs.</li>
    <li><b>After M3:</b> rv32 returns to mandatory milestone boot acceptance (the bench already supports both widths); SMP; the FPGA.</li>
  </ul>
  <p><a href="BUILD-PLAN.md">BUILD-PLAN.md</a> · <a href="SWARM.md">SWARM.md</a> · <a href="HISTORY.md">HISTORY.md</a></p>
</section>

<footer>
  Redoubt began as <b>Xous</b> by the betrusted.io project. Licensed under the terms in
  <a href="LICENSE">LICENSE</a> / <a href="LICENSES/">LICENSES/</a>. This page is generated by
  <a href="../tools/gen_readme.py">tools/gen_readme.py</a>.
</footer>

</main>
</body>
</html>
"""

html = TEMPLATE
for token, key in [
    ("STACK", "stack"), ("BOOT", "boot"), ("KERNEL", "kernel"), ("IPC", "ipc"),
    ("SECURITY", "security"), ("SERVERS", "servers"), ("BEAMLET", "beamlet"),
    ("TESTING", "testing"), ("MILESTONES", "milestones"), ("REPO", "repo"),
]:
    html = put(html, token, key)

# Pages publishes only docs/. Keep its document links local; repository source and
# root-level setup/license links must not escape that published directory.
def published_link(match):
    href = match.group(1)
    if href.startswith("../"):
        target = (OUT.parent / href).resolve()
    elif href in ("GETTING-STARTED.md", "LICENSE", "LICENSES/"):
        target = ROOT / href
    else:
        return match.group(0)
    relative = target.relative_to(ROOT)
    if not target.exists():
        raise ValueError(f"missing repository link: {href}")
    kind = "tree" if target.is_dir() else "blob"
    return f'href="https://github.com/sirmick/redoubt/{kind}/main/{relative}"'


html = re.sub(r'href="([^"]+)"', published_link, html)
OUT.write_text(html)
print(f"wrote {OUT} ({len(html)} bytes, {len(SVG)} diagrams)")
