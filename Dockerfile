# The dev environment for this workspace (Redoubt + beamlet), self-contained.
#
# Build and enter it with ./dev.sh. Everything lives in this image:
#   - Rust (stable, via rustup) with the bare-metal targets the Redoubt kernel builds,
#     plus rustfmt/clippy/llvm-tools and cargo-binutils (RustSBI's xtask uses rust-objcopy),
#     and mdbook with its Mermaid and Svgbob preprocessors, which render the book in docs/.
#   - QEMU for BOTH RISC-V widths (qemu-system-riscv32 and qemu-system-riscv64): the test
#     bench boots entirely inside the container. No host QEMU is used.
#   - OpenSSH server and client: the bench spawns /usr/sbin/sshd -i and drives it with ssh.
#   - Node 22 and the agent CLIs: pi, Claude Code, Codex.
#   - A C toolchain, Python and git.
#
# At run time ./dev.sh bind-mounts this directory into /work. See dev.sh for the exact mount
# list; that script is the whole sandbox boundary.

# trixie (Debian 13) supplies the QEMU 10.x version used by the boot and device test matrix.
FROM debian:trixie

ARG DEBIAN_FRONTEND=noninteractive
ARG NODE_MAJOR=22
ARG PI_VERSION=0.86.1
ARG CLAUDE_VERSION=latest
ARG CODEX_VERSION=latest
ARG USERNAME=dev
ARG USER_UID=1000
ARG USER_GID=1000
ARG IMAGE_REV=2

LABEL org.redoubt.dev.revision="${IMAGE_REV}" \
      org.redoubt.dev.uid="${USER_UID}" \
      org.redoubt.dev.gid="${USER_GID}"

# ---------------------------------------------------------------------------
# Base packages
# ---------------------------------------------------------------------------
# build-essential: a C toolchain for -sys crates the bench pulls in (and pcre2 for beamlet
#   differential runs). qemu-system-misc: the virt machine for both RISC-V widths.
# openssh-server: the bench spawns `sshd -i` for its SSH session cases.
RUN apt-get update && apt-get install -y --no-install-recommends \
      build-essential \
      ca-certificates \
      curl \
      git \
      less \
      libssl-dev \
      openssh-server \
      openssh-client \
      pkg-config \
      python3 \
      qemu-system-misc \
      sudo \
      vim-tiny \
      xz-utils \
    && rm -rf /var/lib/apt/lists/*

# ---------------------------------------------------------------------------
# Node 22 (from NodeSource) and the agent CLIs
# ---------------------------------------------------------------------------
RUN curl -fsSL "https://deb.nodesource.com/setup_${NODE_MAJOR}.x" | bash - \
    && apt-get install -y --no-install-recommends nodejs \
    && rm -rf /var/lib/apt/lists/*

RUN npm install -g \
      "@earendil-works/pi-coding-agent@${PI_VERSION}" \
      "@anthropic-ai/claude-code@${CLAUDE_VERSION}" \
      "@openai/codex@${CODEX_VERSION}" \
    && npm cache clean --force

# ---------------------------------------------------------------------------
# Rust toolchain
# ---------------------------------------------------------------------------
# Installed system-wide under /opt so it is present for any user; dev.sh then seeds the
# project's own cache dirs (/work/.cargo, /work/.rustup) from these so builds land inside the
# workspace and are easy to delete.
#
# Targets:
#   riscv64imac / riscv32imac  -- the Redoubt kernel's two widths (bare-metal, no float).
#   riscv64gc                  -- RustSBI's Prototyper firmware.
ENV RUSTUP_HOME=/opt/rustup \
    CARGO_HOME=/opt/cargo \
    PATH=/opt/cargo/bin:$PATH

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
      | sh -s -- -y --default-toolchain stable --profile minimal \
    && rustup component add rustfmt clippy llvm-tools \
    && rustup target add riscv64imac-unknown-none-elf riscv32imac-unknown-none-elf riscv64gc-unknown-none-elf \
    && cargo install --locked cargo-binutils@0.4.0 \
    && cargo install --locked mdbook@0.5.4 mdbook-mermaid@0.17.1 mdbook-svgbob@0.3.1 \
    && chmod -R a+rX /opt/rustup /opt/cargo

# `bash -lc` resets PATH from the login profile, so put the toolchain there too (and in
# /etc/environment for non-shell invocations).
RUN printf 'export PATH=/opt/cargo/bin:$PATH\n' > /etc/profile.d/rust.sh \
    && chmod 0644 /etc/profile.d/rust.sh \
    && printf 'PATH="/opt/cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"\n' > /etc/environment

# ---------------------------------------------------------------------------
# A non-root user whose uid/gid match the host's, so bind-mounted files stay owned by you
# ---------------------------------------------------------------------------
RUN if ! getent group "${USER_GID}" >/dev/null; then groupadd --gid "${USER_GID}" "${USERNAME}"; fi \
    && useradd --uid "${USER_UID}" --gid "${USER_GID}" --create-home --shell /bin/bash "${USERNAME}" \
    && echo "${USERNAME} ALL=(ALL) NOPASSWD:ALL" > /etc/sudoers.d/"${USERNAME}" \
    && chmod 0440 /etc/sudoers.d/"${USERNAME}"

USER ${USERNAME}
WORKDIR /work

# ---------------------------------------------------------------------------
# Pi extensions the dev environment expects: subagent delegation and the
# deepseek-optimised profile. Installed as the user so they land in
# /home/dev/.pi; when dev.sh bind-mounts the host's ~/.pi over it, the host's
# own settings win and pi reconciles the two.
# ---------------------------------------------------------------------------
ENV HOME=/home/dev
RUN pi install npm:pi-subagents \
    && pi install npm:pi-deepseek-optimized

# `pi` talks to the harness through the same env the host uses; nothing about the sandbox is
# hidden from the agent, only from the filesystem.
ENV PI_CODING_AGENT=true

CMD ["/bin/bash"]
