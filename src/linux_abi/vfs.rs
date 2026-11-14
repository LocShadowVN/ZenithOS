#![forbid(unsafe_code)]

use crate::linux_abi::LinuxErrno;
use crate::ostd::copy_from_buffer;

/// ZenithOS Passwordless Capability Security Bitmask (`Z-Cap`).
/// Replaces traditional Linux `sudo` password prompts with scoped per-process capabilities.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZCapRights(pub u64);

impl ZCapRights {
    /// Allows reading and writing files inside `/home` and `/tmp` without elevation.
    pub const FS_USER_HOME: u64 = 1 << 0;
    /// Allows installing verified `.deb` packages into `/usr/bin` without `sudo`.
    pub const PKG_INSTALL: u64 = 1 << 1;
    /// Allows accessing `/dev/fb0` and `/dev/input/event0` for desktop UI applications.
    pub const DEV_DESKTOP_UI: u64 = 1 << 2;
    /// Allows modifying immutable system files (`/etc`, `/boot`). Disabled for normal apps!
    pub const SYS_ADMIN_IMMUTABLE: u64 = 1 << 3;

    pub const DEFAULT_DESKTOP_USER: Self =
        Self(Self::FS_USER_HOME | Self::PKG_INSTALL | Self::DEV_DESKTOP_UI);

    #[inline]
    pub const fn has(self, flag: u64) -> bool {
        (self.0 & flag) != 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VfsNodeKind {
    RootDir,
    OsRelease,
    ProcVersion,
    ZenithFsInode(usize),
    DevNull,
    DevZero,
    DevFb0,
    DevInputEvent0,
    BinInit,
    BinApp,
    UsrBinHelloDeb,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapObject {
    SerialIn,
    SerialOut,
    SerialErr,
    PipeRead(usize),
    PipeWrite(usize),
    VfsFile { kind: VfsNodeKind, offset: usize },
    VfsDir { offset: usize },
    EpollInstance,
}

#[derive(Debug, Clone, Copy)]
pub struct CapabilityEntry {
    pub object: CapObject,
    pub can_read: bool,
    pub can_write: bool,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct LinuxInputEvent {
    pub tv_sec: u64,
    pub tv_usec: u64,
    pub type_: u16,
    pub code: u16,
    pub value: i32,
}

impl LinuxInputEvent {
    pub fn to_bytes(self) -> [u8; 24] {
        let mut out = [0u8; 24];
        out[0..8].copy_from_slice(&self.tv_sec.to_le_bytes());
        out[8..16].copy_from_slice(&self.tv_usec.to_le_bytes());
        out[16..18].copy_from_slice(&self.type_.to_le_bytes());
        out[18..20].copy_from_slice(&self.code.to_le_bytes());
        out[20..24].copy_from_slice(&self.value.to_le_bytes());
        out
    }
}

pub const MAX_FDS: usize = 16;
pub const MAX_PIPES: usize = 4;
pub const PIPE_BUF_SIZE: usize = 256;

pub struct PipeRingBuffer {
    pub allocated: bool,
    pub data: [u8; PIPE_BUF_SIZE],
    pub read_pos: usize,
    pub write_pos: usize,
    pub count: usize,
}

impl Default for PipeRingBuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl PipeRingBuffer {
    pub const fn new() -> Self {
        Self {
            allocated: false,
            data: [0; PIPE_BUF_SIZE],
            read_pos: 0,
            write_pos: 0,
            count: 0,
        }
    }

    pub fn write_slice(&mut self, src: &[u8]) -> usize {
        let mut written = 0;
        for &b in src {
            if self.count >= PIPE_BUF_SIZE {
                break;
            }
            if let Some(slot) = self.data.get_mut(self.write_pos) {
                *slot = b;
                self.write_pos = (self.write_pos + 1) % PIPE_BUF_SIZE;
                self.count += 1;
                written += 1;
            }
        }
        written
    }

    pub fn read_slice(&mut self, dst: &mut [u8]) -> usize {
        let mut read_bytes = 0;
        for slot in dst.iter_mut() {
            if self.count == 0 {
                break;
            }
            if let Some(&b) = self.data.get(self.read_pos) {
                *slot = b;
                self.read_pos = (self.read_pos + 1) % PIPE_BUF_SIZE;
                self.count -= 1;
                read_bytes += 1;
            }
        }
        read_bytes
    }
}

pub const OS_RELEASE_CONTENT: &[u8] =
    b"NAME=\"ZenithOS\"\nVERSION=\"1.1.0 (Trixie)\"\nID=zenith\nID_LIKE=debian\nPRETTY_NAME=\"ZenithOS 1.1.0 (Debian 13 Trixie amd64)\"\n";
pub const PROC_VERSION_CONTENT: &[u8] =
    b"Linux version 6.8.0-zenith-framekernel (rust@zenith) #1 SMP PREEMPT_DYNAMIC\n";

pub fn resolve_vfs_path(path: &[u8]) -> Result<VfsNodeKind, LinuxErrno> {
    match path {
        b"/" | b"." | b"/home" | b"/home/user" => Ok(VfsNodeKind::RootDir),
        b"/etc/os-release" | b"etc/os-release" => Ok(VfsNodeKind::OsRelease),
        b"/etc/debian_version" | b"etc/debian_version" => Ok(VfsNodeKind::ZenithFsInode(0)),
        b"/etc/apt/sources.list" => Ok(VfsNodeKind::ZenithFsInode(1)),
        b"/var/lib/dpkg/status" => Ok(VfsNodeKind::ZenithFsInode(2)),
        b"/proc/filesystems" => Ok(VfsNodeKind::ZenithFsInode(3)),
        b"/proc/version" | b"proc/version" => Ok(VfsNodeKind::ProcVersion),
        b"/dev/null" => Ok(VfsNodeKind::DevNull),
        b"/dev/zero" => Ok(VfsNodeKind::DevZero),
        b"/dev/fb0" => Ok(VfsNodeKind::DevFb0),
        b"/dev/input/event0" => Ok(VfsNodeKind::DevInputEvent0),
        b"/bin/init" => Ok(VfsNodeKind::BinInit),
        b"/bin/app" | b"/bin/gui_app" => Ok(VfsNodeKind::BinApp),
        b"/usr/bin/hello-deb" => Ok(VfsNodeKind::UsrBinHelloDeb),
        _ => Err(LinuxErrno::Enoent),
    }
}

/// Reads a null-terminated C string byte-by-byte from user space without crossing unmapped page boundaries.
pub fn read_cstr_from_user(user_ptr: u64, buf: &mut [u8; 64]) -> Result<&[u8], LinuxErrno> {
    let mut len = 0;
    while len < buf.len() {
        let mut byte = [0u8; 1];
        copy_from_buffer(user_ptr.wrapping_add(len as u64), &mut byte)
            .map_err(|_| LinuxErrno::Efault)?;
        if byte[0] == 0 {
            break;
        }
        if let Some(slot) = buf.get_mut(len) {
            *slot = byte[0];
        }
        len += 1;
    }
    buf.get(..len).ok_or(LinuxErrno::Einval)
}
