mod sail;
mod setup;
mod string_res;

use crate::sail::Sail;
use anyhow::Result;
use sail::{LinuxVariant, StorageType, ZfsType};

fn main() -> Result<()> {
    let sail = Sail::new(
        LinuxVariant::LinuxLts,
        ZfsType::Normal,
        StorageType::Ssd,
        "/dev/disk/by-path/virtio-pci-0000:04:00.0",
        "500M",
        "3.5G",
    )?;

    setup::command_checker()?;
    setup::check_as_root()?;
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

    // TODO: wrapper fn for eprintln!
    // TODO: nix install script

    Ok(())
}
