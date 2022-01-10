use crate::{
    sail::{LinuxVariant, Sail},
    StorageType, ZfsType,
};
use anyhow::{bail, Result};
use serde_derive::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Config {
    linvar: LinuxVariant,
    zfs_type: ZfsType,
    storage_type: StorageType,
    disk: String,
    partsize_esp: String,
    partsize_bpool: String,
}

pub fn generate_conf() -> Result<()> {
    let _: Config = confy::load_path("sail.toml")?;

    bail!(
        "./sail.toml not found, \
          generating a new one...\n\
          Edit beforehand"
    );
}

pub fn parse_conf() -> Result<Sail> {
    let conf: Config = confy::load_path("sail.toml")?;

    dbg!(&conf);
    let sail = Sail::new(
        conf.linvar,
        conf.zfs_type,
        conf.storage_type,
        conf.disk,
        conf.partsize_esp,
        conf.partsize_bpool,
    )?;

    Ok(sail)
}
