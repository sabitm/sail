use crate::{sail::Sail, string_res};
use anyhow::{bail, Context, Result};
use cradle::{
    input::{Split, Stdin},
    output::StdoutTrimmed,
    run_output, run_result,
};
use std::{
    fs::{File, OpenOptions},
    io::Write,
    path::Path,
    thread, time,
};

pub fn command_checker() -> Result<()> {
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

pub fn check_as_root() -> Result<()> {
    let StdoutTrimmed(uid) = run_output!(%"id -u");

    if uid != "0" {
        bail!("Must be run as root!");
    }

    Ok(())
}

fn openopt_a<P>(path: P) -> Result<File>
where
    P: AsRef<Path>,
{
    let openopt = OpenOptions::new().append(true).create(true).open(path)?;
    Ok(openopt)
}

fn openopt_w<P>(path: P) -> Result<File>
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

pub fn partition_disk(sail: &Sail) -> Result<()> {
    let some_delay = time::Duration::from_secs(1);

    let disk = sail.get_disk();
    let partsize_esp = sail.get_partsize_esp();
    let partsize_bpool = sail.get_partsize_bpool();

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

    thread::sleep(some_delay);
    Ok(())
}

pub fn format_disk(sail: &Sail) -> Result<()> {
    let efi_part = sail.get_efi_part()?;
    let bpool_part = sail.get_bpool_part()?;
    let rpool_part = sail.get_rpool_part()?;

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
        let dset = "rpool/arch/DATA/default/".to_owned() + dir;
        run_result!(%"zfs create -o canmount=off", dset)?;
    }

    for dir in ["home", "root", "srv", "usr/local", "var/log", "var/spool"] {
        eprintln!("{}", dir);
        let dset = "rpool/arch/DATA/default/".to_owned() + dir;
        run_result!(%"zfs create -o canmount=on", dset)?;
    }
    run_result!(%"chmod 750 /mnt/root")?;

    eprintln!("\nFormat and mount esp...\n");
    run_result!(%"mkfs.vfat -n EFI", &efi_part)?;

    let efis_mnt = format!("/mnt/boot/efis/{}", sail.get_efi_last_path()?);

    run_result!(%"mkdir -p", &efis_mnt).context("Creating efis dir")?;
    run_result!(%"mount -t vfat", &efi_part, efis_mnt)?;
    run_result!(%"mkdir -p /mnt/boot/efi").context("Creating efi dir")?;
    run_result!(%"mount -t vfat", efi_part, "/mnt/boot/efi")?;

    eprintln!("\nOptional user data datasets...\n");
    run_result!(%"zfs create -o canmount=on rpool/arch/DATA/default/var/games")?;
    run_result!(%"chmod 775 /mnt/var/games")?;
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

pub fn pacstrap(sail: &Sail) -> Result<()> {
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
    let linux_headers = linux.to_owned() + "-headers";
    let zfs = sail.get_zfs();

    eprintln!("\nUpdate pacman repository...\n");
    run_result!(%"pacman -Sy")?;

    eprintln!("\nCheck compatible kernel version...\n");
    let StdoutTrimmed(out) = run_result!(%"pacman -Si", zfs)?;
    let StdoutTrimmed(out) = run_result!("grep", "Depends On", Stdin(out))?;
    let exp = format!("s|.*{}=||", linux);
    let StdoutTrimmed(out) = run_result!("sed", exp, Stdin(out))?;
    let StdoutTrimmed(req_linver) = run_result!("awk", "{ print $1 }", Stdin(out))?;

    eprintln!("\nCheck repo kernel version...\n");
    let StdoutTrimmed(out) = run_result!(%"pacman -Si", linux)?;
    let StdoutTrimmed(out) = run_result!(%"grep Version", Stdin(out))?;
    let StdoutTrimmed(repo_linver) = run_result!("awk", "{ print $3 }", Stdin(out))?;

    eprintln!("\nInstall base packages...\n");
    run_result!(%"pacstrap -c /mnt", base)?;

    eprintln!("\nInstall kernel, download from archive if not available...\n");
    if req_linver == repo_linver {
        eprintln!("Install from repo...\n");
        run_result!(%"pacstrap -c /mnt", linux, linux_headers)?;
    } else {
        let url = format!(
            "https://archive.archlinux.org/packages/l/{linux}/{linux}-{linver}-x86_64.pkg.tar.zst",
            linux = linux,
            linver = req_linver
        );
        eprintln!("Install manually from {}\n", url);
        run_result!(%"pacstrap -U /mnt", url)?;
        run_result!(%"pacstrap -c /mnt", linux_headers)?;
    }

    eprintln!("\nInstall firmware...\n");
    run_result!(%"pacstrap -c /mnt linux-firmware intel-ucode amd-ucode")?;

    eprintln!("\nInstall zfs...\n");
    run_result!(%"pacstrap -c /mnt", zfs, "zfs-utils")?;

    Ok(())
}

pub fn system_configuration(sail: &Sail) -> Result<()> {
    eprintln!("\nSet mkinitcpio zfs hook scan path...\n");
    let mut grub_default_p = openopt_a("/mnt/etc/default/grub")?;
    let grub_cmdline_c = format!(
        r#"{}GRUB_CMDLINE_LINUX="zfs_import_dir={}""#,
        "GRUB_DISABLE_OS_PROBER=false\n",
        sail.get_disk_parent()?
    );
    writeln!(grub_default_p, "{}", grub_cmdline_c)?;

    eprintln!("\nGenerate fstab...\n");
    let mut fstab_p = openopt_w("/mnt/etc/fstab")?;
    let StdoutTrimmed(out) = run_result!(%"genfstab -U /mnt")?;
    let StdoutTrimmed(out) = run_result!(%"sed", "s;zfs[[:space:]]*;zfs zfsutil,;g", Stdin(out))?;
    let StdoutTrimmed(fstab_zfs) = run_result!(%"grep", "zfs zfsutil", Stdin(out))?;
    writeln!(fstab_p, "{}", fstab_zfs)?;

    let efi_part = sail.get_efi_part()?;
    let StdoutTrimmed(uuid) = run_result!(%"blkid -s UUID -o value", efi_part)?;
    let fstab_efis = format!("/boot/efis/{}", sail.get_efi_last_path()?);
    let fstab_efis = format!("UUID={} {} {}", uuid, fstab_efis, "vfat x-systemd.idle-timeout=1min,x-systemd.automount,noauto,umask=0022,fmask=0022,dmask=0022 0 1");
    let fstab_efi = format!("UUID={} {}", uuid, "/boot/efi vfat x-systemd.idle-timeout=1min,x-systemd.automount,noauto,umask=0022,fmask=0022,dmask=0022 0 1");
    writeln!(fstab_p, "{}\n{}", fstab_efis, fstab_efi)?;

    eprintln!("\nConfigure mkinitcpio...\n");
    let mut mkinitcpio_p = openopt_a("/mnt/etc/mkinitcpio.conf")?;
    let hooks_c = "HOOKS=(base udev autodetect modconf block keyboard zfs filesystems)";
    run_result!(%"mv /mnt/etc/mkinitcpio.conf /mnt/etc/mkinitcpio.conf.old")?;
    writeln!(mkinitcpio_p, "{}", hooks_c)?;

    eprintln!("\nEnable internet time sync...\n");
    run_result!(%"hwclock --systohc")?;
    run_result!(%"systemctl enable systemd-timesyncd --root=/mnt")?;

    eprintln!("\nSet locale, timezone, keymap...\n");
    run_result!(%"rm -f /mnt/etc/localtime")?;
    run_result!(%"systemd-firstboot --root=/mnt --force --locale=en_US.UTF-8 --locale-messages=en_US.UTF-8 --keymap=us --timezone=Asia/Jakarta --hostname=lbox --root-password=123 --root-shell=/bin/bash")?;

    eprintln!("\nChange root password using chroot...\n");
    run_result!(%"arch-chroot /mnt passwd", Stdin("123\n123"))?;

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
    let mut kernel_updater_p = openopt_w("/mnt/usr/local/bin/kernel_updater")?;
    let kernel_updater_s = string_res::KERNEL_UPDATER_S;
    writeln!(kernel_updater_p, "{}", kernel_updater_s)?;
    run_result!(%"chmod +x /mnt/usr/local/bin/kernel_updater")?;

    eprintln!("\nEnable zfs services...\n");
    run_result!(%"systemctl enable zfs-import-scan.service zfs-import.target zfs-zed zfs.target --root=/mnt")?;
    run_result!(%"systemctl disable zfs-mount --root=/mnt")?;

    eprintln!("\nApply locales...\n");
    let locales_c = "en_US.UTF-8 UTF-8";
    let mut locale_file_p = openopt_w("/mnt/etc/locale.gen")?;
    writeln!(locale_file_p, "{}", locales_c)?;
    run_result!(%"arch-chroot /mnt bash --login", Stdin("locale-gen"))?;

    eprintln!("\nImport keys of archzfs...\n");
    let StdoutTrimmed(archzfs_gpg) =
        run_result!(%"curl -L https://mirror.sum7.eu/archlinux/archzfs/archzfs.gpg")?;
    run_result!(%"arch-chroot /mnt pacman-key -a -", Stdin(archzfs_gpg))?;

    let StdoutTrimmed(sign_key) = run_result!(%"curl -L https://git.io/JsfVS")?;
    run_result!(%"arch-chroot /mnt pacman-key --lsign-key", sign_key)?;

    let StdoutTrimmed(mirrorlist_c) = run_result!(%"curl -L https://git.io/Jsfw2")?;
    let mut mirrorlist_archzfs_p = openopt_w("/mnt/etc/pacman.d/mirrorlist-archzfs")?;
    writeln!(mirrorlist_archzfs_p, "{}", mirrorlist_c)?;

    eprintln!("\nAdd archzfs repo...\n");
    let mut pacman_conf_p = openopt_a("/mnt/etc/pacman.conf")?;
    let archzfs_repo_c = string_res::ARCHZFS_REPO_C;
    writeln!(pacman_conf_p, "{}", archzfs_repo_c)?;

    Ok(())
}

pub fn install_aurs() -> Result<()> {
    let arch_chroot = Split("arch-chroot /mnt bash --login");

    eprintln!("\nInstall paru...\n");
    let paru_install_i = string_res::PARU_INSTALL_I;
    run_result!(&arch_chroot, Stdin(paru_install_i))?;

    eprintln!("\nInstall boot environment manager...\n");
    let bieaz_install_i = string_res::BIEAZ_INSTALL_I;
    run_result!(&arch_chroot, Stdin(bieaz_install_i))?;

    eprintln!("\nInstall pacman hook for BEM...\n");
    let bieaz_pachook_install_i = string_res::BIEAZ_PACHOOK_INSTALL_I;
    run_result!(&arch_chroot, Stdin(bieaz_pachook_install_i))?;

    eprintln!("\nInstall zrepl auto snapshotter...\n");
    let zrepl_install_i = string_res::ZREPL_INSTALL_I;
    run_result!(&arch_chroot, Stdin(zrepl_install_i))?;

    eprintln!("\nGenerate zrepl configuration...\n");
    run_result!(%"mkdir -p /mnt/etc/zrepl")?;
    let mut zrepl_conf_path_p = openopt_w("/mnt/etc/zrepl/zrepl.yml")?;
    let zrepl_yml_c = string_res::ZREPL_YML_C;
    writeln!(zrepl_conf_path_p, "{}", zrepl_yml_c)?;

    eprintln!("\nDelete temporary user...\n");
    run_result!(%"rm /mnt/etc/sudoers.d/00_nobody")?;

    Ok(())
}

pub fn workarounds() -> Result<()> {
    eprintln!("\nGrub canonical path fix...\n");
    let mut zpool_vdev_p = openopt_w("/mnt/etc/profile.d/zpool_vdev_name_path.sh")?;
    let mut sudoers_p = openopt_a("/mnt/etc/sudoers")?;
    let canonical_fix_c = "export ZPOOL_VDEV_NAME_PATH=YES";
    let env_keep_c = r#"Defaults env_keep += "ZPOOL_VDEV_NAME_PATH""#;
    writeln!(zpool_vdev_p, "{}", canonical_fix_c)?;
    writeln!(sudoers_p, "{}", env_keep_c)?;

    eprintln!("\nPool name missing fix...\n");
    let exp =
        r"s/rpool=.*/rpool=`zdb -l ${GRUB_DEVICE} | grep -E '[[:blank:]]name' | cut -d\\' -f 2`/";
    run_result!(%"sed -i", exp, "/mnt/etc/grub.d/10_linux")?;

    Ok(())
}

pub fn bootloaders(sail: &Sail) -> Result<()> {
    let arch_chroot = Split("arch-chroot /mnt bash --login");

    eprintln!("\nGenerate initrd...\n");
    let gen_initrd_i = string_res::GEN_INITRD_I;
    run_result!(&arch_chroot, Stdin(gen_initrd_i))?;

    eprintln!("\nCreate grub boot dir, in esp and boot pool...\n");
    run_result!(%"mkdir -p /mnt/boot/efi/EFI/arch")?;
    run_result!(%"mkdir -p /mnt/boot/grub")?;

    eprintln!("\nInstall grub efi...\n");
    let grub_install_i = string_res::GRUB_INSTALL_I;
    let efibootmgr_i = format!(
        r#"efibootmgr -cgp 1 -l "\EFI\arch\grubx64.efi" -L "arch-{}" -d {}"#,
        sail.get_disk_last_path()?,
        sail.get_disk()
    );
    let grub_setup_i = [grub_install_i, &efibootmgr_i, "\n"].concat();
    run_result!(&arch_chroot, Stdin(grub_setup_i))?;

    eprintln!("\nGenerate grub menu...\n");
    let grub_menu_i = string_res::GRUB_MENU_I;
    run_result!(&arch_chroot, Stdin(grub_menu_i))?;

    eprintln!("\nMirror esp content...\n");
    let mirror_esp_i = string_res::MIRROR_ESP_I;
    run_result!(&arch_chroot, Stdin(mirror_esp_i))?;

    Ok(())
}

pub fn finishing(sail: &Sail) -> Result<()> {
    let some_delay = time::Duration::from_secs(1);

    eprintln!("\nGenerate monthly scrub service...\n");
    let mut scrub_timer_p = openopt_w("/mnt/etc/systemd/system/zfs-scrub@.timer")?;
    let mut scrub_service_p = openopt_w("/mnt/etc/systemd/system/zfs-scrub@.service")?;
    let scrub_timer_c = string_res::SCRUB_TIMER_C;
    let scrub_service_c = string_res::SCRUB_SERVICE_C;
    writeln!(scrub_timer_p, "{}", scrub_timer_c)?;
    writeln!(scrub_service_p, "{}", scrub_service_c)?;

    if sail.is_using_ssd() {
        eprintln!("\nGenerate monthly trim service...\n");
        let mut trim_timer_p = openopt_w("/mnt/etc/systemd/system/zfs-trim@.timer")?;
        let mut trim_service_p = openopt_w("/mnt/etc/systemd/system/zfs-trim@.service")?;
        let trim_timer_c = string_res::TRIM_TIMER_C;
        let trim_service_c = string_res::TRIM_SERVICE_C;
        writeln!(trim_timer_p, "{}", trim_timer_c)?;
        writeln!(trim_service_p, "{}", trim_service_c)?;
    }

    eprintln!("\nEnable systemd services...\n");
    let arch_chroot = Split("arch-chroot /mnt bash --login");
    let service_enable_i = string_res::SERVICE_ENABLE_I;
    run_result!(&arch_chroot, Stdin(service_enable_i))?;
    if sail.is_using_ssd() {
        let trim_enable_i = string_res::TRIM_ENABLE_I;
        run_result!(&arch_chroot, Stdin(trim_enable_i))?;
    }

    eprintln!("\nAdd wheel to sudoers...\n");
    let wheel_sudoers_c = "%wheel ALL=(ALL) ALL";
    let mut sudoers_p = openopt_a("/mnt/etc/sudoers")?;
    writeln!(sudoers_p, "{}", wheel_sudoers_c)?;

    let post_scripts_p = "/mnt/root/post_install_scripts";
    eprintln!("\nGenerating post-installation scripts...\n");
    run_result!(%"mkdir -p", post_scripts_p)?;

    let mut additional_storage_p = openopt_w([post_scripts_p, "/additional_storage.sh"].concat())?;
    let additional_storage_s = string_res::ADDITIONAL_STORAGE_S;
    writeln!(additional_storage_p, "{}", additional_storage_s)?;

    let mut add_user_p = openopt_w([post_scripts_p, "/add_user.sh"].concat())?;
    let add_user_s = string_res::ADD_USER_S;
    writeln!(add_user_p, "{}", add_user_s)?;

    let mut enable_services_p = openopt_w([post_scripts_p, "/enable_services.sh"].concat())?;
    let enable_services_s = string_res::ENABLE_SERVICES_S;
    writeln!(enable_services_p, "{}", enable_services_s)?;

    let mut zfs_mount_generator_p =
        openopt_w([post_scripts_p, "/zfs_mount_generator.sh"].concat())?;
    let zfs_mount_generator_s = string_res::ZFS_MOUNT_GENERATOR_S;
    writeln!(zfs_mount_generator_p, "{}", zfs_mount_generator_s)?;

    thread::sleep(some_delay);
    Ok(())
}

pub fn shot_and_clean() -> Result<()> {
    eprintln!("\nSnapshot of clean installation...\n");
    run_result!(%"zfs snapshot -r rpool/arch@install")?;
    run_result!(%"zfs snapshot -r bpool/arch@install")?;

    eprintln!("\nUnmount efi partition...\n");
    run_result!(%"umount /mnt/boot/efi")?;
    run_result!(%"bash --login", Stdin("umount /mnt/boot/efis/*\n"))?;

    eprintln!("\nExport pools...\n");
    run_result!(%"zpool export bpool")?;
    run_result!(%"zpool export rpool")?;

    Ok(())
}
