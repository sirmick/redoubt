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
    node("ul", "USERLAND (planned integration)\\nbeamlet runs on the host today  ·  sessions & agents", DES),
    "  hw -> fw -> ld -> ke;",
    "  ke -> sv;",
    "  ke -> ul;",
    "  { rank=same; sv; ul; }",
    "  sv -> ul [label=\"IPC: call / send · 9P · handles\", dir=both, color=\"#6d28d9\", fontcolor=\"#6d28d9\"];",
])

DIAGRAMS["ipc"] = digraph("ipc", [
    node("client", "client", BUILT),
    node("ep", "ENDPOINT", ACC, shape="ellipse"),
    node("server", "server thread", BUILT),
    "  client -> ep [label=\"call(handle, lend buffer)\"];",
    "  ep -> server [label=\"receive()  →  a call or a send\"];",
    "  server -> ep [label=\"reply(msg_id)\"];",
    "  ep -> client [label=\"reply + lend returned\"];",
    "  server -> server [label=\"serve(msg_id)\\nsets the current call (blame)\", color=\"#9a6700\", fontcolor=\"#9a6700\", style=dashed];",
    "  ep -> server [label=\"abandoned-call notice if the caller dies\", color=\"#b42318\", fontcolor=\"#b42318\", style=dashed];",
], rank="LR")

DIAGRAMS["swarm"] = digraph("swarm", [
    node("contract", "Contract + open decisions", ACC),
    node("claim", "Claim ready package\\ncheck dependencies", KERN),
    node("work", "Isolated worktree\\none writer per hotspot", KERN),
    node("test", "Acceptance + full bench\\nattack cases · rv32 · unsafe ratchet", BUILT),
    node("review", "Defensive · simplifier · editor\\nfix findings / track review debt", ACC),
    node("integrate", "Rebase · retest · integrate\\nupdate claims and current status", BUILT),
    "  contract -> claim -> work -> test -> review -> integrate;",
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
<title>Redoubt — tenets, architecture and build method</title>
<style>
:root{color-scheme:light;--ink:#172033;--muted:#475569;--line:#dbe2ea}
*{box-sizing:border-box}body{margin:0;background:#f7f8fa;color:var(--ink);font:17px/1.6 system-ui,sans-serif}
main,header,footer{max-width:1100px;margin:auto;padding:24px}header{padding-top:48px}
h1{font-size:48px;margin:0}h2{margin:0 0 16px}a{color:#1d4ed8}p{max-width:85ch}
nav{display:flex;flex-wrap:wrap;gap:20px;margin-top:24px}section{padding:28px 0;border-top:1px solid var(--line)}
table{border-collapse:collapse;width:100%;margin:20px 0}th,td{padding:10px;text-align:left;border-bottom:1px solid var(--line);vertical-align:top}
.diagram{overflow-x:auto;background:white;padding:16px;border:1px solid var(--line);border-radius:8px;margin:20px 0}
.diagram svg{display:block;width:100%;min-width:620px;height:auto}.note{color:var(--muted)}
code{font-size:.9em}footer{font-size:14px;color:var(--muted)}
</style>
</head>
<body>
<header>
<h1>Redoubt</h1>
<p>A RISC-V microkernel in pure Rust for running hostile code under explicit authority,
resource limits and information-flow labels. QEMU boots on rv64 and rv32; beamlet currently
runs on the host. Native userland integration remains work in progress.</p>
<nav aria-label="Page"><a href="#tenets">Tenets</a><a href="#architecture">Architecture</a>
<a href="#method">Swarm method</a><a href="#start">Build and test</a><a href="#next">Next</a></nav>
</header>
<main>
<section id="tenets">
<h2>The tenets govern the design</h2>
<p><a href="TENETS.md">TENETS</a> outranks every other document. These are target constraints;
<a href="STATUS.md">STATUS</a> records implementation gaps and evidence.</p>
<ol>
<li><b>Simple enough to audit:</b> a small kernel, one mechanism per job, deletion before configuration.</li>
<li><b>Secure by construction:</b> explicit capabilities, least privilege, W^X, authenticated privileged code,
fail-closed behavior, bounded unsafe code and an out-of-band approval channel.</li>
<li><b>Rust:</b> assembly only where Rust cannot reach; host test-oracle exceptions stay off target.</li>
<li><b>Open standards:</b> published hardware interfaces, formats, protocols and reproducible sources.</li>
<li><b>Dependencies are part of the TCB:</b> small, read, pinned and justified.</li>
<li><b>Attack-tested:</b> real boots, regressions, hostile inputs and a harness proved able to fail.</li>
<li><b>Virtio devices:</b> trivial-device exceptions; DMA drivers are trusted unless hardware confines them.</li>
</ol>
<p class="note">The label set is the isolation unit. Covert/physical channels are outside the software claim.
Mediation, authority closure and the wakeup bound still have <a href="QUESTIONS.md">open decisions 164–166</a>;
their recommendations are not accepted rules.</p>
</section>
<section id="architecture">
<h2>System and IPC</h2>
<p>The kernel owns memory, threads, IPC, interrupt delivery and the timer; services and policy run
in user processes. This diagram shows the target structure; <a href="STATUS.md">STATUS</a> distinguishes
implemented components from integrated services.</p>
<div class="diagram" role="img" aria-label="Redoubt architecture">@@STACK@@</div>
<p><a href="BOOT.md">Boot flow</a> · <a href="VERIFIED-BOOT.md">Bundle authentication</a> ·
<a href="KERNEL-SPEC.md">Kernel contract</a> · <a href="INIT.md">Init and startup</a></p>
<div class="diagram" role="img" aria-label="Call and reply lifecycle">@@IPC@@</div>
<p>The caller consumes a lend and receives an explicit ownership outcome. A server learns whether
its reply was delivered and which handles were installed. <a href="KERNEL-SPEC.md#ipc-completion">Completion rules</a>
cover cancellation, partial replies and output failure; full IPC1 acceptance is still open.</p>
<p><a href="CAPABILITIES.md">Delegation and leases</a> · <a href="CONTAINMENT.md">Labels and mediation</a> ·
<a href="RESOURCES.md">Budgets and scheduling</a> · <a href="NAMESPACES.md">9P and namespaces</a> ·
<a href="WIRE.md">Wire formats</a></p>
</section>
<section id="method">
<h2>The swarm method</h2>
<p><a href="SWARM.md">SWARM</a> owns execution and claims. Independent packages use isolated worktrees;
one writer owns each shared hotspot. The resident architect resolves contract questions, implementers
build bounded packages, and defensive, simplifier and editor reviews check the result.</p>
<div class="diagram" role="img" aria-label="Package execution and review">@@SWARM@@</div>
<p>Acceptance includes package tests, the full bench, rv32 compilation and the unsafe ratchet.
Review can be batched for small changes; TCB/security changes get a dedicated round. Deferred
review stays visible as debt and is cleared before a new wave. Keep approved rules in their
specifications and current state in <a href="SWARM.md#claims">Claims</a>.</p>
<p><a href="BUILD-PLAN.md">Remaining packages and acceptance</a> ·
<a href="ANSWERS.md">Approval provenance</a> · <a href="CONTRIBUTING.md">Contribution rules</a></p>
</section>
<section id="start">
<h2>Build and test</h2>
<p><a href="GETTING-STARTED.md">Getting started</a> is the setup and command reference.
It covers Docker, firmware, both widths, images, and separate OTP/Elixir prerequisites.</p>
<p><a href="testbench.md">Testbench</a> defines cases and trusted verdicts; <code>./test --list</code>
lists current coverage. Host tests complement real-kernel boots. <a href="DEBUGGING.md">Debugging</a>
explains source debug builds and QEMU's GDB stub.</p>
<p>Source: <a href="../kernel/">kernel</a> · <a href="../libs/">libraries</a> ·
<a href="../servers/">servers</a> · <a href="../userland/otp/">beamlet</a> ·
<a href="../tests/">boot cases</a> · <a href="../tools/testbench/">harness</a>.</p>
</section>
<section id="next">
<h2>What will be</h2>
<p><a href="PLAN.md">PLAN</a> owns milestone outcomes and attack acceptance:</p>
<ol>
<li>Separated SSH sessions and a leased, contained agent on QEMU.</li>
<li>Signed packages, projects, persistent policy and A/B updates.</li>
<li>Self-hosted development and compilers; the real-agent harness begins alongside milestone 2.</li>
</ol>
<p>SMP follows milestone 1; required full-stack rv32 boots follow milestone 3. Current rv32 boot
tests do not establish full-stack support.</p>
<p><a href="USERLAND.md">Userland boundary</a> · <a href="PACKAGES.md">Packages</a> ·
<a href="IO-ARCHITECTURE.md">I/O</a> · <a href="PLATFORM-FPGA.md">FPGA</a> ·
<a href="GAME.md">Agent attack scenarios</a> · <a href="README.md">Documentation map</a>.</p>
</section>
</main>
<footer>Forked from Xous. <a href="LICENSE">LICENSE</a> / <a href="LICENSES/">LICENSES</a>.
Generated by <a href="../tools/gen_readme.py">tools/gen_readme.py</a> (Graphviz); edit the source and regenerate.</footer>
</body>
</html>
"""

html = TEMPLATE
for key in DIAGRAMS:
    html = put(html, key.upper(), key)

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
