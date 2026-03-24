# cbox

Contained Box — OS-level sandboxing for AI agents and arbitrary commands. Run anything with full shell access — filesystem, network, and process mutations are isolated. Nothing touches your real system until you approve.

Works on **Linux** (native namespaces) and **macOS** (via Docker/Podman). Same CLI, same workflow.

![cbox demo](demo.gif)

Network is denied by default — pass `--network allow` to grant access.

```
~/downloads $ cbox run --network allow -- claude
cbox session a3f7c012 (adapter: claude, backend: native, persist: false)

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

cbox has two backends that provide the same isolation guarantees:

### Native backend (Linux)
- **User/PID/mount/network/UTS/IPC namespaces** via `unshare(2)` — the agent runs as fakeroot in its own process tree
- **OverlayFS** — project files appear read-write inside the sandbox, but all writes go to a separate upper layer
- **Seccomp-BPF** — blocks `mount`, `ptrace`, `bpf`, `kexec_load`, and other escape vectors
- **Cgroups v2** — enforces memory, CPU, and PID limits
- **Network isolation** — deny-all by default with explicit host:port whitelist via veth pairs and iptables

### Container backend (macOS, Linux fallback)
- **Docker or Podman** — auto-detected, no configuration needed
- **Auto-built base image** — on first run, cbox builds a `cbox-base` image with common tools (bash, zsh, fish, git, vim, curl, build-essential) and **Claude Code** pre-installed
- **OverlayFS via tmpfs** — uses a tmpfs-backed overlay inside the container
- **macOS note** — falls back to copy-based isolation due to virtiofs incompatibility with overlayfs (slightly slower initial setup)
- **`--network=none`** for deny mode, `--network=host` for allow mode (needed for OAuth callbacks)
- **Resource limits** via `--memory`, `--cpu-quota`, `--pids-limit`
- **Custom images** — use `--image` or set `sandbox.image` in config to bring your own toolchain

The backend is selected automatically (`--backend auto` is the default):
- Linux with user namespaces → native
- Linux without namespaces → container (fallback)
- macOS → container

After the agent exits, `cbox diff` shows exactly what changed. `cbox merge` applies your approved changes to the real filesystem. Everything else is discarded.

## Install

### Homebrew (macOS/Linux)

```
brew tap borngraced/cbox
brew install cbox
```

### Pre-built binaries

Download from [GitHub Releases](https://github.com/borngraced/cbox/releases).

### From source

```
cargo install --git https://github.com/borngraced/cbox cbox
```

<details>
<summary>System requirements</summary>

#### Linux (native backend)
| Feature | Requirement | Fallback |
|---|---|---|
| Sandboxing | User namespaces (`kernel.unprivileged_userns_clone=1`) | Container backend |
| File isolation | OverlayFS (`CONFIG_OVERLAY_FS`) | fuse-overlayfs |
| Resource limits | Cgroups v2 | Disabled with warning |
| Network rules | `iptables` + `ip` | Empty netns (no connectivity) |

#### macOS (container backend)
| Feature | Requirement |
|---|---|
| Container runtime | Docker Desktop, OrbStack, or Podman |

</details>

## Usage

```
cbox run [OPTIONS] [-- <CMD>]     # launch sandbox (defaults to $SHELL)
cbox diff [SESSION]               # show what changed
cbox merge [--pick] [SESSION]     # apply changes to real filesystem
cbox destroy [SESSION]            # tear down session
cbox save [--name NAME] [SESSION] # snapshot session
cbox list [--json]                # list sessions
```

<details>
<summary>Persistence</summary>

By default, sessions are one-shot: run a command, review changes with `cbox diff`, apply them with `cbox merge`, then clean up with `cbox destroy`.

With `--persist`, the session's overlay data is kept after exit so you can re-enter it later, compare multiple sessions side by side, or snapshot it with `cbox save`.

</details>

### Examples

```bash
# Drop into a sandboxed shell (with Claude Code available)
cbox run --network allow

# Run Claude Code directly
cbox run --network allow -- claude

# Run a specific command
cbox run -- python3 train.py

# Named persistent session with resource limits
cbox run --session experiment --persist --memory 2G --cpu 100% -- bash

# Use a custom container image
cbox run --image myuser/cbox-dev:latest

# Force container backend on Linux
cbox run --backend container -- npm start

# Review and selectively apply changes
cbox diff --stat
cbox merge --pick

# Clean up
cbox destroy
```

<details>
<summary>Custom container image</summary>

The default base image includes common tools and Claude Code. For a custom setup, use the included `Dockerfile` as a starting point:

```bash
# Build your personal image
docker build -t cbox-dev .

# Use it
cbox run --image cbox-dev --network allow

# Or set permanently in cbox.toml:
# [sandbox]
# image = "cbox-dev"
```

</details>

<details>
<summary>Configuration</summary>

cbox uses layered config resolution:

1. Built-in defaults
2. Global: `~/.config/cbox/config.toml`
3. Per-project: `./cbox.toml`

```toml
[sandbox]
# Container image (ignored on native backend)
image = "ubuntu:24.04"
ro_mounts = ["/usr", "/lib", "/lib64", "/bin", "/sbin", "/etc"]
blocked_syscalls = []
merge_exclude = [
    "root/.bash_history",
    "root/.cache/**",
    "root/.local/**",
    "root/.config/**",
    "home/**",
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

</details>

<details>
<summary>Adapters</summary>

cbox ships two adapters that customize sandbox behavior for specific tools:

- **generic** — pass-through, runs the command as-is
- **claude** — resolves the Claude binary, sets `ANTHROPIC_API_KEY`, `HOME`, `CLAUDE_CODE_SANDBOX=cbox`, bind-mounts `~/.claude` read-write

Auto-detection: commands containing "claude" use the claude adapter, everything else uses generic.

</details>

<details>
<summary>Architecture</summary>

```
bins/cbox/          CLI binary (clap, subcommand dispatch)
crates/
  cbox-core/        Config, session store, capability detection, SandboxBackend trait
  cbox-sandbox/     Native backend: namespace setup, seccomp-BPF, cgroups v2
  cbox-container/   Container backend: Docker/Podman, auto-built base image
  cbox-overlay/     OverlayFS mount/diff/merge, whiteout detection
  cbox-network/     Veth pairs, iptables rules, DNS
  cbox-adapter/     AgentAdapter trait, generic + claude adapters
  cbox-diff/        Colored diff rendering, interactive file picker
```

</details>

## License

MIT
