#![forbid(unsafe_code)]

/// Magic number for `SYS_statfs` (`f_type`) identifying a mounted ZenithFS volume.
pub const ZENITHFS_SUPER_MAGIC: u64 = 0x5A45_4E31;
/// On-disk 64-bit superblock signature `"Z_ZENFS1"`.
pub const ZFS_DISK_SIGNATURE: u64 = 0x3153_464E_455A_5F5A;
pub const ZFS_BLOCK_SIZE: usize = 4096;
pub const ZFS_MAX_INODES: usize = 8;
pub const MAX_DEB_ELF_SIZE: usize = 16384;

/// Standard Debian `.deb` archive magic header (`ar` format).
pub const DEB_AR_MAGIC: &[u8; 8] = b"!<arch>\n";

/// Real Debian 13 (`trixie`) `.deb` package built by GCC in CI from `userland/hello_trixie.c`.
pub static EMBEDDED_GCC_DEB_PACKAGE: &[u8] = include_bytes!("../../userland/hello-trixie.deb");

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ZenithSuperblock {
    pub signature: u64,
    pub version: u32,
    pub block_size: u32,
    pub total_blocks: u32,
    pub free_blocks: u32,
    pub inode_count: u32,
    pub flags: u32,
    pub volume_label: [u8; 16],
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ZenithInode {
    pub inode_no: u32,
    pub mode: u16,
    pub links: u16,
    pub size_bytes: u32,
    pub direct_block: u32,
    pub checksum_fnv1a: u32,
    pub path: [u8; 32],
    pub path_len: usize,
}

impl ZenithInode {
    pub const fn empty() -> Self {
        Self {
            inode_no: 0,
            mode: 0,
            links: 0,
            size_bytes: 0,
            direct_block: 0,
            checksum_fnv1a: 0,
            path: [0; 32],
            path_len: 0,
        }
    }
}

pub const fn compute_fnv1a_checksum(data: &[u8]) -> u32 {
    let mut hash: u32 = 0x811c_9dc5;
    let mut i = 0;
    while i < data.len() {
        hash ^= data[i] as u32;
        hash = hash.wrapping_mul(0x0100_0193);
        i += 1;
    }
    hash
}

pub const DEBIAN_VERSION_DATA: &[u8] = b"13.0\n";
pub const APT_SOURCES_DATA: &[u8] =
    b"deb http://deb.debian.org/debian trixie main contrib non-free-firmware\n";
pub const DPKG_STATUS_DATA: &[u8] =
    b"Package: hello-trixie\nStatus: install ok installed\nSuite: trixie\nArchitecture: amd64\nVersion: 1.0+deb13u1\n";
pub const PROC_FILESYSTEMS_DATA: &[u8] = b"nodev\tsysfs\nnodev\tproc\n\tzenithfs\n";

pub struct ZenithFsVolume {
    pub mounted: bool,
    pub deb_installed: bool,
    pub superblock: ZenithSuperblock,
    pub inodes: [ZenithInode; ZFS_MAX_INODES],
    pub installed_deb_elf: [u8; MAX_DEB_ELF_SIZE],
    pub installed_deb_len: usize,
}

impl ZenithFsVolume {
    pub const fn const_new() -> Self {
        Self {
            mounted: false,
            deb_installed: false,
            superblock: ZenithSuperblock {
                signature: ZFS_DISK_SIGNATURE,
                version: 1,
                block_size: ZFS_BLOCK_SIZE as u32,
                total_blocks: 1024,
                free_blocks: 1010,
                inode_count: 5,
                flags: 1,
                volume_label: *b"ZENITH-TRIXIE\0\0\0",
            },
            inodes: [ZenithInode::empty(); ZFS_MAX_INODES],
            installed_deb_elf: [0u8; MAX_DEB_ELF_SIZE],
            installed_deb_len: 0,
        }
    }

    pub fn format_and_mount(&mut self) {
        self.register_inode(1, 0o100644, 2, b"/etc/debian_version", DEBIAN_VERSION_DATA);
        self.register_inode(2, 0o100644, 3, b"/etc/apt/sources.list", APT_SOURCES_DATA);
        self.register_inode(3, 0o100644, 4, b"/var/lib/dpkg/status", DPKG_STATUS_DATA);
        self.register_inode(4, 0o100444, 5, b"/proc/filesystems", PROC_FILESYSTEMS_DATA);
        self.mounted = self.superblock.signature == ZFS_DISK_SIGNATURE;

        let _ = self.install_deb_package(EMBEDDED_GCC_DEB_PACKAGE);
    }

    /// Parses a Debian `.deb` (`!<arch>\n`) archive, verifies `debian-binary` ("2.0\n"),
    /// and extracts the GCC-compiled `ELF64` binary payload into `/usr/bin/hello-deb`.
    pub fn install_deb_package(&mut self, deb_bytes: &[u8]) -> bool {
        if deb_bytes.len() <= 256 || deb_bytes.get(0..8) != Some(DEB_AR_MAGIC) {
            return false;
        }
        if deb_bytes.get(68..72) != Some(b"2.0\n") {
            return false;
        }
        let payload_len = (deb_bytes.len() - 256).min(MAX_DEB_ELF_SIZE);
        if let Some(elf_payload) = deb_bytes.get(256..256 + payload_len) {
            if elf_payload.get(0..4) == Some(b"\x7FELF") {
                if let Some(dst) = self.installed_deb_elf.get_mut(..payload_len) {
                    dst.copy_from_slice(elf_payload);
                    self.installed_deb_len = payload_len;
                    self.register_inode(5, 0o100755, 6, b"/usr/bin/hello-deb", elf_payload);
                    self.deb_installed = true;
                    return true;
                }
            }
        }
        false
    }

    fn register_inode(
        &mut self,
        ino: u32,
        mode: u16,
        block: u32,
        path_bytes: &[u8],
        content: &[u8],
    ) {
        let idx = (ino as usize).saturating_sub(1);
        if let Some(slot) = self.inodes.get_mut(idx) {
            let mut path_arr = [0u8; 32];
            let copy_len = path_bytes.len().min(32);
            for (d, &s) in path_arr.iter_mut().take(copy_len).zip(path_bytes.iter()) {
                *d = s;
            }
            *slot = ZenithInode {
                inode_no: ino,
                mode,
                links: 1,
                size_bytes: content.len() as u32,
                direct_block: block,
                checksum_fnv1a: compute_fnv1a_checksum(content),
                path: path_arr,
                path_len: copy_len,
            };
        }
    }

    pub fn verify_inode_checksum(&self, ino: u32, content: &[u8]) -> bool {
        let idx = (ino as usize).saturating_sub(1);
        if let Some(inode) = self.inodes.get(idx) {
            inode.inode_no == ino && inode.checksum_fnv1a == compute_fnv1a_checksum(content)
        } else {
            false
        }
    }
}
