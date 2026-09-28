---
title: Install and build
description: Toolchain, system libraries, the build, and access to Claude.
---

[Manual](./) | Next: [Getting started](getting-started.md)

# Install and build

## What you need

- **Rust.** A stable toolchain from [rustup](https://rustup.rs). The tree uses edition 2021 and is built with rustc 1.98.
- **A C compiler and pkg-config.** Some dependencies compile C at build time (the Vorbis and Ogg encoders, the TLS library of the API client).
- **ALSA development headers.** Audio output goes through ALSA, found by pkg-config. On Debian or Ubuntu: `apt install build-essential pkg-config libasound2-dev`. On Fedora: `dnf install gcc pkgconf alsa-lib-devel`.
- **A graphical session.** The studio opens its window through winit and draws with wgpu. The X11 (or Wayland) client libraries, libxkbcommon and a Vulkan or OpenGL driver are loaded at run time, not linked, so the build does not need their headers. The tested setup is X11.
- **Claude**, only to write new songs. Either the [Claude Code](https://docs.anthropic.com/en/docs/claude-code) `claude` command on your `PATH`, logged in to your account, or an Anthropic API key in `ANTHROPIC_API_KEY`. Playing, rendering and reading songs you already have needs neither.

No network access is needed to build. Every Rust dependency is vendored in `vendor/`, and `.cargo/config.toml` points Cargo there.

## Build

From the repository root:

    cargo build --release -p studio -p sunflower

This produces two programs:

- `target/release/studio`: Sunflower Studio, the desktop app.
- `target/release/sunflower`: the command-line renderer.

`cargo build --release` builds the whole workspace, these two included. Build in release mode: the synthesis is heavy, and a debug build renders many times slower.

To run the tests:

    cargo test --release --workspace

## Access to Claude

The studio and `sunflower write` reach Claude in one of two ways.

**The claude CLI (default).** The engine runs `claude -p --output-format json` with your logged-in account. It runs in a fresh empty temporary directory, with CLAUDE.md files, auto-memory, MCP servers, tools and session persistence switched off, so the request carries only the songwriter prompt. Your own `~/.claude/CLAUDE.md` still reaches the model in this mode. Check the CLI with `claude --version`, and log in by running `claude` once.

**The Anthropic API.** Set `ANTHROPIC_API_KEY` in the environment that starts the studio or `sunflower`. The request goes to the Messages API with a 900 s timeout and up to three retries on rate limits, server errors and transport failures. This path has not yet been exercised with a live call; the CLI path has.

Either way the default model is `claude-opus-5-5` at effort `high`. The New song form and `sunflower write --model` take another model id.

## Install

There is no installer. Copy or link the two binaries onto your `PATH`, for example:

    install -m 755 target/release/studio target/release/sunflower ~/.local/bin/

The demo song is compiled into both, and the music glyphs of the sheet view (Bravura) into the studio. The sheet's text (title, lyrics, chord names) uses a system serif font: DejaVu Serif, Liberation Serif or Georgia when installed, else any serif.
