pub const KERNEL_UPDATER_S: &str = r"
#!/bin/bash

INST_LINVAR=$(sed 's|.*linux|linux|' /proc/cmdline | sed 's|.img||g' | awk '{ print $1 }')
pacman -Sy --needed --noconfirm ${INST_LINVAR} ${INST_LINVAR}-headers zfs-${INST_LINVAR} zfs-utils
";

pub const IMPORT_ARCHZFS_KEYS_I: &str = r#"
curl -L https://archzfs.com/archzfs.gpg |  pacman-key -a -
pacman-key --lsign-key $(curl -L https://git.io/JsfVS)
curl -L https://git.io/Jsfw2 > /etc/pacman.d/mirrorlist-archzfs
"#;

pub const ARCHZFS_REPO_C: &str = r"
#[archzfs-testing]
#Include = /etc/pacman.d/mirrorlist-archzfs

[archzfs]
Include = /etc/pacman.d/mirrorlist-archzfs
";

pub const PARU_INSTALL_I: &str = r"
echo 'nobody ALL=(ALL) NOPASSWD: ALL' > /etc/sudoers.d/00_nobody
su - nobody -s /bin/bash
mkdir /tmp/build
cd /tmp/build
git clone https://aur.archlinux.org/paru-bin.git
cd paru-bin
makepkg -si
Y
";

pub const BIEAZ_INSTALL_I: &str = r"
su - nobody -s /bin/bash
mkdir /tmp/build
cd /tmp/build
git clone https://aur.archlinux.org/bieaz.git
cd bieaz
makepkg -si
Y
";

pub const BIEAZ_PACHOOK_INSTALL_I: &str = r"
su - nobody -s /bin/bash
mkdir /tmp/build
cd /tmp/build
git clone https://aur.archlinux.org/rozb3-pac.git
cd rozb3-pac
makepkg -si
Y
";

pub const ZREPL_INSTALL_I: &str = r"
su - nobody -s /bin/bash
mkdir /tmp/build
cd /tmp/build
git clone https://aur.archlinux.org/zrepl-bin.git
cd zrepl-bin
makepkg -si
Y
";

pub const ZREPL_YML_C: &str = r#"
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

pub const GEN_INITRD_I: &str = r"
rm -f /etc/zfs/zpool.cache
touch /etc/zfs/zpool.cache
chmod a-w /etc/zfs/zpool.cache
chattr +i /etc/zfs/zpool.cache
mkinitcpio -P
";

pub const GRUB_INSTALL_I: &str = r"
grub-install --boot-directory /boot/efi/EFI/arch --efi-directory /boot/efi/
grub-install --boot-directory /boot/efi/EFI/arch --efi-directory /boot/efi/ --removable
";

pub const GRUB_MENU_I: &str = r"
grub-mkconfig -o /boot/efi/EFI/arch/grub/grub.cfg
cp /boot/efi/EFI/arch/grub/grub.cfg /boot/grub/grub.cfg
";

pub const MIRROR_ESP_I: &str = r"
ESP_MIRROR=$(mktemp -d)
cp -r /boot/efi/EFI $ESP_MIRROR
for i in /boot/efis/*; do
cp -r $ESP_MIRROR/EFI $i
done
";

pub const SCRUB_TIMER_C: &str = r"
[Unit]
Description=Monthly zpool scrub on %i

[Timer]
OnCalendar=monthly
AccuracySec=1h
Persistent=true

[Install]
WantedBy=multi-user.target
";

pub const SCRUB_SERVICE_C: &str = r"
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

pub const TRIM_TIMER_C: &str = r"
[Unit]
Description=Monthly zpool trim on %i

[Timer]
OnCalendar=monthly
AccuracySec=1h
Persistent=true

[Install]
WantedBy=multi-user.target
";

pub const TRIM_SERVICE_C: &str = r"
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

pub const SERVICE_ENABLE_I: &str = r"
systemctl enable NetworkManager
systemctl enable zfs-scrub@rpool.timer
systemctl enable zfs-scrub@bpool.timer
";

pub const TRIM_ENABLE_I: &str = r"
systemctl enable zfs-trim@rpool.timer
systemctl enable zfs-trim@bpool.timer
";

pub const ADDITIONAL_STORAGE_S: &str = r#"
set -e

my_user=UserName
pool_name=tank0
disk=/dev/disk/by-path/virtio-pci-0000:04:00.0-part1
tmp_mpoint=/mnt/tmpmnt
dsets_mpoint_pair=(
    "Downloads /home/${my_user}/Downloads"
    "dot_cache /home/${my_user}/.cache"
)

mkdir -p "$tmp_mpoint"

zpool create \
    -o ashift=12 \
    -o autotrim=on \
    -R "$tmp_mpoint" \
    -O acltype=posixacl \
    -O canmount=off \
    -O compression=zstd \
    -O dnodesize=auto \
    -O normalization=formD \
    -O relatime=on \
    -O xattr=sa \
    -O mountpoint=/ \
    ${pool_name} \
    ${disk}

zfs create -o canmount=off -o mountpoint=none ${pool_name}/arch
zfs create -o canmount=off -o mountpoint=none ${pool_name}/arch/DATA
zfs create -o canmount=off -o mountpoint=none ${pool_name}/arch/DATA/default

for pair in "${dsets_mpoint_pair[@]}"; do
    read -r dset mpoint <<< "$pair"

    zfs create -o mountpoint="$mpoint" -o canmount=on ${pool_name}/arch/DATA/default/"$dset"

    chown -R ${my_user}:${my_user} "$tmp_mpoint"/"$mpoint"

    echo "${pool_name}/arch/DATA/default/$dset  $mpoint zfs x-systemd.automount,noauto,zfsutil,rw,xattr,posixacl   0 0" >> /etc/fstab
done

zpool export "$pool_name"
rm -rf "$tmp_mpoint"
"#;

pub const ADD_USER_S: &str = r"
my_user=UserName
useradd -m -G wheel -s /bin/bash ${my_user}
passwd ${my_user}
";

pub const ENABLE_SERVICES_S: &str = r"
systemctl enable zrepl
";

pub const ZFS_MOUNT_GENERATOR_S: &str = r#"
DATA_POOL='tank0 tank1'

# tab-separated zfs properties
# see /etc/zfs/zed.d/history_event-zfs-list-cacher.sh
export \
PROPS="name,mountpoint,canmount,atime,relatime,devices,exec\
,readonly,setuid,nbmand,encroot,keylocation"

mkdir -p /etc/zfs/zfs-list.cache

for i in $DATA_POOL; do
  zfs list -H -t filesystem -o $PROPS -r $i > /etc/zfs/zfs-list.cache/$i
done
"#;

pub const NIX_INSTALL_S: &str = r#"
set -e
my_user=UserName

pacman -S nix
systemctl enable nix-daemon.service
gpasswd -a "${my_user}" nix-users

cat <<EOF > /home/"${my_user}"/nix_channel_add.sh
nix-channel --add https://nixos.org/channels/nixpkgs-unstable
nix-channel --update
EOF

cat <<EOF > /home/"${my_user}"/home_manager_install.sh
nix-channel --add https://github.com/nix-community/home-manager/archive/master.tar.gz home-manager
nix-channel --update

export NIX_PATH=$HOME/.nix-defexpr/channels${NIX_PATH:+:}$NIX_PATH
echo "source or add this command below to your shell"
echo 'export NIX_PATH=$HOME/.nix-defexpr/channels${NIX_PATH:+:}$NIX_PATH'

nix-shell '<home-manager>' -A install
EOF

echo -e "\nReboot as ${my_user} and execute /home/${my_user}/nix_channel_setup.sh"
"#;

pub const GNOME_INSTALL_S: &str = r"
pacman -S gnome
systemctl enable gdm.service
";
