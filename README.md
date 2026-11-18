# Keel

A fast, lightweight code editor for macOS, written in Rust.

Keel is built on [GPUI](https://github.com/zed-industries/zed), the GPU-accelerated UI framework from Zed, and on [gpui-component](https://github.com/longbridge/gpui-component). It aims to run Visual Studio Code extensions through a compatibility bridge, starting with the ones its author actually uses.

> **Status:** early development, phase 0 (prototype). Not ready for daily use.

## Requirements

- macOS
- Rust through [rustup](https://rustup.rs). The toolchain pinned in `rust-toolchain.toml` is installed automatically.
- Xcode with the Metal toolchain, selected with `xcode-select`

## Build and run

```bash
cargo run -p keel
```

## Repository layout

| Path | Contents |
|---|---|
| `crates/keel` | the application |
| `fixtures/` | files used by tests, such as extension packages |
| `docs/` | development plan and decisions (in Italian) |

## Development plan

The full plan, split into phases, is in [`docs/PIANO.md`](docs/PIANO.md), and a handoff with the current state is in [`docs/HANDOFF.md`](docs/HANDOFF.md) (both in Italian).
