# cbox

Kernel-enforced sandboxing for AI agents. Run any agent with full shell access — filesystem, network, and process mutations are isolated via Linux namespaces. Nothing touches your real system until you approve.

```
~/downloads $ cbox run --network allow -- claude
cbox session a3f7c012 (adapter: claude, persist: false)

> clean up this directory — delete duplicates, organize files by type,
  remove anything over 6 months old

claude: I'll reorganize your downloads folder...
  - Deleted 47 duplicate files
  - Moved 23 images to images/
  - Moved 8 PDFs to documents/
  - Removed 31 files older than 6 months
  Done.

> exit

~/downloads $ cbox diff --stat
 A images/screenshot-2024.png
 A documents/invoice.pdf
 D IMG_2024 (1).png
 D IMG_2024 (2).png
 D old-installer.dmg
 ... 109 files changed: 31 added, 0 modified, 78 deleted

~/downloads $ cbox merge --pick
[1/109] A images/screenshot-2024.png — accept? [y/n/q] y
[2/109] D old-installer.dmg — accept? [y/n/q] y
...
```

## How it works

cbox creates an isolated environment using Linux kernel primitives:

- **User/PID/mount/network/UTS/IPC namespaces** via `unshare(2)` — the agent runs as fakeroot in its own process tree
- **OverlayFS** — project files appear read-write inside the sandbox, but all writes go to a separate upper layer
- **Seccomp-BPF** — blocks `mount`, `ptrace`, `bpf`, `kexec_load`, and other escape vectors
- **Cgroups v2** — enforces memory, CPU, and PID limits
- **Network isolation** — deny-all by default with explicit host:port whitelist via veth pairs and iptables

After the agent exits, `cbox diff` shows exactly what changed. `cbox merge` applies your approved changes to the real filesystem. Everything else is discarded.

## Install

Requires Linux with user namespace support (most distros since kernel 3.8).

```
git clone https://github.com/borngraced/cbox
cd cbox
cargo build --release
sudo cp target/release/cbox /usr/local/bin/
```

### System requirements

| Feature | Requirement | Fallback |
|---|---|---|
| Sandboxing | User namespaces (`kernel.unprivileged_userns_clone=1`) | None (required) |
| File isolation | OverlayFS (`CONFIG_OVERLAY_FS`) | fuse-overlayfs |
| Resource limits | Cgroups v2 | Disabled with warning |
| Network rules | `iptables` + `ip` | Empty netns (no connectivity) |

## Usage

```
cbox run [OPTIONS] [-- <CMD>]     # launch sandbox (defaults to $SHELL)
cbox diff [SESSION]               # show what changed
cbox merge [--pick] [SESSION]     # apply changes to real filesystem
cbox destroy [SESSION]            # tear down session
cbox save [--name NAME] [SESSION] # snapshot session
cbox list [--json]                # list sessions
```

### Examples

```bash
# Drop into a sandboxed shell
cbox run

# Run a specific command
cbox run -- python3 train.py

# Claude Code with network access to Anthropic API
cbox run --network allow -- claude

# Named persistent session with resource limits
cbox run --session experiment --persist --memory 2G --cpu 100% -- bash

# Review and selectively apply changes
cbox diff --stat
cbox merge --pick

# Clean up
cbox destroy
```

## Configuration

cbox uses layered config resolution:

1. Built-in defaults
2. Global: `~/.config/cbox/config.toml`
3. Per-project: `./cbox.toml`

```toml
[sandbox]
ro_mounts = ["/usr", "/lib", "/lib64", "/bin", "/sbin", "/etc"]
blocked_syscalls = []
merge_exclude = [
    "root/.bash_history",
    "root/.cache/**",
    "root/.local/**",
    "root/.config/**",
]

[network]
mode = "deny"
allow = ["api.anthropic.com:443"]
dns = ["8.8.8.8", "8.8.4.4"]

[resources]
memory = "4G"
cpu = "200%"
max_pids = 4096

[adapter]
default = "auto"
env_passthrough = ["ANTHROPIC_API_KEY"]
```

## Adapters

cbox ships two adapters that customize sandbox behavior for specific tools:

- **generic** — pass-through, runs the command as-is
- **claude** — sets `ANTHROPIC_API_KEY`, `HOME`, `CLAUDE_CODE_SANDBOX=cbox`

Auto-detection: commands containing "claude" use the claude adapter, everything else uses generic.

## Architecture

```
bins/cbox/          CLI binary (clap, subcommand dispatch)
crates/
  cbox-core/        Config, session store, capability detection
  cbox-sandbox/     Namespace setup, seccomp-BPF, cgroups v2
  cbox-overlay/     OverlayFS mount/diff/merge, whiteout detection
  cbox-network/     Veth pairs, iptables rules, DNS
  cbox-adapter/     AgentAdapter trait, generic + claude adapters
  cbox-diff/        Colored diff rendering, interactive file picker
```

## License

MIT
