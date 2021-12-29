mod sail;

use crate::sail::Sail;
use anyhow::{bail, Context, Result};
use cradle::input::{Split, Stdin};
use cradle::output::StdoutTrimmed;
use cradle::prelude::*;
use sail::{LinuxVariant, ZfsType, StorageType};
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::{fs::OpenOptions, thread, time};

fn command_checker() -> Result<()> {
    let commands = [
        "arch-chroot",
        "awk",
        "bash",
        "blkid",
        "chmod",
        "curl",
        "genfstab",
        "grep",
        "hwclock",
        "mkdir",
        "mkfs.vfat",
        "modprobe",
        "mount",
        "mv",
        "pacman",
        "pacstrap",
        "rm",
        "sed",
        "sgdisk",
        "systemctl",
        "systemd-firstboot",
        "umount",
        "which",
        "zfs",
        "zgenhostid",
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

fn openopt_write<P>(path: P) -> Result<File>
where
    P: AsRef<Path>,
{
    let openopt = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)?;
    
    Ok(openopt)
}

fn partition_disk(sail: &Sail) -> Result<()> {
    let duration = time::Duration::from_secs(1);

    let disk = &sail.get_disk();
    let partsize_esp = &sail.get_partsize_esp();
    let partsize_bpool = &sail.get_partsize_bpool();

    eprintln!("\nFind last partition number...\n");
    let mut next_partnum = sail.get_next_partnum();

    eprintln!("\nCreate efi partition...\n");
    let efi_partnum = next_partnum.to_string();
    let part_desc = format!("-n{}:0:+{}", efi_partnum, partsize_esp);
    let part_type = format!("-t{}:EF00", efi_partnum);

    run_result!("sgdisk", part_desc, part_type, disk)?;

    eprintln!("\nCreate bpool partition...\n");
    next_partnum += 1;
    let bpool_partnum = next_partnum.to_string();
    let part_desc = format!("-n{}:0:+{}", bpool_partnum, partsize_bpool);
    let part_type = format!("-t{}:BE00", bpool_partnum);

    run_result!(%"sgdisk", part_desc, part_type, disk)?;

    eprintln!("\nCreate rpool partition...\n");
    next_partnum += 1;
    let rpool_partnum = next_partnum.to_string();
    let part_desc = format!("-n{}:0:0", rpool_partnum);
    let part_type = format!("-t{}:BF00", rpool_partnum);

    run_result!(%"sgdisk", part_desc, part_type, disk)?;

    thread::sleep(duration);

    Ok(())
}

fn format_disk(sail: &Sail) -> Result<()> {
    let efi_part = &sail.get_efi_part()?;
    let bpool_part = &sail.get_bpool_part()?;
    let rpool_part = &sail.get_rpool_part()?;

    eprintln!("\nLoad zfs kernel module...\n");
    run_result!(%"modprobe zfs")?;

    eprintln!("\nCreate boot pool...\n");
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

    eprintln!("\nCreate root pool...\n");
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

    eprintln!("\nCreate root dataset...\n");
    run_result!(%"zfs create -o canmount=off -o mountpoint=none rpool/arch")?;

    eprintln!("\nCreate other dataset...\n");
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
        eprintln!("{}", dir);
        let d_path = "rpool/arch/DATA/default/".to_owned() + dir;
        run_result!(%"zfs create -o canmount=off", d_path)?;
    }

    for dir in ["home", "root", "srv", "usr/local", "var/log", "var/spool"] {
        eprintln!("{}", dir);
        let d_path = "rpool/arch/DATA/default/".to_owned() + dir;
        run_result!(%"zfs create -o canmount=on", d_path)?;
    }
    run_result!(%"chmod 750 /mnt/root")?;

    eprintln!("\nFormat and mount esp...\n");
    run_result!(%"mkfs.vfat -n EFI", &efi_part)?;

    let e_path = format!("/mnt/boot/efis/{}", sail.get_efi_last_path()?);

    run_result!(%"mkdir -p", &e_path).context("Creating efis dir")?;
    run_result!(%"mount -t vfat", &efi_part, e_path)?;
    run_result!(%"mkdir -p /mnt/boot/efi").context("Creating efi dir")?;
    run_result!(%"mount -t vfat", efi_part, "/mnt/boot/efi")?;

    eprintln!("\nOptional user data datasets...\n");
    run_result!(%"zfs create -o canmount=on rpool/arch/DATA/default/var/games")?;
    run_result!(%"zfs create -o canmount=on rpool/arch/DATA/default/var/www")?;
    eprintln!("\nFor GNOME...\n");
    run_result!(%"zfs create -o canmount=on rpool/arch/DATA/default/var/lib/AccountsService")?;
    eprintln!("\nFor Docker...\n");
    run_result!(%"zfs create -o canmount=on rpool/arch/DATA/default/var/lib/docker")?;
    eprintln!("\nFor NFS...\n");
    run_result!(%"zfs create -o canmount=on rpool/arch/DATA/default/var/lib/nfs")?;
    eprintln!("\nFor LXC...\n");
    run_result!(%"zfs create -o canmount=on rpool/arch/DATA/default/var/lib/lxc")?;
    eprintln!("\nFor LibVirt...\n");
    run_result!(%"zfs create -o canmount=on rpool/arch/DATA/default/var/lib/libvirt")?;

    Ok(())
}

fn pacstrap(sail: &Sail) -> Result<()> {
    let base = [
        "base",
        "base-devel",
        "dosfstools",
        "efibootmgr",
        "grub",
        "git",
        "htop",
        "mandoc",
        "mkinitcpio",
        "neovim",
        "networkmanager",
        "reflector",
        "sudo",
        "zsh",
    ];
    let linux = sail.get_linvar();
    let linux_header = linux.to_owned() + "-headers";
    let zfs = sail.get_zfs();

    eprintln!("\nUpdate pacman repository...\n");
    run_result!(%"pacman -Sy")?;

    eprintln!("\nCheck compatible kernel version...\n");
    let StdoutTrimmed(out) = run_result!(%"pacman -Si", zfs)?;
    let StdoutTrimmed(out) = run_result!("grep", "Depends On", Stdin(out))?;
    let exp = format!("s|.*{}=||", linux);
    let StdoutTrimmed(out) = run_result!("sed", exp, Stdin(out))?;
    let StdoutTrimmed(linver) = run_result!("awk", "{ print $1 }", Stdin(out))?;

    eprintln!("\nCheck repo kernel version...\n");
    let StdoutTrimmed(out) = run_result!(%"pacman -Si", linux)?;
    let StdoutTrimmed(out) = run_result!(%"grep Version", Stdin(out))?;
    let StdoutTrimmed(repo_linver) = run_result!("awk", "{ print $3 }", Stdin(out))?;

    eprintln!("\nInstall base packages...\n");
    run_result!(%"pacstrap -c /mnt", base)?;

    eprintln!("\nInstall kernel, download from archive if not available...\n");
    if linver == repo_linver {
        eprintln!("Install from repo...\n");
        run_result!(%"pacstrap -c /mnt", linux, linux_header)?;
    } else {
        let url = format!(
            "https://archive.archlinux.org/packages/l/{linux}/{linux}-{linver}-x86_64.pkg.tar.zst",
            linux = linux,
            linver = linver
        );
        eprintln!("Install manually from {}\n", url);
        run_result!(%"pacstrap -U /mnt", url)?;
        run_result!(%"pacstrap -c /mnt", linux_header)?;
    }

    eprintln!("\nInstall firmware...\n");
    run_result!(%"pacstrap -c /mnt linux-firmware intel-ucode amd-ucode")?;

    eprintln!("\nInstall zfs...\n");
    run_result!(%"pacstrap -c /mnt", zfs, "zfs-utils")?;

    Ok(())
}

fn system_configuration(sail: &Sail) -> Result<()> {
    eprintln!("\nSet mkinitcpio zfs hook scan path...\n");
    let cmdline = format!(
        r#"{}GRUB_CMDLINE_LINUX="zfs_import_dir={}""#,
        "GRUB_DISABLE_OS_PROBER=false\n",
        sail.get_disk_parent()?
    );
    let mut grub_default = OpenOptions::new()
        .append(true)
        .open("/mnt/etc/default/grub")?;

    writeln!(grub_default, "{}", cmdline)?;

    eprintln!("\nGenerate fstab...\n");
    let mut fstab = openopt_write("/mnt/etc/fstab")?;

    let StdoutTrimmed(fstab_out) = run_result!(%"genfstab -U /mnt")?;
    let StdoutTrimmed(fstab_out) =
        run_result!(%"sed", "s;zfs[[:space:]]*;zfs zfsutil,;g", Stdin(fstab_out))?;
    let StdoutTrimmed(fstab_out) = run_result!(%"grep", "zfs zfsutil", Stdin(fstab_out))?;

    writeln!(fstab, "{}", fstab_out)?;

    let efi_part = &sail.get_efi_part()?;
    let StdoutTrimmed(uuid) = run_result!(%"blkid -s UUID -o value", efi_part)?;
    let fstab_efi_path = format!("/boot/efis/{}", sail.get_efi_last_path()?);
    let fstab_efi_path = format!("UUID={} {} {}", uuid, fstab_efi_path, "vfat x-systemd.idle-timeout=1min,x-systemd.automount,noauto,umask=0022,fmask=0022,dmask=0022 0 1");
    let fstab_efi_path2 = format!("UUID={} {}", uuid, "/boot/efi vfat x-systemd.idle-timeout=1min,x-systemd.automount,noauto,umask=0022,fmask=0022,dmask=0022 0 1");

    writeln!(fstab, "{}\n{}", fstab_efi_path, fstab_efi_path2)?;

    eprintln!("\nConfigure mkinitcpio...\n");
    run_result!(%"mv /mnt/etc/mkinitcpio.conf /mnt/etc/mkinitcpio.conf.old")?;

    let mut mkinitcpio = OpenOptions::new()
        .append(true)
        .create(true)
        .open("/mnt/etc/mkinitcpio.conf")?;
    let hooks = "HOOKS=(base udev autodetect modconf block keyboard zfs filesystems)";

    writeln!(mkinitcpio, "{}", hooks)?;

    eprintln!("\nEnable internet time sync...\n");
    run_result!(%"hwclock --systohc")?;
    run_result!(%"systemctl enable systemd-timesyncd --root=/mnt")?;

    eprintln!("\nSet locale, timezone, keymap...\n");
    run_result!(%"rm -f /mnt/etc/localtime")?;
    run_result!(%"systemd-firstboot --root=/mnt --force --locale=en_US.UTF-8 --locale-messages=en_US.UTF-8 --keymap=us --timezone=Asia/Jakarta --hostname=lbox --root-password=12345678 --root-shell=/bin/bash")?;
    eprintln!("\nChange root password using chroot...\n");
    run_result!(%"arch-chroot /mnt passwd", Stdin("12345678\n12345678"))?;

    eprintln!("\nGenerate hostid...\n");
    run_result!(%"zgenhostid -f -o /mnt/etc/hostid")?;

    eprintln!("\nIgnore kernel update...\n");
    run_result!(%"sed -i", "s/#IgnorePkg/IgnorePkg/", "/mnt/etc/pacman.conf")?;
    let exp = format!(
        "/^IgnorePkg/ s/$/ {linux} {linux}-headers zfs-{linux} zfs-utils/",
        linux = sail.get_linvar()
    );
    run_result!(%"sed -i", exp, "/mnt/etc/pacman.conf")?;

    eprintln!("\nGenerate kernel_updater script in /usr/local/bin...\n");
    let script = r"
#!/bin/bash

INST_LINVAR=$(sed 's|.*linux|linux|' /proc/cmdline | sed 's|.img||g' | awk '{ print $1 }')
pacman -Sy --needed --noconfirm ${INST_LINVAR} ${INST_LINVAR}-headers zfs-${INST_LINVAR} zfs-utils
";
    let mut script_file = OpenOptions::new()
        .write(true)
        .create(true)
        .open("/mnt/usr/local/bin/kernel_updater")?;
    writeln!(script_file, "{}", script)?;
    run_result!(%"chmod +x /mnt/usr/local/bin/kernel_updater")?;

    eprintln!("\nEnable zfs services...\n");
    run_result!(%"systemctl enable zfs-import-scan.service zfs-import.target zfs-zed zfs.target --root=/mnt")?;
    run_result!(%"systemctl disable zfs-mount --root=/mnt")?;

    eprintln!("\nApply locales...\n");
    let locales = "en_US.UTF-8 UTF-8";
    let mut locale_file = openopt_write("/mnt/etc/locale.gen")?;

    writeln!(locale_file, "{}", locales)?;

    run_result!(%"arch-chroot /mnt bash --login", Stdin("locale-gen"))?;

    eprintln!("\nImport keys of archzfs...\n");
    let StdoutTrimmed(archzfs_gpg) =
        run_result!(%"curl -L https://mirror.sum7.eu/archlinux/archzfs/archzfs.gpg")?;
    run_result!(%"arch-chroot /mnt pacman-key -a -", Stdin(archzfs_gpg))?;

    let StdoutTrimmed(sign_key) = run_result!(%"curl -L https://git.io/JsfVS")?;
    run_result!(%"arch-chroot /mnt pacman-key --lsign-key", sign_key)?;

    let StdoutTrimmed(mirrorlist) = run_result!(%"curl -L https://git.io/Jsfw2")?;
    let mut mirrorlist_archzfs = openopt_write("/mnt/etc/pacman.d/mirrorlist-archzfs")?;
    writeln!(mirrorlist_archzfs, "{}", mirrorlist)?;

    eprintln!("\nAdd archzfs repo...\n");
    let mut pacman_conf = OpenOptions::new()
        .append(true)
        .create(true)
        .open("/mnt/etc/pacman.conf")?;
    let archzfs_repo = r"
#[archzfs-testing]
#Include = /etc/pacman.d/mirrorlist-archzfs

[archzfs]
Include = /etc/pacman.d/mirrorlist-archzfs
";

    writeln!(pacman_conf, "{}", archzfs_repo)?;

    Ok(())
}

fn install_aurs() -> Result<()> {
    let arch_chroot = Split("arch-chroot /mnt bash --login");

    eprintln!("\nInstall paru...\n");
    let paru_install = r"
echo 'nobody ALL=(ALL) NOPASSWD: ALL' > /etc/sudoers.d/00_nobody
su - nobody -s /bin/bash
mkdir /tmp/build
cd /tmp/build
git clone https://aur.archlinux.org/paru-bin.git
cd paru-bin
makepkg -si
Y
";
    run_result!(&arch_chroot, Stdin(paru_install))?;

    eprintln!("\nInstall boot environment manager...\n");
    let bieaz_install = r"
su - nobody -s /bin/bash
mkdir /tmp/build
cd /tmp/build
git clone https://aur.archlinux.org/bieaz.git
cd bieaz
makepkg -si
Y
";
    let bem_install = r"
su - nobody -s /bin/bash
mkdir /tmp/build
cd /tmp/build
git clone https://aur.archlinux.org/rozb3-pac.git
cd rozb3-pac
makepkg -si
Y
";
    run_result!(&arch_chroot, Stdin(bieaz_install))?;
    run_result!(&arch_chroot, Stdin(bem_install))?;

    eprintln!("\nInstall zrepl auto snapshot...\n");
    let zrepl_install = r"
su - nobody -s /bin/bash
mkdir /tmp/build
cd /tmp/build
git clone https://aur.archlinux.org/zrepl-bin.git
cd zrepl-bin
makepkg -si
Y
";
    run_result!(&arch_chroot, Stdin(zrepl_install))?;

    eprintln!("\nGenerate zrepl configuration...\n");
    run_result!(%"mkdir -p /mnt/etc/zrepl")?;
    let mut zrepl_conf_path = openopt_write("/mnt/etc/zrepl/zrepl.yml")?;
    let conf = r#"
jobs:

- name: snapjob
  type: snap
  filesystems: {
    "bpool/arch/BOOT": true,
    "bpool/arch/BOOT/default": true,
    "rpool/arch/DATA<": true,
    "rpool/arch/ROOT": true,
    "rpool/arch/ROOT/default": true,
  }
  snapshotting:
    type: periodic
    interval: 15m
    prefix: zrepl_
  pruning:
    keep:
    - type: grid
      grid: 1x1h(keep=all) | 12x1h | 7x1d
      regex: "^zrepl_.*"
    - type: regex
      negate: true
      regex: "^zrepl_.*"
"#;
    writeln!(zrepl_conf_path, "{}", conf)?;

    eprintln!("\nDelete temporary user...\n");
    run_result!(%"rm /mnt/etc/sudoers.d/00_nobody")?;

    Ok(())
}

fn workarounds() -> Result<()> {
    eprintln!("\nGrub canonical path fix...\n");
    let canonical_fix = "export ZPOOL_VDEV_NAME_PATH=YES";
    let mut zpool_vdev = openopt_write("/mnt/etc/profile.d/zpool_vdev_name_path.sh")?;
    let env_keep = r#"Defaults env_keep += "ZPOOL_VDEV_NAME_PATH""#;
    let mut sudoers = OpenOptions::new()
        .append(true)
        .create(true)
        .open("/mnt/etc/sudoers")?;

    writeln!(zpool_vdev, "{}", canonical_fix)?;
    writeln!(sudoers, "{}", env_keep)?;

    eprintln!("\nPool name missing fix...\n");
    let exp =
        r"s/rpool=.*/rpool=`zdb -l ${GRUB_DEVICE} | grep -E '[[:blank:]]name' | cut -d\\' -f 2`/";

    run_result!(%"sed -i", exp, "/mnt/etc/grub.d/10_linux")?;

    Ok(())
}

fn bootloaders(sail: &Sail) -> Result<()> {
    let arch_chroot = Split("arch-chroot /mnt bash --login");

    eprintln!("\nGenerate initrd...\n");
    let cmd = r"
rm -f /etc/zfs/zpool.cache
touch /etc/zfs/zpool.cache
chmod a-w /etc/zfs/zpool.cache
chattr +i /etc/zfs/zpool.cache
mkinitcpio -P
";
    run_result!(&arch_chroot, Stdin(cmd))?;

    eprintln!("\nCreate grub boot dir, in esp and boot pool...\n");
    run_result!(%"mkdir -p /mnt/boot/efi/EFI/arch")?;
    run_result!(%"mkdir -p /mnt/boot/grub")?;

    eprintln!("\nInstall grub efi...\n");
    let cmd = format!(
        r#"
grub-install --boot-directory /boot/efi/EFI/arch --efi-directory /boot/efi/
grub-install --boot-directory /boot/efi/EFI/arch --efi-directory /boot/efi/ --removable
efibootmgr -cgp 1 -l "\EFI\arch\grubx64.efi" -L "arch-{}" -d {}
"#,
        sail.get_disk_last_path()?,
        sail.get_disk()
    );

    run_result!(&arch_chroot, Stdin(cmd))?;

    eprintln!("\nGenerate grub menu...\n");
    let cmd = r"
grub-mkconfig -o /boot/efi/EFI/arch/grub/grub.cfg
cp /boot/efi/EFI/arch/grub/grub.cfg /boot/grub/grub.cfg
";

    run_result!(&arch_chroot, Stdin(cmd))?;

    eprintln!("\nMirror esp content...\n");
    let cmd = r"
ESP_MIRROR=$(mktemp -d)
cp -r /boot/efi/EFI $ESP_MIRROR
for i in /boot/efis/*; do
  cp -r $ESP_MIRROR/EFI $i
done
";

    run_result!(&arch_chroot, Stdin(cmd))?;

    Ok(())
}

fn finishing(sail: &Sail) -> Result<()> {
    let duration = time::Duration::from_secs(1);

    eprintln!("\nGenerate monthly scrub service...\n");
    let mut scrub_timer_path = openopt_write("/mnt/etc/systemd/system/zfs-scrub@.timer")?;
    let mut scrub_service_path = openopt_write("/mnt/etc/systemd/system/zfs-scrub@.service")?;
    let scrub_timer = r"
[Unit]
Description=Monthly zpool scrub on %i

[Timer]
OnCalendar=monthly
AccuracySec=1h
Persistent=true

[Install]
WantedBy=multi-user.target
";
    let scrub_service = r"
[Unit]
Description=zpool scrub on %i

[Service]
Nice=19
IOSchedulingClass=idle
KillSignal=SIGINT
ExecStart=/usr/bin/zpool scrub %i

[Install]
WantedBy=multi-user.target
";
    writeln!(scrub_timer_path, "{}", scrub_timer)?;
    writeln!(scrub_service_path, "{}", scrub_service)?;

    if sail.is_using_ssd() {
        eprintln!("\nGenerate monthly trim service...\n");
        let mut trim_timer_path = openopt_write("/mnt/etc/systemd/system/zfs-trim@.timer")?;
        let mut trim_service_path = openopt_write("/mnt/etc/systemd/system/zfs-trim@.service")?;
        let trim_timer = r"
[Unit]
Description=Monthly zpool trim on %i

[Timer]
OnCalendar=monthly
AccuracySec=1h
Persistent=true

[Install]
WantedBy=multi-user.target
";
        let trim_service = r"
[Unit]
Description=zpool trim on %i

[Service]
Nice=19
IOSchedulingClass=idle
KillSignal=SIGINT
ExecStart=/usr/bin/zpool trim %i

[Install]
WantedBy=multi-user.target
";

        writeln!(trim_timer_path, "{}", trim_timer)?;
        writeln!(trim_service_path, "{}", trim_service)?;
    }

    eprintln!("\nEnable systemd services...\n");
    let arch_chroot = Split("arch-chroot /mnt bash --login");
    let nm_enable = r"
systemctl enable NetworkManager
systemctl enable zrepl
systemctl enable zfs-scrub@rpool.timer
systemctl enable zfs-scrub@bpool.timer
";
    run_result!(&arch_chroot, Stdin(nm_enable))?;

    eprintln!("\nAdd wheel to sudoers...\n");
    let wheel_sudo = "%wheel ALL=(ALL) ALL";
    let mut sudoers = OpenOptions::new()
        .append(true)
        .create(true)
        .open("/mnt/etc/sudoers")?;

    writeln!(sudoers, "{}", wheel_sudo)?;

    let post_scripts_path = "/mnt/root/post_install_scripts";
    eprintln!("\nGenerating script post installation...\n");
    run_result!(%"mkdir -p", post_scripts_path)?;

    let mut data_pools_path = openopt_write([post_scripts_path, "/addt_data_pools.sh"].concat())?;
    let data_pools = r#"
DATA_POOL='tank0 tank1'

# tab-separated zfs properties
# see /etc/zfs/zed.d/history_event-zfs-list-cacher.sh
export \
PROPS="name,mountpoint,canmount,atime,relatime,devices,exec\
,readonly,setuid,nbmand,encroot,keylocation"

for i in $DATA_POOL; do
  zfs list -H -t filesystem -o $PROPS -r $i > /etc/zfs/zfs-list.cache/$i
done
"#;
    writeln!(data_pools_path, "{}", data_pools)?;

    let mut add_user_path = openopt_write([post_scripts_path, "/add_user.sh"].concat())?;
    let add_user = r"
myUser=UserName
useradd -m -G wheel -s /bin/zsh ${myUser}
passwd ${myUser}
";
    writeln!(add_user_path, "{}", add_user)?;

    thread::sleep(duration);

    eprintln!("\nSnapshot of clean installation...\n");
    run_result!(%"zfs snapshot -r rpool/arch@install")?;
    run_result!(%"zfs snapshot -r bpool/arch@install")?;

    eprintln!("\nUnmount efi partition...\n");
    run_result!(%"umount /mnt/boot/efi")?;
    run_result!(%"bash --login", Stdin("umount /mnt/boot/efis/*\n"))?;

    eprintln!("\nExport pools...\n");
    run_result!(%"zpool export bpool")?;
    thread::sleep(duration);
    run_result!(%"zpool export rpool")?;

    Ok(())
}

fn main() -> Result<()> {
    let sail = Sail::new(
        LinuxVariant::LinuxLts,
        ZfsType::Normal,
        StorageType::Ssd,
        "/dev/disk/by-path/virtio-pci-0000:04:00.0",
        "1G",
        "4G",
    )?;

    command_checker()?;
    check_as_root()?;
    partition_disk(&sail)?;
    format_disk(&sail)?;
    pacstrap(&sail)?;
    system_configuration(&sail)?;
    install_aurs()?;
    workarounds()?;
    bootloaders(&sail)?;
    finishing(&sail)?;

    // TODO: check if using hdd or ssd
    // TODO: schedule scrub and trim using timer or cron
    Ok(())
}
