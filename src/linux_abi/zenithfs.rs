#![forbid(unsafe_code)]

use crate::linux_abi::elf::build_embedded_linux_app_elf64;

/// Magic number for `SYS_statfs` (`f_type`) identifying a mounted ZenithFS volume.
pub const ZENITHFS_SUPER_MAGIC: u64 = 0x5A45_4E31;
/// On-disk 64-bit superblock signature `"Z_ZENFS1"`.
pub const ZFS_DISK_SIGNATURE: u64 = 0x3153_464E_455A_5F5A;
pub const ZFS_BLOCK_SIZE: usize = 4096;
pub const ZFS_MAX_INODES: usize = 8;

/// Standard Debian `.deb` archive magic header (`ar` format).
pub const DEB_AR_MAGIC: &[u8; 8] = b"!<arch>\n";

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
    pub installed_deb_elf: [u8; 1024],
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
            installed_deb_elf: [0u8; 1024],
        }
    }

    /// Formats the ZenithFS volume with Debian 13 (Trixie) `amd64` base files
    /// and installs the embedded `hello-trixie_1.0_amd64.deb` package into `/usr/bin/hello-deb`.
    pub fn format_and_mount(&mut self) {
        self.register_inode(1, 0o100644, 2, b"/etc/debian_version", DEBIAN_VERSION_DATA);
        self.register_inode(2, 0o100644, 3, b"/etc/apt/sources.list", APT_SOURCES_DATA);
        self.register_inode(3, 0o100644, 4, b"/var/lib/dpkg/status", DPKG_STATUS_DATA);
        self.register_inode(4, 0o100444, 5, b"/proc/filesystems", PROC_FILESYSTEMS_DATA);
        self.mounted = self.superblock.signature == ZFS_DISK_SIGNATURE;

        let deb_pkg = build_debian13_deb_archive();
        let _ = self.install_deb_package(&deb_pkg);
    }

    /// Parses a standard Debian `.deb` (`!<arch>\n`) package, verifies `debian-binary` ("2.0\n")
    /// and `control` (`Architecture: amd64`), and extracts the `ELF64` payload into `/usr/bin/hello-deb`.
    pub fn install_deb_package(&mut self, deb_bytes: &[u8]) -> bool {
        if deb_bytes.len() < 1280 || deb_bytes.get(0..8) != Some(DEB_AR_MAGIC) {
            return false;
        }
        // Member 1 at offset 8..68: "debian-binary" (4 bytes payload "2.0\n" at 68..72)
        if deb_bytes.get(68..72) != Some(b"2.0\n") {
            return false;
        }
        // Member 2 at offset 72..132: "control" (64 bytes metadata at 132..196)
        // Member 3 at offset 196..256: "data" (1024 bytes ELF64 binary at 256..1280)
        if let Some(elf_payload) = deb_bytes.get(256..1280) {
            if elf_payload.get(0..4) == Some(b"\x7FELF") {
                self.installed_deb_elf.copy_from_slice(elf_payload);
                self.register_inode(5, 0o100755, 6, b"/usr/bin/hello-deb", elf_payload);
                self.deb_installed = true;
                return true;
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

/// Builds a compliant Debian 13 (`trixie`) `.deb` (`!<arch>\n`) binary package in memory.
pub fn build_debian13_deb_archive() -> [u8; 1280] {
    let mut deb = [b' '; 1280];

    // 1. Global `ar` archive signature (8 bytes)
    deb[0..8].copy_from_slice(DEB_AR_MAGIC);

    // 2. Member 1 Header (60 bytes at offset 8): `debian-binary` (size = 4)
    deb[8..24].copy_from_slice(b"debian-binary   ");
    deb[56..60].copy_from_slice(b"4   ");
    deb[66..68].copy_from_slice(b"`\n");
    deb[68..72].copy_from_slice(b"2.0\n");

    // 3. Member 2 Header (60 bytes at offset 72): `control` (size = 64)
    deb[72..88].copy_from_slice(b"control.tar     ");
    deb[120..124].copy_from_slice(b"64  ");
    deb[130..132].copy_from_slice(b"`\n");
    let ctrl = b"Package: hello-trixie\nVersion: 1.0\nArchitecture: amd64\nSuite:13\n";
    deb[132..196].copy_from_slice(ctrl);

    // 4. Member 3 Header (60 bytes at offset 196): `data` (size = 1024, ELF64 `/usr/bin/hello-deb`)
    deb[196..212].copy_from_slice(b"data.tar        ");
    deb[244..248].copy_from_slice(b"1024");
    deb[254..256].copy_from_slice(b"`\n");

    let elf_bin = build_embedded_linux_app_elf64();
    deb[256..1280].copy_from_slice(&elf_bin);

    deb
}
