# Contributing to cbox

## Setup

```bash
git clone https://github.com/borngraced/cbox
cd cbox
cargo build
cargo test
```

Requires Rust stable. On Linux, tests run natively. On macOS, you need Docker or Podman for the container backend.

## Development

```bash
cargo build                    # debug build
cargo build --release          # release build
cargo test                     # run all tests
cargo clippy --all-targets     # lint
cargo fmt --all                # format
```

### Running locally

```bash
# Native backend (Linux, needs sudo for namespaces)
sudo ./target/release/cbox run -- bash

# Container backend (any platform with Docker/Podman)
./target/release/cbox run --backend container -- bash
```

## Project structure

```
bins/cbox/          CLI binary
crates/
  cbox-core/        Config, sessions, SandboxBackend trait
  cbox-sandbox/     Native backend (Linux namespaces, seccomp, cgroups)
  cbox-container/   Container backend (Docker/Podman)
  cbox-overlay/     OverlayFS diff/merge
  cbox-network/     Veth pairs, iptables
  cbox-adapter/     AgentAdapter trait, generic + claude adapters
  cbox-diff/        Diff rendering
```

## Custom adapters

Adapters customize sandbox behavior for specific tools. See `crates/cbox-adapter/src/claude.rs` for an example. Implement the `AgentAdapter` trait and register it in `AdapterRegistry::new()`.

## Submitting changes

1. Fork the repo
2. Create a branch (`git checkout -b my-feature`)
3. Make your changes
4. Ensure `cargo test`, `cargo clippy`, and `cargo fmt --check` pass
5. Open a PR against `main`

Keep PRs focused — one feature or fix per PR.
