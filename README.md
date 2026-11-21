<div align="center">

<h1>
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/lotype-dark.svg">
    <img alt="Lotype" src="assets/lotype.svg" width="336">
  </picture>
</h1>

<p>
  A native Rust SDK for block-based document editors.<br/>
  WYSIWYM engine with realtime collaboration on Loro, and a GPUI WYSIWYG editor — no browser, no WebView.
</p>

<p>
  <a href="https://github.com/lotype/lotype/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/lotype/lotype/actions/workflows/ci.yml/badge.svg"></a>
  <a href="https://www.rust-lang.org"><img alt="Rust 1.90+" src="https://img.shields.io/badge/rust-1.90%2B-orange?logo=rust"></a>
  <a href="#license"><img alt="License: MIT OR Apache-2.0" src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue"></a>
  <img alt="Status: alpha" src="https://img.shields.io/badge/status-alpha-red">
</p>

<p>
  <a href="https://lotype.dev">Website</a> ·
  <a href="https://docs.lotype.dev">Docs</a> ·
  <a href="ARCHITECTURE.md">Architecture</a>
</p>

</div>

## Background

**Lotype** is named after the *line-o'-type* machine, which cast an entire line of text as one solid slug of metal instead of setting it letter by letter. Lotype handles documents the same way: as solid blocks.

The SDK grew out of an editor built on **Tauri + React + Tiptap/BlockNote**. Two problems in that stack could not be fixed from inside it:

- **The browser engine differs on every OS.** Tauri uses whatever WebView the OS ships (WKWebView, WebView2, WebKitGTK). Even ProseMirror carries dozens of per-browser workarounds, most of them in IME, selection and keyboard handling, which is the heart of an editor.
- **The layers patch each other.** BlockNote imposes a block model with IDs on top of ProseMirror's position-based text tree. Each block becomes three wrapper nodes, IDs are re-applied after every transaction, and helpers keep translating positions to blocks. Every upgrade of a lower layer can break the layer above it.

Lotype replaces that stack with a single native one:

- **`lotype-core`** plays the role of ProseMirror + Yjs. Blocks with stable IDs are the native data model, built on the [Loro](https://github.com/loro-dev/loro) CRDT.
- **`lotype-ui`** plays the role of Tiptap + BlockNote. It renders straight to the GPU through [GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui).

There is no DOM, no WebView and no patching layer in between.

The first release targets **Rust/GPUI applications on macOS** (Intel and Apple Silicon). See [ARCHITECTURE.md](ARCHITECTURE.md) for the design and its trade-offs.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <https://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <https://opensource.org/licenses/MIT>)

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in Lotype by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
