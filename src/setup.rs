use crate::{sail::Sail, string_res};
use anyhow::{bail, Context, Result};
use cradle::{
    input::{Split, Stdin},
    output::StdoutTrimmed,
    run_output, run_result,
};
use std::{fs::OpenOptions, io::Write, thread, time};

fn writeln_w(content: &str, path: &str) -> Result<()> {
    let mut path = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)?;
    writeln!(path, "{}", content)?;

    Ok(())
}

fn writeln_a(content: &str, path: &str) -> Result<()> {
    let mut path = OpenOptions::new().append(true).create(true).open(path)?;
    writeln!(path, "{}", content)?;

    Ok(())
}

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
        "id",
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

fn log(content: &str) {
    eprintln!("\n{}...\n", content);
}

pub fn partition_disk(sail: &Sail) -> Result<()> {
    let some_delay = time::Duration::from_secs(1);

    let disk = sail.get_disk();
    let partsize_esp = sail.get_partsize_esp();
    let partsize_bpool = sail.get_partsize_bpool();

    log("Find last partition number");
    let mut next_partnum = sail.get_next_partnum();

    log("Create efi partition");
    let efi_partnum = next_partnum.to_string();
    let part_desc = format!("-n{}:0:+{}", efi_partnum, partsize_esp);
    let part_type = format!("-t{}:EF00", efi_partnum);
    run_result!("sgdisk", part_desc, part_type, disk)?;

    log("Create bpool partition");
    next_partnum += 1;
    let bpool_partnum = next_partnum.to_string();
    let part_desc = format!("-n{}:0:+{}", bpool_partnum, partsize_bpool);
    let part_type = format!("-t{}:BE00", bpool_partnum);
    run_result!(%"sgdisk", part_desc, part_type, disk)?;

    log("Create rpool partition");
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

    log("Load zfs kernel module");
    run_result!(%"modprobe zfs")?;

    log("Create boot pool");
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

    log("Create root pool");
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

    log("Create root dataset");
    run_result!(%"zfs create -o canmount=off -o mountpoint=none rpool/arch")?;

    log("Create other dataset");
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

    log("Format and mount esp");
    run_result!(%"mkfs.vfat -n EFI", &efi_part)?;

    let efis_mnt = format!("/mnt/boot/efis/{}", sail.get_efi_last_path()?);

    run_result!(%"mkdir -p", &efis_mnt).context("Creating efis dir")?;
    run_result!(%"mount -t vfat", &efi_part, efis_mnt)?;
    run_result!(%"mkdir -p /mnt/boot/efi").context("Creating efi dir")?;
    run_result!(%"mount -t vfat", efi_part, "/mnt/boot/efi")?;

    log("Optional user data datasets");
    run_result!(%"zfs create -o canmount=on rpool/arch/DATA/default/var/games")?;
    run_result!(%"chmod 775 /mnt/var/games")?;
    run_result!(%"zfs create -o canmount=on rpool/arch/DATA/default/var/www")?;
    log("For GNOME");
    run_result!(%"zfs create -o canmount=on rpool/arch/DATA/default/var/lib/AccountsService")?;
    log("For Docker");
    run_result!(%"zfs create -o canmount=on rpool/arch/DATA/default/var/lib/docker")?;
    log("For NFS");
    run_result!(%"zfs create -o canmount=on rpool/arch/DATA/default/var/lib/nfs")?;
    log("For LXC");
    run_result!(%"zfs create -o canmount=on rpool/arch/DATA/default/var/lib/lxc")?;
    log("For LibVirt");
    run_result!(%"zfs create -o canmount=on rpool/arch/DATA/default/var/lib/libvirt")?;
    log("For nix");
    run_result!(%"zfs create -o canmount=on rpool/arch/DATA/default/nix")?;

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
        "mandoc",
        "mkinitcpio",
        "networkmanager",
        "reflector",
        "sudo",
    ];
    let linux = sail.get_linvar();
    let linux_headers = linux.to_owned() + "-headers";
    let zfs = sail.get_zfs();

    log("Update pacman repository");
    run_result!(%"pacman -Sy")?;

    log("Check compatible kernel version");
    let StdoutTrimmed(out) = run_result!(%"pacman -Si", zfs)?;
    let StdoutTrimmed(out) = run_result!("grep", "Depends On", Stdin(out))?;
    let exp = format!("s|.*{}=||", linux);
    let StdoutTrimmed(out) = run_result!("sed", exp, Stdin(out))?;
    let StdoutTrimmed(req_linver) = run_result!("awk", "{ print $1 }", Stdin(out))?;

    log("Check repo kernel version");
    let StdoutTrimmed(out) = run_result!(%"pacman -Si", linux)?;
    let StdoutTrimmed(out) = run_result!(%"grep Version", Stdin(out))?;
    let StdoutTrimmed(repo_linver) = run_result!("awk", "{ print $3 }", Stdin(out))?;

    log("Install base packages");
    run_result!(%"pacstrap -c /mnt", base)?;

    log("Install kernel, download from archive if not available");
    if req_linver == repo_linver {
        log("Install from repo");
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

    log("Install firmware");
    run_result!(%"pacstrap -c /mnt linux-firmware intel-ucode amd-ucode")?;

    log("Install zfs");
    run_result!(%"pacstrap -c /mnt", zfs, "zfs-utils")?;

    Ok(())
}

pub fn system_configuration(sail: &Sail) -> Result<()> {
    let arch_chroot = Split("arch-chroot /mnt bash --login");

    log("Set mkinitcpio zfs hook scan path");
    let grub_cmdline_c = format!(
        r#"{}GRUB_CMDLINE_LINUX="zfs_import_dir={}""#,
        "GRUB_DISABLE_OS_PROBER=false\n",
        sail.get_disk_parent()?
    );
    writeln_a(&grub_cmdline_c, "/mnt/etc/default/grub")?;

    log("Generate fstab");
    let StdoutTrimmed(out) = run_result!(%"genfstab -U /mnt")?;
    let StdoutTrimmed(out) = run_result!(%"sed", "s;zfs[[:space:]]*;zfs zfsutil,;g", Stdin(out))?;
    let StdoutTrimmed(fstab_zfs) = run_result!(%"grep", "zfs zfsutil", Stdin(out))?;
    writeln_w(&fstab_zfs, "/mnt/etc/fstab")?;

    let efi_part = sail.get_efi_part()?;
    let StdoutTrimmed(uuid) = run_result!(%"blkid -s UUID -o value", efi_part)?;
    let fstab_efis = format!("/boot/efis/{}", sail.get_efi_last_path()?);
    let fstab_efis = format!("UUID={} {} {}", uuid, fstab_efis, "vfat x-systemd.idle-timeout=1min,x-systemd.automount,noauto,umask=0022,fmask=0022,dmask=0022 0 1");
    let fstab_efi = format!("UUID={} {}", uuid, "/boot/efi vfat x-systemd.idle-timeout=1min,x-systemd.automount,noauto,umask=0022,fmask=0022,dmask=0022 0 1");
    let fstab_efi = format!("{}\n{}", fstab_efis, fstab_efi);
    writeln_a(&fstab_efi, "/mnt/etc/fstab")?;

    log("Configure mkinitcpio");
    run_result!(%"mv /mnt/etc/mkinitcpio.conf /mnt/etc/mkinitcpio.conf.old")?;
    let hooks_c = "HOOKS=(base udev autodetect modconf block keyboard zfs filesystems)";
    writeln_a(hooks_c, "/mnt/etc/mkinitcpio.conf")?;

    log("Enable internet time sync");
    run_result!(%"hwclock --systohc")?;
    run_result!(%"systemctl enable systemd-timesyncd --root=/mnt")?;

    log("Set locale, timezone, keymap");
    run_result!(%"rm -f /mnt/etc/localtime")?;
    run_result!(%"systemd-firstboot --root=/mnt --force --locale=en_US.UTF-8 --locale-messages=en_US.UTF-8 --keymap=us --timezone=Asia/Jakarta --hostname=lbox --root-password=123 --root-shell=/bin/bash")?;

    log("Change root password using chroot");
    run_result!(%"arch-chroot /mnt passwd", Stdin("123\n123"))?;

    log("Generate hostid");
    run_result!(%"zgenhostid -f -o /mnt/etc/hostid")?;

    log("Ignore kernel update");
    run_result!(%"sed -i", "s/#IgnorePkg/IgnorePkg/", "/mnt/etc/pacman.conf")?;
    let exp = format!(
        "/^IgnorePkg/ s/$/ {linux} {linux}-headers zfs-{linux} zfs-utils/",
        linux = sail.get_linvar()
    );
    run_result!(%"sed -i", exp, "/mnt/etc/pacman.conf")?;

    log("Generate kernel_updater script in /usr/local/bin");
    writeln_w(
        string_res::KERNEL_UPDATER_S,
        "/mnt/usr/local/bin/kernel_updater",
    )?;
    run_result!(%"chmod +x /mnt/usr/local/bin/kernel_updater")?;

    log("Enable zfs services");
    run_result!(%"systemctl enable zfs-import-scan.service zfs-import.target zfs-zed zfs.target --root=/mnt")?;
    run_result!(%"systemctl disable zfs-mount --root=/mnt")?;

    log("Apply locales");
    writeln_w("en_US.UTF-8 UTF-8", "/mnt/etc/locale.gen")?;
    run_result!(&arch_chroot, Stdin("locale-gen"))?;

    log("Import keys of archzfs");
    let import_archzfs_keys_i = string_res::IMPORT_ARCHZFS_KEYS_I;
    run_result!(&arch_chroot, Stdin(import_archzfs_keys_i))?;

    log("Add archzfs repo");
    writeln_a(string_res::ARCHZFS_REPO_C, "/mnt/etc/pacman.conf")?;

    Ok(())
}

pub fn install_aurs() -> Result<()> {
    let arch_chroot = Split("arch-chroot /mnt bash --login");

    log("Install paru");
    let paru_install_i = string_res::PARU_INSTALL_I;
    run_result!(&arch_chroot, Stdin(paru_install_i))?;

    log("Install boot environment manager");
    let bieaz_install_i = string_res::BIEAZ_INSTALL_I;
    run_result!(&arch_chroot, Stdin(bieaz_install_i))?;

    log("Install pacman hook for BEM");
    let bieaz_pachook_install_i = string_res::BIEAZ_PACHOOK_INSTALL_I;
    run_result!(&arch_chroot, Stdin(bieaz_pachook_install_i))?;

    log("Install zrepl auto snapshotter");
    let zrepl_install_i = string_res::ZREPL_INSTALL_I;
    run_result!(&arch_chroot, Stdin(zrepl_install_i))?;

    log("Generate zrepl configuration");
    run_result!(%"mkdir -p /mnt/etc/zrepl")?;
    writeln_w(string_res::ZREPL_YML_C, "/mnt/etc/zrepl/zrepl.yml")?;

    log("Delete temporary user");
    run_result!(%"rm /mnt/etc/sudoers.d/00_nobody")?;

    Ok(())
}

pub fn workarounds() -> Result<()> {
    log("Grub canonical path fix");
    let canonical_fix_c = "export ZPOOL_VDEV_NAME_PATH=YES";
    let env_keep_c = r#"Defaults env_keep += "ZPOOL_VDEV_NAME_PATH""#;
    writeln_w(
        canonical_fix_c,
        "/mnt/etc/profile.d/zpool_vdev_name_path.sh",
    )?;
    writeln_a(env_keep_c, "/mnt/etc/sudoers")?;

    log("Pool name missing fix");
    let exp =
        r"s/rpool=.*/rpool=`zdb -l ${GRUB_DEVICE} | grep -E '[[:blank:]]name' | cut -d\\' -f 2`/";
    run_result!(%"sed -i", exp, "/mnt/etc/grub.d/10_linux")?;

    Ok(())
}

pub fn bootloaders(sail: &Sail) -> Result<()> {
    let arch_chroot = Split("arch-chroot /mnt bash --login");

    log("Generate initrd");
    let gen_initrd_i = string_res::GEN_INITRD_I;
    run_result!(&arch_chroot, Stdin(gen_initrd_i))?;

    log("Create grub boot dir, in esp and boot pool");
    run_result!(%"mkdir -p /mnt/boot/efi/EFI/arch")?;
    run_result!(%"mkdir -p /mnt/boot/grub")?;

    log("Install grub efi");
    let grub_install_i = string_res::GRUB_INSTALL_I;
    let efibootmgr_i = format!(
        r#"efibootmgr -cgp 1 -l "\EFI\arch\grubx64.efi" -L "arch-{}" -d {}"#,
        sail.get_disk_last_path()?,
        sail.get_disk()
    );
    let grub_setup_i = [grub_install_i, &efibootmgr_i, "\n"].concat();
    run_result!(&arch_chroot, Stdin(grub_setup_i))?;

    log("Generate grub menu");
    let grub_menu_i = string_res::GRUB_MENU_I;
    run_result!(&arch_chroot, Stdin(grub_menu_i))?;

    log("Mirror esp content");
    let mirror_esp_i = string_res::MIRROR_ESP_I;
    run_result!(&arch_chroot, Stdin(mirror_esp_i))?;

    Ok(())
}

pub fn finishing(sail: &Sail) -> Result<()> {
    log("Generate monthly scrub service");
    writeln_w(
        string_res::SCRUB_TIMER_C,
        "/mnt/etc/systemd/system/zfs-scrub@.timer",
    )?;
    writeln_w(
        string_res::SCRUB_SERVICE_C,
        "/mnt/etc/systemd/system/zfs-scrub@.service",
    )?;

    if sail.is_using_ssd() {
        log("Generate monthly trim service");
        writeln_w(
            string_res::TRIM_TIMER_C,
            "/mnt/etc/systemd/system/zfs-trim@.timer",
        )?;
        writeln_w(
            string_res::TRIM_SERVICE_C,
            "/mnt/etc/systemd/system/zfs-trim@.service",
        )?;
    }

    log("Enable systemd services");
    let arch_chroot = Split("arch-chroot /mnt bash --login");
    let service_enable_i = string_res::SERVICE_ENABLE_I;
    run_result!(&arch_chroot, Stdin(service_enable_i))?;
    if sail.is_using_ssd() {
        let trim_enable_i = string_res::TRIM_ENABLE_I;
        run_result!(&arch_chroot, Stdin(trim_enable_i))?;
    }

    log("Add wheel to sudoers");
    writeln_a("%wheel ALL=(ALL) ALL", "/mnt/etc/sudoers")?;

    Ok(())
}

pub fn post_scripts_gen() -> Result<()> {
    log("Generating post-installation scripts");
    let post_scripts_p = "/mnt/root/post_install_scripts";
    run_result!(%"mkdir -p", post_scripts_p)?;

    let path_script_pairs = [
        ("additional_storage.sh", string_res::ADDITIONAL_STORAGE_S),
        ("add_user.sh", string_res::ADD_USER_S),
        ("enable_services.sh", string_res::ENABLE_SERVICES_S),
        ("nix_install.sh", string_res::NIX_INSTALL_S),
        ("zfs_mount_generator.sh", string_res::ZFS_MOUNT_GENERATOR_S),
    ];

    for pair in path_script_pairs {
        let path = [post_scripts_p, "/", pair.0].concat();
        writeln_w(pair.1, &path)?;
    }

    Ok(())
}

pub fn shot_and_clean() -> Result<()> {
    log("Snapshot of clean installation");
    run_result!(%"zfs snapshot -r rpool/arch@install")?;
    run_result!(%"zfs snapshot -r bpool/arch@install")?;

    log("Unmount efi partition");
    run_result!(%"umount /mnt/boot/efi")?;
    run_result!(%"bash --login", Stdin("umount /mnt/boot/efis/*\n"))?;

    log("Export pools");
    run_result!(%"zpool export bpool")?;
    run_result!(%"zpool export rpool")?;

    Ok(())
}
