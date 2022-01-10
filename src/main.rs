mod parse_args;
mod parse_conf;
mod sail;
mod setup;
mod string_res;

use crate::sail::Sail;
use anyhow::Result;
use parse_args::SailState;
use sail::{StorageType, ZfsType};

fn start(sail: Sail) -> Result<()> {
    setup::check_as_root()?;
    setup::init_check()?;
    setup::partition_disk(&sail)?;
    setup::format_disk(&sail)?;
    setup::pacstrap(&sail)?;
    setup::system_configuration(&sail)?;
    setup::install_aurs()?;
    setup::workarounds()?;
    setup::bootloaders(&sail)?;
    setup::finishing(&sail)?;
    setup::post_scripts_gen()?;
    setup::shot_and_clean()?;

    Ok(())
}

fn main() -> Result<()> {
    match parse_args::parse_args()? {
        SailState::Start => {
            start(parse_conf::parse_conf()?)?;
        }
        SailState::Exec => {}
        SailState::List => {}
    }

    Ok(())
}
