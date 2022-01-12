use crate::parse_conf;
use anyhow::Result;
use argh::FromArgs;
use std::path::Path;

pub enum SailState {
    Start,
    Exec,
    List,
}

#[derive(FromArgs)]
/// Arch linux installation and post-installation script
struct SailArgs {
    #[argh(subcommand)]
    sailsubs: SailSubCommand,
}

#[derive(FromArgs)]
#[argh(subcommand)]
enum SailSubCommand {
    Start(StartCmd),
    Exec(ExecCmd),
    List(ListCmd),
}

#[derive(FromArgs)]
#[argh(subcommand, name = "start")]
/// start Arch linux installation
struct StartCmd {}

#[derive(FromArgs)]
#[argh(subcommand, name = "exec")]
/// execute a script (post-installation)
struct ExecCmd {
    #[argh(option, short = 's')]
    /// specify script name to execute
    script: String,
}

#[derive(FromArgs)]
#[argh(subcommand, name = "list")]
/// list all available script
struct ListCmd {}

pub fn parse_args() -> Result<SailState> {
    let sail_args: SailArgs = argh::from_env();

    let sailsubs = sail_args.sailsubs;
    match sailsubs {
        SailSubCommand::Start(_) => {
            let conf_path = Path::new("sail.toml");
            if !conf_path.is_file() {
                parse_conf::generate_conf()?;
            }
            Ok(SailState::Start)
        }
        SailSubCommand::Exec(execopt) => Ok(SailState::Exec),
        SailSubCommand::List(_) => Ok(SailState::List),
    }
}
