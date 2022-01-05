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
        "1G",
        "4G",
    )?;

    setup::init_check()?;
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

    Ok(())
}
