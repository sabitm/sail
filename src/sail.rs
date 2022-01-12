use anyhow::Result;
use anyhow::{bail, Context};
use cradle::output::Status;
use cradle::run_result;
use serde_derive::{Deserialize, Serialize};
use std::{fs, path::Path};

#[derive(Debug, Serialize, Deserialize)]
pub enum LinuxVariant {
    Linux,
    LinuxLts,
    LinuxZen,
    LinuxHardened,
}

impl Default for LinuxVariant {
    fn default() -> Self {
        LinuxVariant::Linux
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub enum ZfsType {
    Normal,
    Dkms,
}

impl Default for ZfsType {
    fn default() -> Self {
        ZfsType::Normal
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub enum StorageType {
    Ssd,
    Hdd,
}

impl Default for StorageType {
    fn default() -> Self {
        StorageType::Ssd
    }
}

pub struct Sail {
    inst_linvar: String,
    inst_zfs: String,
    disk: String,
    inst_partsize_esp: String,
    inst_partsize_bpool: String,
    next_partnum: usize,
    storage_type: StorageType,
}

impl Sail {
    pub fn new(
        linvar: LinuxVariant,
        zfs_type: ZfsType,
        storage_type: StorageType,
        disk: String,
        partsize_esp: String,
        partsize_bpool: String,
    ) -> Result<Self> {
        let linvar = match linvar {
            LinuxVariant::Linux => "linux",
            LinuxVariant::LinuxLts => "linux-lts",
            LinuxVariant::LinuxZen => "linux-zen",
            LinuxVariant::LinuxHardened => "linux-hardened",
        };

        let inst_zfs = "zfs-".to_owned()
            + match zfs_type {
                ZfsType::Normal => linvar,
                ZfsType::Dkms => "dkms",
            };

        let block_test = "test -b ".to_owned() + &disk;
        let Status(block_status) = run_result!(%"bash -c", block_test)?;
        if !block_status.success() {
            bail!("{} is not a block device!", &disk);
        }

        for partsize in [&partsize_esp, &partsize_bpool] {
            let mut partsize_c = partsize.clone();
            if let Some(unit) = partsize_c.pop() {
                match unit {
                    'K' | 'M' | 'G' | 'T' | 'P' => {}
                    _ => bail!(
                        r#""{}" isn't a valid unit in partsize_* (K, M, G, T, P)"#,
                        partsize
                    ),
                }
            }
            partsize_c
                .parse::<usize>()
                .context("Invalid partsize_* size")?;
        }

        Ok(Self {
            inst_linvar: linvar.to_owned(),
            inst_zfs,
            disk: disk.to_owned(),
            inst_partsize_esp: partsize_esp.to_owned(),
            inst_partsize_bpool: partsize_bpool.to_owned(),
            next_partnum: Self::_get_next_partnum(&disk)?,
            storage_type,
        })
    }

    pub fn get_linvar(&self) -> &str {
        &self.inst_linvar
    }

    pub fn get_zfs(&self) -> &str {
        &self.inst_zfs
    }

    pub fn get_disk(&self) -> &str {
        &self.disk
    }

    pub fn get_partsize_esp(&self) -> &str {
        &self.inst_partsize_esp
    }

    pub fn get_partsize_bpool(&self) -> &str {
        &self.inst_partsize_bpool
    }

    pub fn get_efi_part(&self) -> Result<String> {
        let efi_part = format!("{}-part{}", self.disk, self.get_next_partnum());

        Ok(efi_part)
    }

    pub fn get_bpool_part(&self) -> Result<String> {
        let bpool_part = format!("{}-part{}", self.disk, self.get_next_partnum() + 1);

        Ok(bpool_part)
    }

    pub fn get_rpool_part(&self) -> Result<String> {
        let rpool_part = format!("{}-part{}", self.disk, self.get_next_partnum() + 2);

        Ok(rpool_part)
    }

    pub fn get_efi_last_path(&self) -> Result<String> {
        let efi_part = self.get_efi_part()?;
        let suffix = efi_part.split('/');
        let suffix = suffix
            .last()
            .context("split efi path to get the last part")?;

        Ok(suffix.to_owned())
    }

    pub fn get_disk_last_path(&self) -> Result<&str> {
        let suffix = self.disk.split('/');
        let suffix = suffix
            .last()
            .context("split disk path to get the last part")?;

        Ok(suffix)
    }

    pub fn get_next_partnum(&self) -> usize {
        self.next_partnum
    }

    fn _get_next_partnum(disk: &str) -> Result<usize> {
        let disk_path = Path::new(disk);
        let disk_parent = disk_path
            .parent()
            .context("get the parent directory of $disk")?;
        let suffix = disk.split('/');
        let disk_last_path = suffix
            .last()
            .context("split disk path to get the last part")?;

        let mut last_partnum = 0;

        if disk_parent.is_dir() {
            for dev in fs::read_dir(disk_parent)? {
                let dev = dev?.file_name();
                let dev = if let Ok(dev) = dev.into_string() {
                    dev
                } else {
                    bail!("not a valid unicode!");
                };

                if dev.contains(disk_last_path) {
                    last_partnum += 1;
                }
            }
        } else {
            bail!("invalid parent disk directory!");
        }

        Ok(last_partnum)
    }

    pub fn is_using_ssd(&self) -> bool {
        match self.storage_type {
            StorageType::Ssd => true,
            StorageType::Hdd => false,
        }
    }
}
