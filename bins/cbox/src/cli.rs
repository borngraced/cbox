use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "cbox",
    about = "Kernel-enforced OS-level sandboxing for AI agents",
    version,
    long_about = "Claude's Box — run AI agents with full shell access inside an isolated sandbox.\n\
                  All mutations (filesystem, network, processes) are contained via Linux namespaces.\n\
                  Nothing touches the real system until you explicitly approve."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Launch an agent inside a sandbox
    Run {
        /// Adapter to use (auto, generic, claude)
        #[arg(long, default_value = "auto")]
        adapter: String,

        /// Keep session after process exits
        #[arg(long)]
        persist: bool,

        /// Named session
        #[arg(long)]
        session: Option<String>,

        /// Network mode: deny or allow
        #[arg(long, default_value = "deny")]
        network: String,

        /// Memory limit (e.g., 4G, 512M)
        #[arg(long)]
        memory: Option<String>,

        /// CPU limit as percentage (e.g., 200%)
        #[arg(long)]
        cpu: Option<String>,

        /// Show what would be done without actually doing it
        #[arg(long)]
        dry_run: bool,

        /// Backend: auto, native, or container
        #[arg(long, default_value = "auto")]
        backend: String,

        /// Container image to use (e.g., "ubuntu:24.04", "myuser/cbox-dev:latest")
        #[arg(long)]
        image: Option<String>,

        /// Command to run inside the sandbox (defaults to $SHELL)
        #[arg(last = true)]
        cmd: Vec<String>,
    },

    /// Show what the agent changed
    Diff {
        /// Show file stats only
        #[arg(long)]
        stat: bool,

        /// Show changed file names only
        #[arg(long)]
        name_only: bool,

        /// Session ID or name (defaults to most recent)
        session: Option<String>,
    },

    /// Apply sandbox changes to real filesystem
    Merge {
        /// Interactively pick which changes to apply
        #[arg(long)]
        pick: bool,

        /// Skip confirmation prompt
        #[arg(long)]
        force: bool,

        /// Show what would be merged without doing it
        #[arg(long)]
        dry_run: bool,

        /// Session ID or name (defaults to most recent)
        session: Option<String>,
    },

    /// Tear down a session and clean up resources
    Destroy {
        /// Destroy all sessions
        #[arg(long)]
        all: bool,

        /// Skip confirmation prompt
        #[arg(long)]
        force: bool,

        /// Session ID or name
        session: Option<String>,
    },

    /// Snapshot a session for later use
    Save {
        /// Name for the snapshot
        #[arg(long)]
        name: Option<String>,

        /// Session ID or name (defaults to most recent)
        session: Option<String>,
    },

    /// List active and saved sessions
    List {
        /// Include saved sessions
        #[arg(long)]
        all: bool,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}
