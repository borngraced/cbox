pub mod destroy;
pub mod diff;
pub mod list;
pub mod merge;
pub mod run;
pub mod save;

use anyhow::Result;

use crate::cli::{Cli, Command};

pub fn dispatch(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Run {
            adapter,
            persist,
            session,
            network,
            memory,
            cpu,
            dry_run,
            backend,
            image,
            cmd,
        } => run::execute(
            adapter, persist, session, network, memory, cpu, dry_run, backend, image, cmd,
        ),

        Command::Diff {
            stat,
            name_only,
            session,
        } => diff::execute(stat, name_only, session),

        Command::Merge {
            pick,
            force,
            dry_run,
            session,
        } => merge::execute(pick, force, dry_run, session),

        Command::Destroy {
            all,
            force,
            session,
        } => destroy::execute(all, force, session),

        Command::Save { name, session } => save::execute(name, session),

        Command::List { all, json } => list::execute(all, json),
    }
}
