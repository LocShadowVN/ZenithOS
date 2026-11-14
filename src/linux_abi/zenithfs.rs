#![forbid(unsafe_code)]

pub const ZENITHFS_SUPER_MAGIC: u64 = 0x5A45_4E31;
pub const ZFS_DISK_SIGNATURE: u64 = 0x3153_464E_455A_5F5A;
pub const ZFS_BLOCK_SIZE: usize = 4096;
pub const ZFS_MAX_INODES: usize = 8;
pub const ZFS_DATA_SLOT_SIZE: usize = 256;
pub const MAX_DEB_ELF_SIZE: usize = 16384;

/// Standard 8-byte GPT Header Signature `"EFI PART"` at LBA 1.
pub const GPT_HEADER_SIGNATURE: &[u8; 8] = b"EFI PART";
pub const DEB_AR_MAGIC: &[u8; 8] = b"!<arch>\n";

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
    pub system_immutable: bool,
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
            system_immutable: false,
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
    b"Package: zenith-desktop\nStatus: install ok installed\nSuite: trixie\nArchitecture: amd64\n";
pub const FSTAB_DATA: &[u8] =
    b"UUID=ZENITH-TRIXIE  /          zenithfs  defaults,zcap,noatime  0  1\nUUID=ZENITH-ESP     /boot/efi  vfat      umask=0077,shim        0  2\n";
pub const PROC_FILESYSTEMS_DATA: &[u8] = b"nodev\tsysfs\nnodev\tproc\n\tzenithfs\n";

/// UEFI Secure Boot variable state (`SecureBoot = 1`, `SetupMode = 0`, Shim + Zenith MOK active).
pub const EFI_SECURE_BOOT_DATA: &[u8] = b"\x06\x00\x00\x00\x01";

pub struct ZenithFsVolume {
    pub mounted: bool,
    pub deb_installed: bool,
    pub disk_installed: bool,
    pub disk_synced: bool,
    pub superblock: ZenithSuperblock,
    pub inodes: [ZenithInode; ZFS_MAX_INODES],
    pub data_blocks: [[u8; ZFS_DATA_SLOT_SIZE]; ZFS_MAX_INODES],
    pub vda_raw_sectors: [u8; 2048],
    pub installed_deb_elf: [u8; MAX_DEB_ELF_SIZE],
    pub installed_deb_len: usize,
}

impl ZenithFsVolume {
    pub const fn const_new() -> Self {
        Self {
            mounted: false,
            deb_installed: false,
            disk_installed: false,
            disk_synced: false,
            superblock: ZenithSuperblock {
                signature: ZFS_DISK_SIGNATURE,
                version: 1,
                block_size: ZFS_BLOCK_SIZE as u32,
                total_blocks: 1024,
                free_blocks: 1008,
                inode_count: 5,
                flags: 1,
                volume_label: *b"ZENITH-TRIXIE\0\0\0",
            },
            inodes: [ZenithInode::empty(); ZFS_MAX_INODES],
            data_blocks: [[0u8; ZFS_DATA_SLOT_SIZE]; ZFS_MAX_INODES],
            vda_raw_sectors: [0u8; 2048],
            installed_deb_elf: [0u8; MAX_DEB_ELF_SIZE],
            installed_deb_len: 0,
        }
    }

    pub fn format_and_mount(&mut self) {
        self.register_inode(
            1,
            0o100644,
            true,
            b"/etc/debian_version",
            DEBIAN_VERSION_DATA,
        );
        self.register_inode(2, 0o100644, true, b"/etc/apt/sources.list", APT_SOURCES_DATA);
        self.register_inode(3, 0o100644, true, b"/var/lib/dpkg/status", DPKG_STATUS_DATA);
        self.register_inode(4, 0o100644, true, b"/etc/fstab", FSTAB_DATA);
        self.mounted = self.superblock.signature == ZFS_DISK_SIGNATURE;

        // Initialize default GPT + Protective MBR + ESP UEFI Shim layout on `/dev/vda`
        self.vda_raw_sectors[510] = 0x55;
        self.vda_raw_sectors[511] = 0xAA;
        self.vda_raw_sectors[512..520].copy_from_slice(GPT_HEADER_SIGNATURE);
        self.vda_raw_sectors[1024..1053].copy_from_slice(b"SHIM-MOK/EFI/BOOT/BOOTX64.EFI");
        self.disk_installed = true;
        self.disk_synced = true;

        let _ = self.install_deb_package(EMBEDDED_GCC_DEB_PACKAGE);
    }

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
                    self.register_inode(5, 0o100755, true, b"/usr/bin/hello-deb", b"ELF64_DEB");
                    self.deb_installed = true;
                    return true;
                }
            }
        }
        false
    }

    pub fn write_vda_disk(&mut self, offset: usize, src: &[u8]) -> Option<usize> {
        if offset >= self.vda_raw_sectors.len() {
            return None;
        }
        let avail = self.vda_raw_sectors.len() - offset;
        let to_write = src.len().min(avail);
        if let (Some(dst_slice), Some(src_slice)) = (
            self.vda_raw_sectors.get_mut(offset..offset + to_write),
            src.get(..to_write),
        ) {
            dst_slice.copy_from_slice(src_slice);
            if self.vda_raw_sectors.get(512..520) == Some(GPT_HEADER_SIGNATURE) {
                self.disk_installed = true;
            }
            return Some(to_write);
        }
        None
    }

    pub fn verify_gpt_and_shim_install(&self) -> bool {
        self.disk_installed
            && self.disk_synced
            && self.vda_raw_sectors.get(510..512) == Some(&[0x55, 0xAA])
            && self.vda_raw_sectors.get(512..520) == Some(GPT_HEADER_SIGNATURE)
            && self.vda_raw_sectors.get(1024..1032) == Some(b"SHIM-MOK")
    }

    pub fn find_inode_by_path(&self, path: &[u8]) -> Option<usize> {
        for (idx, inode) in self.inodes.iter().enumerate() {
            if inode.inode_no != 0 && inode.path_len == path.len() {
                if let Some(stored) = inode.path.get(..inode.path_len) {
                    if stored == path {
                        return Some(idx);
                    }
                }
            }
        }
        None
    }

    pub fn create_user_file(&mut self, path: &[u8], mode: u16) -> Option<usize> {
        if let Some(existing_idx) = self.find_inode_by_path(path) {
            return Some(existing_idx);
        }
        for idx in 0..ZFS_MAX_INODES {
            if let Some(slot) = self.inodes.get(idx) {
                if slot.inode_no == 0 {
                    let ino = (idx + 1) as u32;
                    self.register_inode(ino, mode, false, path, &[]);
                    self.superblock.inode_count += 1;
                    self.superblock.free_blocks = self.superblock.free_blocks.saturating_sub(1);
                    return Some(idx);
                }
            }
        }
        None
    }

    pub fn write_inode_data(&mut self, idx: usize, offset: usize, src: &[u8]) -> Option<usize> {
        let inode = self.inodes.get_mut(idx)?;
        if inode.inode_no == 0 || inode.system_immutable || offset >= ZFS_DATA_SLOT_SIZE {
            return None;
        }
        let block = self.data_blocks.get_mut(idx)?;
        let avail = ZFS_DATA_SLOT_SIZE - offset;
        let to_write = src.len().min(avail);
        if let (Some(dst_slice), Some(src_slice)) = (
            block.get_mut(offset..offset + to_write),
            src.get(..to_write),
        ) {
            dst_slice.copy_from_slice(src_slice);
            let new_end = (offset + to_write) as u32;
            if new_end > inode.size_bytes {
                inode.size_bytes = new_end;
            }
            if let Some(valid_data) = block.get(..inode.size_bytes as usize) {
                inode.checksum_fnv1a = compute_fnv1a_checksum(valid_data);
            }
            return Some(to_write);
        }
        None
    }

    pub fn read_inode_data(&self, idx: usize, offset: usize, dst: &mut [u8]) -> Option<usize> {
        let inode = self.inodes.get(idx)?;
        if inode.inode_no == 0 {
            return None;
        }
        let size = inode.size_bytes as usize;
        if offset >= size {
            return Some(0);
        }
        let block = self.data_blocks.get(idx)?;
        let avail = size - offset;
        let to_read = dst.len().min(avail);
        if let (Some(src_slice), Some(dst_slice)) = (
            block.get(offset..offset + to_read),
            dst.get_mut(..to_read),
        ) {
            dst_slice.copy_from_slice(src_slice);
            return Some(to_read);
        }
        None
    }

    fn register_inode(
        &mut self,
        ino: u32,
        mode: u16,
        system_immutable: bool,
        path_bytes: &[u8],
        content: &[u8],
    ) {
        let idx = (ino as usize).saturating_sub(1);
        if let (Some(slot), Some(block)) = (self.inodes.get_mut(idx), self.data_blocks.get_mut(idx))
        {
            let mut path_arr = [0u8; 32];
            let copy_len = path_bytes.len().min(32);
            for (d, &s) in path_arr.iter_mut().take(copy_len).zip(path_bytes.iter()) {
                *d = s;
            }
            *block = [0u8; ZFS_DATA_SLOT_SIZE];
            let data_len = content.len().min(ZFS_DATA_SLOT_SIZE);
            for (d, &s) in block.iter_mut().take(data_len).zip(content.iter()) {
                *d = s;
            }
            let valid_slice = block.get(..data_len).unwrap_or(&[]);
            *slot = ZenithInode {
                inode_no: ino,
                mode,
                system_immutable,
                links: 1,
                size_bytes: data_len as u32,
                direct_block: ino + 1,
                checksum_fnv1a: compute_fnv1a_checksum(valid_slice),
                path: path_arr,
                path_len: copy_len,
            };
        }
    }

    pub fn verify_inode_checksum(&self, ino: u32, expected_content: &[u8]) -> bool {
        let idx = (ino as usize).saturating_sub(1);
        if let Some(inode) = self.inodes.get(idx) {
            inode.inode_no == ino
                && inode.checksum_fnv1a == compute_fnv1a_checksum(expected_content)
        } else {
            false
        }
    }
}
