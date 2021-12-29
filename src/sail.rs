use anyhow::Result;
use anyhow::{bail, Context};
use std::{fs, path::Path};

pub enum LinuxVariant {
    Linux,
    LinuxLts,
    LinuxZen,
    LinuxHardened,
}

pub enum ZfsType {
    Normal,
    Dkms,
}

pub enum StorageType {
    Ssd,
    Hdd,
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
        disk: &str,
        partsize_esp: &str,
        partsize_bpool: &str,
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

        Ok(Self {
            inst_linvar: linvar.to_owned(),
            inst_zfs,
            disk: disk.to_owned(),
            inst_partsize_esp: partsize_esp.to_owned(),
            inst_partsize_bpool: partsize_bpool.to_owned(),
            next_partnum: Self::_get_next_partnum(disk)?,
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

    pub fn get_disk_parent(&self) -> Result<&str> {
        let disk_path = Path::new(&self.disk);
        let disk_parent = disk_path
            .parent()
            .context("get the parent directory of $disk")?;
        let disk_parent = disk_parent
            .to_str()
            .context("convert disk parent Path to str")?;

        Ok(disk_parent)
    }

    pub fn get_next_partnum(&self) -> usize {
        self.next_partnum
        // 1
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
