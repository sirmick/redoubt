# The dev image: Debian with what scripts/setup.sh installs, and nothing else.
#
# Build and enter it with ./dev.sh, which mounts this checkout at /work. The script is the one
# list of prerequisites; running it here, with --with-beam, proves it on every build:
#   - Rust (rustup, under /opt) as rust-toolchain.toml pins it, the pinned cargo tools and mdbook;
#   - QEMU for both RISC-V widths and OpenSSH's client: the bench boots and drives everything
#     in here, from Debian's own packages;
#   - beamlet's pinned OTP and Elixir under /opt/toolchains, named by BEAMLET_TOOLCHAINS.
# The firmware and the build need the checkout, so they happen at run time, not here: the
# firmware's own nightly (bios/rust-toolchain.toml) is fetched then, and that needs the network.
FROM debian:trixie

ARG USERNAME=dev
ARG USER_UID=1000
ARG USER_GID=1000
ARG IMAGE_REV=5

LABEL org.redoubt.dev.revision="${IMAGE_REV}" \
      org.redoubt.dev.uid="${USER_UID}" \
      org.redoubt.dev.gid="${USER_GID}"

# Installed system-wide under /opt so it is present for any user; dev.sh then seeds the
# project's own /work/.cargo and /work/.rustup from these, so builds land inside the checkout.
# Elixir expects a UTF-8 locale (else the VM names files in latin1).
ENV RUSTUP_HOME=/opt/rustup \
    CARGO_HOME=/opt/cargo \
    PATH=/opt/cargo/bin:$PATH \
    BEAMLET_TOOLCHAINS=/opt/toolchains \
    LANG=C.UTF-8

COPY rust-toolchain.toml /tmp/redoubt/
COPY scripts/setup.sh /tmp/redoubt/scripts/
RUN /tmp/redoubt/scripts/setup.sh --with-beam --toolchains /opt/toolchains \
    && rm -rf /tmp/redoubt /var/lib/apt/lists/* \
    && chmod -R a+rX /opt/rustup /opt/cargo /opt/toolchains

# A user whose uid/gid match the host's, so files created in /work stay owned by you. OpenSSH
# refuses to run for a uid that NSS cannot resolve, so the user is real.
RUN if ! getent group "${USER_GID}" >/dev/null; then groupadd --gid "${USER_GID}" "${USERNAME}"; fi \
    && useradd --uid "${USER_UID}" --gid "${USER_GID}" --create-home --shell /bin/bash "${USERNAME}"

USER ${USERNAME}
WORKDIR /work
CMD ["/bin/bash"]
