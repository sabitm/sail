use anyhow::{bail, Context, Result};
use cradle::input::Stdin;
use cradle::output::StdoutTrimmed;
use cradle::prelude::*;
use std::fs;
use std::io::Write;

use std::path::Path;
use std::{fs::OpenOptions, thread, time};

struct Sail {
    inst_linvar: String,
    inst_id: String,
    disk: String,
    inst_partsize_esp: String,
    inst_partsize_bpool: String,
    efi_part: String,
    bpool_part: String,
    rpool_part: String,
}

impl Sail {
    fn new(id: &str, linvar: &str, disk: &str, partsize_esp: &str, partsize_bpool: &str) -> Self {
        Sail {
            inst_linvar: linvar.to_owned(),
            inst_id: id.to_owned(),
            disk: disk.to_owned(),
            inst_partsize_esp: partsize_esp.to_owned(),
            inst_partsize_bpool: partsize_bpool.to_owned(),
            efi_part: disk.to_owned() + "-part1",
            bpool_part: disk.to_owned() + "-part2",
            rpool_part: disk.to_owned() + "-part3",
        }
    }

    fn get_efi_last_path(&self) -> Result<&str> {
        let suffix = self.efi_part.split('/');
        let suffix = suffix
            .last()
            .context("split efi path to get the last part")?;

        Ok(suffix)
    }

    fn get_disk_last_path(&self) -> Result<&str> {
        let suffix = self.disk.split('/');
        let suffix = suffix
            .last()
            .context("split disk path to get the last part")?;

        Ok(suffix)
    }

    fn get_next_partnum(&self) -> Result<usize> {
        let disk_path = Path::new(&self.disk);
        let disk_path = disk_path
            .parent()
            .context("get the parent directory of $disk")?;
        let disk_last_path = self.get_disk_last_path()?;
        let mut last_partnum = 0;

        if disk_path.is_dir() {
            for dev in fs::read_dir(disk_path)? {
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
}

fn command_checker() -> Result<()> {
    let commands = [
        "chmod",
        "id",
        "mkfs.vfat",
        "mkdir",
        "mount",
        "pacstrap",
        "sgdisk",
        "zfs",
        "zpool",
    ];

    for cmd in commands {
        let StdoutTrimmed(_) = run_result!("which", cmd)?;
    }

    Ok(())
}

fn check_as_root() -> Result<()> {
    let StdoutTrimmed(uid) = run_output!(%"id -u");

    if uid != "0" {
        bail!("Must be run as root!");
    }

    Ok(())
}

fn stage_1(sail: &Sail) -> Result<()> {
    let duration = time::Duration::from_secs(1);

    let disk = &sail.disk;
    let efi_part = &sail.efi_part;
    let partsize_esp = &sail.inst_partsize_esp;
    let bpool_part = &sail.bpool_part;
    let partsize_bpool = &sail.inst_partsize_bpool;
    let rpool_part = &sail.rpool_part;

    eprintln!("\nload zfs kernel module...\n");
    run_result!(%"modprobe zfs")?;

    eprintln!("\nfind last partition number...\n");
    let mut next_partnum = sail.get_next_partnum()?;

    eprintln!("\ncreate efi partition...\n");
    let efi_partnum = next_partnum.to_string();
    let part_desc = "-n".to_owned() + efi_partnum.as_str() + ":0:+" + partsize_esp;
    let part_type = "-t".to_owned() + efi_partnum.as_str() + ":EF00";

    run_result!("sgdisk", part_desc, part_type, disk)?;

    eprintln!("\ncreate bpool partition...\n");
    next_partnum += 1;
    let bpool_partnum = next_partnum.to_string();
    let part_desc = "-n".to_owned() + bpool_partnum.as_str() + ":0:+" + partsize_bpool;
    let part_type = "-t".to_owned() + bpool_partnum.as_str() + ":BE00";

    run_result!(%"sgdisk", part_desc, part_type, disk)?;

    eprintln!("\ncreate rpool partition...\n");
    next_partnum += 1;
    let rpool_partnum = next_partnum.to_string();
    let part_desc = "-n".to_owned() + rpool_partnum.as_str() + ":0:0";
    let part_type = "-t".to_owned() + rpool_partnum.as_str() + ":BF00";

    run_result!(%"sgdisk", part_desc, part_type, disk)?;

    thread::sleep(duration);

    eprintln!("\ncreate boot pool...\n");
    run_result!(%"zpool create",
        %"-o compatibility=grub2",
        %"-o ashift=12",
        %"-o autotrim=on",
        %"-O acltype=posixacl",
        %"-O canmount=off",
        %"-O compression=lz4",
        %"-O devices=off",
        %"-O normalization=formD",
        %"-O relatime=on",
        %"-O xattr=sa",
        %"-O mountpoint=/boot",
        %"-R /mnt",
        "bpool",
        bpool_part)?;

    eprintln!("\ncreate root pool...\n");
    run_result!(%"zpool create",
        %"-o ashift=12",
        %"-o autotrim=on",
        %"-R /mnt",
        %"-O acltype=posixacl",
        %"-O canmount=off",
        %"-O compression=zstd",
        %"-O dnodesize=auto",
        %"-O normalization=formD",
        %"-O relatime=on",
        %"-O xattr=sa",
        %"-O mountpoint=/",
        %"rpool",
        rpool_part)?;

    eprintln!("\ncreate root dataset...\n");
    run_result!(%"zfs create -o canmount=off -o mountpoint=none rpool/arch")?;

    eprintln!("\ncreate other dataset...\n");
    run_result!(%"zfs create -o canmount=off -o mountpoint=none bpool/arch")?;
    run_result!(%"zfs create -o canmount=off -o mountpoint=none bpool/arch/BOOT")?;
    run_result!(%"zfs create -o canmount=off -o mountpoint=none rpool/arch/ROOT")?;
    run_result!(%"zfs create -o canmount=off -o mountpoint=none rpool/arch/DATA")?;
    run_result!(%"zfs create -o mountpoint=/boot -o canmount=noauto bpool/arch/BOOT/default")?;
    run_result!(%"zfs create -o mountpoint=/ -o canmount=off    rpool/arch/DATA/default")?;
    run_result!(%"zfs create -o mountpoint=/ -o canmount=noauto rpool/arch/ROOT/default")?;
    run_result!(%"zfs mount rpool/arch/ROOT/default")?;
    run_result!(%"zfs mount bpool/arch/BOOT/default")?;

    for dir in ["usr", "var", "var/lib"] {
        dbg!(dir);
        let d_path = "rpool/arch/DATA/default/".to_owned() + dir;
        run_result!(%"zfs create -o canmount=off", d_path)?;
    }

    for dir in ["home", "root", "srv", "usr/local", "var/log", "var/spool"] {
        dbg!(dir);
        let d_path = "rpool/arch/DATA/default/".to_owned() + dir;
        run_result!(%"zfs create -o canmount=on", d_path)?;
    }
    run_result!(%"chmod 750 /mnt/root")?;

    eprintln!("\nformat and mount esp...\n");
    run_result!(%"mkfs.vfat -n EFI", &efi_part)?;

    let e_path = "/mnt/boot/efis/".to_owned() + sail.get_efi_last_path()?;

    run_result!(%"mkdir -p", &e_path).context("Creating efis dir")?;
    run_result!(%"mount -t vfat", &efi_part, e_path)?;
    run_result!(%"mkdir -p /mnt/boot/efi").context("Creating efi dir")?;
    run_result!(%"mount -t vfat", efi_part, "/mnt/boot/efi")?;

    eprintln!("\nOptional user data datasets...\n");
    run_result!(%"zfs create -o canmount=on rpool/arch/DATA/default/var/games")?;
    run_result!(%"zfs create -o canmount=on rpool/arch/DATA/default/var/www")?;
    eprintln!("\nfor GNOME...\n");
    run_result!(%"zfs create -o canmount=on rpool/arch/DATA/default/var/lib/AccountsService")?;
    eprintln!("\nfor Docker...\n");
    run_result!(%"zfs create -o canmount=on rpool/arch/DATA/default/var/lib/docker")?;
    eprintln!("\nfor NFS...\n");
    run_result!(%"zfs create -o canmount=on rpool/arch/DATA/default/var/lib/nfs")?;
    eprintln!("\nfor LXC...\n");
    run_result!(%"zfs create -o canmount=on rpool/arch/DATA/default/var/lib/lxc")?;
    eprintln!("\nfor LibVirt...\n");
    run_result!(%"zfs create -o canmount=on rpool/arch/DATA/default/var/lib/libvirt")?;

    eprintln!("\nInstall base packages...\n");
    run_result!(%"pacstrap -c /mnt base neovim networkmanager grub efibootmgr mkinitcpio reflector git base-devel sudo zsh")?;
    run_result!(%"pacstrap -c /mnt linux-lts linux-lts-headers")?;
    run_result!(%"pacstrap -c /mnt linux-firmware intel-ucode")?;
    run_result!(%"pacstrap -c /mnt zfs-dkms zfs-utils")?;

    Ok(())
}

fn stage_2(sail: &Sail) -> Result<()> {
    eprintln!("\nset mkinitcpio zfs hook scan path...\n");
    let cmdline = "GRUB_CMDLINE_LINUX=\"zfs_import_dir=/dev/disk/by-id\"";
    let mut grub_default = OpenOptions::new()
        .append(true)
        .open("/mnt/etc/default/grub")?;

    writeln!(grub_default, "{}", cmdline)?;

    eprintln!("\ngenerate fstab...\n");
    let mut fstab = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open("/mnt/etc/fstab")?;

    let StdoutTrimmed(fstab_out) = run_result!(%"genfstab -U /mnt")?;
    let StdoutTrimmed(fstab_out) =
        run_result!(%"sed", "s;zfs[[:space:]]*;zfs zfsutil,;g", Stdin(fstab_out))?;
    let StdoutTrimmed(fstab_out) = run_result!(%"grep", "zfs zfsutil", Stdin(fstab_out))?;

    writeln!(fstab, "{}", fstab_out)?;

    let efi_part = &sail.efi_part;
    let StdoutTrimmed(uuid) = run_result!(%"blkid -s UUID -o value", efi_part)?;
    let fstab_efi_path = "/boot/efis/".to_owned() + sail.get_efi_last_path()?;
    let fstab_efi_path = format!("{} {} {}", uuid, fstab_efi_path, "vfat x-systemd.idle-timeout=1min,x-systemd.automount,noauto,umask=0022,fmask=0022,dmask=0022 0 1");
    let fstab_efi_path2 = format!("{} {}", uuid, "/boot/efi vfat x-systemd.idle-timeout=1min,x-systemd.automount,noauto,umask=0022,fmask=0022,dmask=0022 0 1");

    writeln!(fstab, "{}/n{}", fstab_efi_path, fstab_efi_path2)?;

    eprintln!("\nconfigure mkinitcpio...\n");
    run_result!(%"mv /mnt/etc/mkinitcpio.conf /mnt/etc/mkinitcpio.conf.old")?;

    let mut mkinitcpio = OpenOptions::new()
        .append(true)
        .create(true)
        .open("/mnt/etc/mkinitcpio.conf")?;
    let hooks = "HOOKS=(base udev autodetect modconf block keyboard zfs filesystems)";

    writeln!(mkinitcpio, "{}", hooks)?;

    eprintln!("\nenable internet time sync...\n");
    run_result!(%"hwclock --systohc")?;
    run_result!(%"systemctl enable systemd-timesyncd --root=/mnt")?;

    eprintln!("\nset locale, timezone, keymap...\n");
    run_result!(%"rm -f /mnt/etc/localtime")?;
    run_result!(%"systemd-firstboot --root=/mnt --force --locale=en_US.UTF-8 --locale-messages=en_US.UTF-8 --keymap=us --timezone=Asia/Jakarta --hostname=lbox --root-password=12345678 --root-shell=/bin/bash")?;
    eprintln!("\nbecause of bug, you must change root password using chroot...\n");
    run_result!(%"arch-chroot /mnt passwd", Stdin("12345678\n12345678"))?;

    eprintln!("\ngenerate hostid...\n");
    run_result!(%"zgenhostid -f -o /mnt/etc/hostid")?;

    eprintln!("\nenable zfs services...\n");
    run_result!(%"systemctl enable zfs-import-scan.service zfs-import.target zfs-zed zfs.target --root=/mnt")?;
    run_result!(%"systemctl disable zfs-mount --root=/mnt")?;

    eprintln!("\napply locales...\n");
    let locales = "en_US.UTF-8 UTF-8";
    let mut locale_file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open("/mnt/etc/locale.gen")?;

    writeln!(locale_file, "{}", locales)?;

    run_result!(%"arch-chroot /mnt bash --login", Stdin("locale-gen"))?;

    eprintln!("\nimport keys of archzfs...\n");
    let StdoutTrimmed(archzfs_gpg) =
        run_result!(%"curl -L https://mirror.sum7.eu/archlinux/archzfs/archzfs.gpg")?;
    run_result!(%"arch-chroot /mnt pacman-key -a -", Stdin(archzfs_gpg))?;

    let StdoutTrimmed(sign_key) = run_result!(%"curl -L https://git.io/JsfVS")?;
    run_result!(%"arch-chroot /mnt pacman-key --lsign-key", sign_key)?;

    let StdoutTrimmed(mirrorlist) = run_result!(%"curl -L https://git.io/Jsfw2")?;
    let mut mirrorlist_archzfs = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open("/mnt/etc/pacman.d/mirrorlist-archzfs")?;
    writeln!(mirrorlist_archzfs, "{}", mirrorlist)?;

    eprintln!("\nadd archzfs repo...\n");
    let mut pacman_conf = OpenOptions::new()
        .append(true)
        .create(true)
        .open("/mnt/etc/pacman.conf")?;
    let archzfs_repo = "\n\
        #[archzfs-testing]\n\
        #Include = /etc/pacman.d/mirrorlist-archzfs\n\
        \n\
        [archzfs]\n\
        Include = /etc/pacman.d/mirrorlist-archzfs";

    writeln!(pacman_conf, "{}", archzfs_repo)?;

    Ok(())
}

fn stage_3() -> Result<()> {
    eprintln!("\ninstall paru...\n");
    let paru_inst = "\n\
        echo 'nobody ALL=(ALL) NOPASSWD: ALL' > /etc/sudoers.d/00_nobody\n\
        su - nobody -s /bin/bash\n\
        mkdir /tmp/build\n\
        cd /tmp/build\n\
        git clone https://aur.archlinux.org/paru-bin.git\n\
        cd paru-bin\n\
        makepkg -si\n\
        Y\n\
        ";
    run_result!(%"arch-chroot /mnt bash --login", Stdin(paru_inst))?;

    eprintln!("\ninstall boot environment manager...\n");
    let bieaz_inst = "\n\
        su - nobody -s /bin/bash\n\
        mkdir /tmp/build\n\
        cd /tmp/build\n\
        git clone https://aur.archlinux.org/bieaz.git\n\
        cd bieaz\n\
        makepkg -si\n\
        Y\n\
        ";
    let bem_inst = "\n\
        su - nobody -s /bin/bash\n\
        mkdir /tmp/build\n\
        cd /tmp/build\n\
        git clone https://aur.archlinux.org/rozb3-pac.git\n\
        cd rozb3-pac\n\
        makepkg -si\n\
        Y\n\
        ";
    run_result!(%"arch-chroot /mnt bash --login", Stdin(bieaz_inst))?;
    run_result!(%"arch-chroot /mnt bash --login", Stdin(bem_inst))?;

    eprintln!("\ndelete temporary user...\n");
    run_result!(%"rm /mnt/etc/sudoers.d/00_nobody")?;

    Ok(())
}

fn stage_4() -> Result<()> {
    eprintln!("\ngrub canonical path fix...\n");
    let canonical_fix = "export ZPOOL_VDEV_NAME_PATH=YES";
    let mut zpool_vdev = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open("/mnt/etc/profile.d/zpool_vdev_name_path.sh")?;
    let env_keep = r#"Defaults env_keep += "ZPOOL_VDEV_NAME_PATH""#;
    let mut sudoers = OpenOptions::new()
        .append(true)
        .create(true)
        .open("/mnt/etc/sudoers")?;

    writeln!(zpool_vdev, "{}", canonical_fix)?;
    writeln!(sudoers, "{}", env_keep)?;

    eprintln!("\npool name missing...\n");
    let exp =
        r"s/rpool=.*/rpool=`zdb -l ${GRUB_DEVICE} | grep -E '[[:blank:]]name' | cut -d\\' -f 2`/";

    run_result!(%"sed -i", exp, "/mnt/etc/grub.d/10_linux")?;

    Ok(())
}

fn stage_5(sail: &Sail) -> Result<()> {
    eprintln!("\ngenerate initrd...\n");
    let cmd = "\n\
        rm -f /etc/zfs/zpool.cache\n\
        touch /etc/zfs/zpool.cache\n\
        chmod a-w /etc/zfs/zpool.cache\n\
        chattr +i /etc/zfs/zpool.cache\n\
        mkinitcpio -P\n\
        ";
    run_result!(%"arch-chroot /mnt bash --login", Stdin(cmd))?;

    eprintln!("\ncreate grub boot dir, in esp and boot pool...\n");
    run_result!(%"mkdir -p /mnt/boot/efi/EFI/arch")?;
    run_result!(%"mkdir -p /mnt/boot/grub")?;

    // eprintln!("\ninstall grub bios...\n");
    // let cmd = "grub-install --boot-directory /boot/efi/EFI/arch --target=i386-pc ".to_owned() + &sail.disk;

    // run_result!(%"arch-chroot /mnt bash --login", Stdin(cmd))?;

    eprintln!("\ninstall grub efi...\n");
    let cmd = "\n\
        grub-install --boot-directory /boot/efi/EFI/arch --efi-directory /boot/efi/\n\
        grub-install --boot-directory /boot/efi/EFI/arch --efi-directory /boot/efi/ --removable\n\
        ";
    let cmd = cmd.to_owned()
        + r"efibootmgr -cgp 1 -l \EFI\arch\grubx64.efi -L "
        + "arch-"
        + sail.get_disk_last_path()?
        + " -d "
        + &sail.disk
        + "\n";

    run_result!(%"arch-chroot /mnt bash --login", Stdin(cmd))?;

    eprintln!("\ngenerate grub menu...\n");
    let cmd = "\n\
        grub-mkconfig -o /boot/efi/EFI/arch/grub/grub.cfg\n\
        cp /boot/efi/EFI/arch/grub/grub.cfg /boot/grub/grub.cfg\n\
        ";

    run_result!(%"arch-chroot /mnt bash --login", Stdin(cmd))?;

    eprintln!("\nmirror esp content...\n");
    let cmd = "\n\
        ESP_MIRROR=$(mktemp -d)\n\
        cp -r /boot/efi/EFI $ESP_MIRROR\n\
        for i in /boot/efis/*; do\n\
          cp -r $ESP_MIRROR/EFI $i\n\
        done\n\
        ";

    run_result!(%"arch-chroot /mnt bash --login", Stdin(cmd))?;

    Ok(())
}

fn stage_6() -> Result<()> {
    eprintln!("\nsnapshot of clean installation...\n");
    run_result!(%"zfs snapshot -r rpool/arch@install")?;
    run_result!(%"zfs snapshot -r bpool/arch@install")?;

    eprintln!("\nunmount efi partition...\n");
    run_result!(%"umount /mnt/boot/efi")?;
    run_result!(%"bash --login", Stdin("umount /mnt/boot/efis/*\n"))?;

    eprintln!("\nexport pools...\n");
    run_result!(%"zpool export bpool")?;
    run_result!(%"zpool export rpool")?;

    Ok(())
}

fn main() -> Result<()> {
    let sail = Sail::new(
        "arch",
        "linux-lts",
        "/dev/disk/by-path/virtio-pci-0000:04:00.0",
        "1G",
        "4G",
    );

    command_checker()?;
    check_as_root()?;
    // stage_1(&sail)?;
    // stage_2(&sail)?;
    // stage_3()?;
    // stage_4()?;
    // stage_5(&sail)?;
    // stage_6()?;

    Ok(())
}
