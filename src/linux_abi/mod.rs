#![forbid(unsafe_code)]

pub mod console;
pub mod elf;
pub mod vfs;
pub mod zenithfs;

pub use console::{ascii_to_glyph_8x8, draw_glyph_8x8, draw_text_line, init_fbcon, GlyphStyle};
pub use elf::{
    build_embedded_linux_app_elf64, build_embedded_linux_init_elf64, load_elf64_into_user_space,
    ElfLoadError, LoadedElfProcess,
};
pub use vfs::{
    read_cstr_from_user, resolve_vfs_path, CapObject, CapabilityEntry, LinuxInputEvent,
    PipeRingBuffer, VfsNodeKind, MAX_FDS, MAX_PIPES, OS_RELEASE_CONTENT, PIPE_BUF_SIZE,
    PROC_VERSION_CONTENT,
};
pub use zenithfs::{
    ZenithFsVolume, APT_SOURCES_DATA, DEBIAN_VERSION_DATA, DPKG_STATUS_DATA, PROC_FILESYSTEMS_DATA,
    ZENITHFS_SUPER_MAGIC, ZFS_BLOCK_SIZE,
};

use crate::ostd::{
    copy_from_buffer, copy_to_buffer, fb_draw_pixel, map_user_page, poll_ps2_scancode, read_msr,
    read_tsc, write_msr, BitmapFrameAllocator, FramebufferInfo, Msr, SerialConsole, TrapFrame,
    PAGE_SIZE,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinuxErrno {
    Eperm = 1,
    Enoent = 2,
    Esrch = 3,
    Ebadf = 9,
    Echild = 10,
    Eagain = 11,
    Enomem = 12,
    Eacces = 13,
    Efault = 14,
    Enotdir = 20,
    Eisdir = 21,
    Einval = 22,
    Emfile = 24,
    Erange = 34,
    Enosys = 38,
}

impl LinuxErrno {
    #[inline(always)]
    pub const fn as_neg_ret(self) -> u64 {
        (-(self as isize)) as u64
    }
}

pub const SYS_READ: u64 = 0;
pub const SYS_WRITE: u64 = 1;
pub const SYS_OPEN: u64 = 2;
pub const SYS_CLOSE: u64 = 3;
pub const SYS_FSTAT: u64 = 5;
pub const SYS_POLL: u64 = 7;
pub const SYS_LSEEK: u64 = 8;
pub const SYS_MMAP: u64 = 9;
pub const SYS_MPROTECT: u64 = 10;
pub const SYS_MUNMAP: u64 = 11;
pub const SYS_BRK: u64 = 12;
pub const SYS_IOCTL: u64 = 16;
pub const SYS_WRITEV: u64 = 20;
pub const SYS_SCHED_YIELD: u64 = 24;
pub const SYS_DUP: u64 = 32;
pub const SYS_DUP2: u64 = 33;
pub const SYS_NANOSLEEP: u64 = 35;
pub const SYS_GETPID: u64 = 39;
pub const SYS_CLONE: u64 = 56;
pub const SYS_EXECVE: u64 = 59;
pub const SYS_EXIT: u64 = 60;
pub const SYS_WAIT4: u64 = 61;
pub const SYS_UNAME: u64 = 63;
pub const SYS_GETCWD: u64 = 79;
pub const SYS_CHDIR: u64 = 80;
pub const SYS_GETPPID: u64 = 110;
pub const SYS_STATFS: u64 = 137;
pub const SYS_ARCH_PRCTL: u64 = 158;
pub const SYS_GETTID: u64 = 186;
pub const SYS_FUTEX: u64 = 202;
pub const SYS_GETDENTS64: u64 = 217;
pub const SYS_SET_TID_ADDRESS: u64 = 218;
pub const SYS_CLOCK_GETTIME: u64 = 228;
pub const SYS_EXIT_GROUP: u64 = 231;
pub const SYS_EPOLL_CTL: u64 = 233;
pub const SYS_OPENAT: u64 = 257;
pub const SYS_NEWFSTATAT: u64 = 262;
pub const SYS_FACCESSAT: u64 = 269;
pub const SYS_EPOLL_PWAIT: u64 = 281;
pub const SYS_EPOLL_CREATE1: u64 = 291;
pub const SYS_PIPE2: u64 = 293;

pub const ARCH_SET_GS: u64 = 0x1001;
pub const ARCH_SET_FS: u64 = 0x1002;
pub const ARCH_GET_FS: u64 = 0x1003;
pub const ARCH_GET_GS: u64 = 0x1004;

pub const TIOCGWINSZ: u64 = 0x5413;
pub const FBIOGET_VSCREENINFO: u64 = 0x4600;
pub const FBIOGET_FSCREENINFO: u64 = 0x4602;

pub const FUTEX_WAIT: u64 = 0;
pub const FUTEX_WAKE: u64 = 1;
pub const FUTEX_PRIVATE_FLAG: u64 = 128;

pub const EV_KEY: u16 = 0x01;
pub const KEY_ENTER: u16 = 28;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    Unused,
    Running,
    Ready,
    BlockedOnFutex(u64),
    Exited(i32),
}

#[derive(Clone, Copy)]
pub struct TaskControlBlock {
    pub pid: u64,
    pub ppid: u64,
    pub state: TaskState,
    pub clear_child_tid: u64,
}

impl TaskControlBlock {
    pub const fn unused() -> Self {
        Self {
            pid: 0,
            ppid: 0,
            state: TaskState::Unused,
            clear_child_tid: 0,
        }
    }
}

pub const MAX_TASKS: usize = 8;

pub struct KernelContext {
    pub pmm: BitmapFrameAllocator,
    pub fb: FramebufferInfo,
    pub zfs: ZenithFsVolume,
    pub fb_pixels_written: u64,
    pub last_fb_pixel_color: u32,
    pub term_col: u32,
    pub term_row: u32,
    pub term_chars_rendered: u64,
    pub evdev_events_read: u64,
    pub current_pid: u64,
    pub next_pid: u64,
    pub program_break: u64,
    pub next_mmap_va: u64,
    pub last_exit_code: Option<i32>,
    pub cwd: [u8; 64],
    pub cwd_len: usize,
    pub fd_table: [Option<CapabilityEntry>; MAX_FDS],
    pub pipes: [PipeRingBuffer; MAX_PIPES],
    pub tasks: [TaskControlBlock; MAX_TASKS],
}

impl KernelContext {
    pub const fn const_new() -> Self {
        let mut fds: [Option<CapabilityEntry>; MAX_FDS] = [None; MAX_FDS];
        fds[0] = Some(CapabilityEntry {
            object: CapObject::SerialIn,
            can_read: true,
            can_write: false,
        });
        fds[1] = Some(CapabilityEntry {
            object: CapObject::SerialOut,
            can_read: false,
            can_write: true,
        });
        fds[2] = Some(CapabilityEntry {
            object: CapObject::SerialErr,
            can_read: false,
            can_write: true,
        });

        let mut tasks = [TaskControlBlock::unused(); MAX_TASKS];
        tasks[0] = TaskControlBlock {
            pid: 1,
            ppid: 0,
            state: TaskState::Running,
            clear_child_tid: 0,
        };

        let mut cwd = [0u8; 64];
        cwd[0] = b'/';

        Self {
            pmm: BitmapFrameAllocator::const_uninit(),
            fb: FramebufferInfo::const_uninit(),
            zfs: ZenithFsVolume::const_new(),
            fb_pixels_written: 0,
            last_fb_pixel_color: 0,
            term_col: 0,
            term_row: 0,
            term_chars_rendered: 0,
            evdev_events_read: 0,
            current_pid: 1,
            next_pid: 2,
            program_break: 0x0000_0000_0040_3000,
            next_mmap_va: 0x0000_0000_1000_0000,
            last_exit_code: None,
            cwd,
            cwd_len: 1,
            fd_table: fds,
            pipes: [
                PipeRingBuffer::new(),
                PipeRingBuffer::new(),
                PipeRingBuffer::new(),
                PipeRingBuffer::new(),
            ],
            tasks,
        }
    }

    pub fn init_hardware(&mut self) {
        self.pmm = BitmapFrameAllocator::new();
        self.zfs.format_and_mount();
        self.fb = FramebufferInfo::detect_from_limine();
        init_fbcon(&self.fb);
        self.render_terminal_bytes(b"root@zenith-os:~# mount -t zenithfs /dev/vda1 /\n");
        self.render_terminal_bytes(b"[ZENITH-FS] Mounted ZFS1 volume 'ZENITH-TRIXIE' (Debian 13.0)\n");
        self.render_terminal_bytes(b"root@zenith-os:~# dpkg -i hello-trixie_1.0_amd64.deb\n");
        self.render_terminal_bytes(b"Unpacking hello-trixie (1.0) to /usr/bin/hello-deb...\n");
    }

    pub fn render_terminal_bytes(&mut self, bytes: &[u8]) {
        let win1_w = ((self.fb.width * 58) / 100).saturating_sub(56).max(280);
        let max_cols = (win1_w / 8).max(32);
        for &ch in bytes {
            if ch == b'\n' {
                self.term_col = 0;
                self.term_row = (self.term_row + 1) % 36;
                continue;
            }
            if ch == b'\r' {
                self.term_col = 0;
                continue;
            }
            let px_x = 38 + self.term_col * 8;
            let px_y = 72 + self.term_row * 12;
            let color = if self.term_row == 0 || self.term_row == 2 {
                0x0060_CDFF
            } else {
                0x00E5_E7EB
            };
            draw_glyph_8x8(
                &self.fb,
                ascii_to_glyph_8x8(ch),
                GlyphStyle {
                    x: px_x,
                    y: px_y,
                    scale: 1,
                    color,
                },
            );
            self.term_col += 1;
            if self.term_col >= max_cols {
                self.term_col = 0;
                self.term_row = (self.term_row + 1) % 36;
            }
            self.term_chars_rendered += 1;
        }
    }

    pub fn alloc_fd(&mut self, entry: CapabilityEntry) -> Result<usize, LinuxErrno> {
        for (idx, slot) in self.fd_table.iter_mut().enumerate() {
            if slot.is_none() {
                *slot = Some(entry);
                return Ok(idx);
            }
        }
        Err(LinuxErrno::Emfile)
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct LinuxUtsName {
    pub sysname: [u8; 65],
    pub nodename: [u8; 65],
    pub release: [u8; 65],
    pub version: [u8; 65],
    pub machine: [u8; 65],
    pub domainname: [u8; 65],
}

impl LinuxUtsName {
    pub fn zenith_default() -> Self {
        let mut uts = Self {
            sysname: [0; 65],
            nodename: [0; 65],
            release: [0; 65],
            version: [0; 65],
            machine: [0; 65],
            domainname: [0; 65],
        };
        Self::fill_field(&mut uts.sysname, b"Linux");
        Self::fill_field(&mut uts.nodename, b"zenith-os");
        Self::fill_field(&mut uts.release, b"6.8.0-zenith-framekernel");
        Self::fill_field(&mut uts.version, b"#1 SMP PREEMPT_DYNAMIC ZenithOS Debian 13");
        Self::fill_field(&mut uts.machine, b"x86_64");
        Self::fill_field(&mut uts.domainname, b"(none)");
        uts
    }

    fn fill_field(dst: &mut [u8; 65], src: &[u8]) {
        for (d, &s) in dst.iter_mut().take(64).zip(src.iter()) {
            *d = s;
        }
    }

    pub fn to_bytes(self) -> [u8; 390] {
        let mut out = [0u8; 390];
        Self::copy_slice(&mut out, 0, &self.sysname);
        Self::copy_slice(&mut out, 65, &self.nodename);
        Self::copy_slice(&mut out, 130, &self.release);
        Self::copy_slice(&mut out, 195, &self.version);
        Self::copy_slice(&mut out, 260, &self.machine);
        Self::copy_slice(&mut out, 325, &self.domainname);
        out
    }

    fn copy_slice(dst: &mut [u8; 390], offset: usize, src: &[u8; 65]) {
        if let Some(sub) = dst.get_mut(offset..offset + 65) {
            for (d, &s) in sub.iter_mut().zip(src.iter()) {
                *d = s;
            }
        }
    }
}

pub fn dispatch_linux_syscall(frame: &mut TrapFrame, ctx: &mut KernelContext) {
    let result = match frame.rax {
        SYS_READ => sys_read(ctx, frame.rdi, frame.rsi, frame.rdx),
        SYS_WRITE => sys_write(ctx, frame.rdi, frame.rsi, frame.rdx),
        SYS_OPEN => sys_openat(ctx, -100i64 as u64, frame.rdi, frame.rsi),
        SYS_CLOSE => sys_close(ctx, frame.rdi),
        SYS_FSTAT => sys_fstat(ctx, frame.rdi, frame.rsi),
        SYS_POLL => Ok(1),
        SYS_LSEEK => sys_lseek(ctx, frame.rdi, frame.rsi as i64, frame.rdx),
        SYS_MMAP => sys_mmap(ctx, frame.rdi, frame.rsi),
        SYS_MPROTECT | SYS_MUNMAP => Ok(0),
        SYS_BRK => sys_brk(ctx, frame.rdi),
        SYS_IOCTL => sys_ioctl(ctx, frame.rdi, frame.rsi, frame.rdx),
        SYS_WRITEV => sys_writev(ctx, frame.rdi, frame.rsi, frame.rdx),
        SYS_SCHED_YIELD | SYS_NANOSLEEP => Ok(0),
        SYS_DUP => sys_dup(ctx, frame.rdi),
        SYS_DUP2 => sys_dup2(ctx, frame.rdi, frame.rsi),
        SYS_GETPID | SYS_GETTID => Ok(ctx.current_pid),
        SYS_GETPPID => Ok(0),
        SYS_CLONE => sys_clone(ctx),
        SYS_EXECVE => sys_execve(frame, ctx),
        SYS_WAIT4 => sys_wait4(ctx, frame.rdi, frame.rsi),
        SYS_UNAME => sys_uname(frame.rdi),
        SYS_GETCWD => sys_getcwd(ctx, frame.rdi, frame.rsi),
        SYS_CHDIR => sys_chdir(ctx, frame.rdi),
        SYS_STATFS => sys_statfs(ctx, frame.rdi, frame.rsi),
        SYS_ARCH_PRCTL => sys_arch_prctl(frame.rdi, frame.rsi),
        SYS_FUTEX => sys_futex(ctx, frame.rdi, frame.rsi, frame.rdx as u32),
        SYS_GETDENTS64 => sys_getdents64(ctx, frame.rdi, frame.rsi, frame.rdx),
        SYS_SET_TID_ADDRESS => {
            if let Some(t) = ctx.tasks.get_mut(0) {
                t.clear_child_tid = frame.rdi;
            }
            Ok(ctx.current_pid)
        }
        SYS_CLOCK_GETTIME => sys_clock_gettime(frame.rdi, frame.rsi),
        SYS_EPOLL_CREATE1 => sys_epoll_create1(ctx),
        SYS_EPOLL_CTL => Ok(0),
        SYS_EPOLL_PWAIT => sys_epoll_pwait(ctx, frame.rdi, frame.rsi, frame.rdx),
        SYS_OPENAT => sys_openat(ctx, frame.rdi, frame.rsi, frame.rdx),
        SYS_NEWFSTATAT => sys_newfstatat(frame.rsi, frame.rdx),
        SYS_FACCESSAT => sys_faccessat(frame.rsi),
        SYS_PIPE2 => sys_pipe2(ctx, frame.rdi),
        SYS_EXIT | SYS_EXIT_GROUP => {
            ctx.last_exit_code = Some(frame.rdi as i32);
            Ok(0)
        }
        _ => Err(LinuxErrno::Enosys),
    };

    frame.rax = match result {
        Ok(val) => val,
        Err(errno) => errno.as_neg_ret(),
    };
}

fn sys_statfs(ctx: &KernelContext, path_ptr: u64, buf_ptr: u64) -> Result<u64, LinuxErrno> {
    let mut path_buf = [0u8; 64];
    let path_slice = read_cstr_from_user(path_ptr, &mut path_buf)?;
    let _kind = resolve_vfs_path(path_slice)?;

    let mut statfs_bytes = [0u8; 64];
    statfs_bytes[0..8].copy_from_slice(&ZENITHFS_SUPER_MAGIC.to_le_bytes());
    statfs_bytes[8..16].copy_from_slice(&(ctx.zfs.superblock.block_size as u64).to_le_bytes());
    statfs_bytes[16..24].copy_from_slice(&(ctx.zfs.superblock.total_blocks as u64).to_le_bytes());
    statfs_bytes[24..32].copy_from_slice(&(ctx.zfs.superblock.free_blocks as u64).to_le_bytes());
    statfs_bytes[32..40].copy_from_slice(&(ctx.zfs.superblock.free_blocks as u64).to_le_bytes());
    copy_to_buffer(buf_ptr, &statfs_bytes).map_err(|_| LinuxErrno::Efault)?;
    Ok(0)
}

fn sys_epoll_create1(ctx: &mut KernelContext) -> Result<u64, LinuxErrno> {
    let fd = ctx.alloc_fd(CapabilityEntry {
        object: CapObject::EpollInstance,
        can_read: true,
        can_write: true,
    })?;
    Ok(fd as u64)
}

fn sys_epoll_pwait(
    ctx: &KernelContext,
    epfd: u64,
    events_ptr: u64,
    maxevents: u64,
) -> Result<u64, LinuxErrno> {
    let cap = ctx
        .fd_table
        .get(epfd as usize)
        .and_then(|c| *c)
        .ok_or(LinuxErrno::Ebadf)?;
    if cap.object != CapObject::EpollInstance || maxevents == 0 {
        return Err(LinuxErrno::Einval);
    }
    let mut ep_ev = [0u8; 12];
    ep_ev[0..4].copy_from_slice(&1u32.to_le_bytes());
    copy_to_buffer(events_ptr, &ep_ev).map_err(|_| LinuxErrno::Efault)?;
    Ok(1)
}

fn sys_openat(
    ctx: &mut KernelContext,
    _dirfd: u64,
    path_ptr: u64,
    _flags: u64,
) -> Result<u64, LinuxErrno> {
    let mut path_buf = [0u8; 64];
    let path_slice = read_cstr_from_user(path_ptr, &mut path_buf)?;
    let kind = resolve_vfs_path(path_slice)?;

    let entry = match kind {
        VfsNodeKind::RootDir => CapabilityEntry {
            object: CapObject::VfsDir { offset: 0 },
            can_read: true,
            can_write: false,
        },
        VfsNodeKind::DevNull | VfsNodeKind::DevZero | VfsNodeKind::DevFb0 => CapabilityEntry {
            object: CapObject::VfsFile { kind, offset: 0 },
            can_read: true,
            can_write: true,
        },
        _ => CapabilityEntry {
            object: CapObject::VfsFile { kind, offset: 0 },
            can_read: true,
            can_write: false,
        },
    };

    let fd = ctx.alloc_fd(entry)?;
    Ok(fd as u64)
}

fn sys_read(
    ctx: &mut KernelContext,
    fd: u64,
    buf_ptr: u64,
    count: u64,
) -> Result<u64, LinuxErrno> {
    let cap = ctx
        .fd_table
        .get(fd as usize)
        .and_then(|c| *c)
        .ok_or(LinuxErrno::Ebadf)?;
    if !cap.can_read {
        return Err(LinuxErrno::Ebadf);
    }
    match cap.object {
        CapObject::SerialIn => Ok(0),
        CapObject::PipeRead(pipe_idx) => {
            let pipe = ctx.pipes.get_mut(pipe_idx).ok_or(LinuxErrno::Ebadf)?;
            let max_len = (count as usize).min(128);
            let mut tmp = [0u8; 128];
            let slice = tmp.get_mut(..max_len).ok_or(LinuxErrno::Efault)?;
            let n = pipe.read_slice(slice);
            if let Some(valid_out) = slice.get(..n) {
                copy_to_buffer(buf_ptr, valid_out).map_err(|_| LinuxErrno::Efault)?;
            }
            Ok(n as u64)
        }
        CapObject::VfsFile {
            kind: VfsNodeKind::DevInputEvent0,
            ..
        } => {
            if count < 24 {
                return Err(LinuxErrno::Einval);
            }
            let hw_scancode = poll_ps2_scancode().unwrap_or(KEY_ENTER as u8);
            let ev = LinuxInputEvent {
                tv_sec: 1,
                tv_usec: 500,
                type_: EV_KEY,
                code: hw_scancode as u16,
                value: 1,
            };
            copy_to_buffer(buf_ptr, &ev.to_bytes()).map_err(|_| LinuxErrno::Efault)?;
            ctx.evdev_events_read += 1;
            Ok(24)
        }
        CapObject::VfsFile { kind, offset } => {
            let content: &[u8] = match kind {
                VfsNodeKind::OsRelease => OS_RELEASE_CONTENT,
                VfsNodeKind::DebianVersion => DEBIAN_VERSION_DATA,
                VfsNodeKind::AptSourcesList => APT_SOURCES_DATA,
                VfsNodeKind::DpkgStatus => DPKG_STATUS_DATA,
                VfsNodeKind::ProcVersion => PROC_VERSION_CONTENT,
                VfsNodeKind::ProcFilesystems => PROC_FILESYSTEMS_DATA,
                VfsNodeKind::DevNull => &[],
                VfsNodeKind::DevZero => &[0u8; 128],
                _ => return Err(LinuxErrno::Einval),
            };
            if offset >= content.len() {
                return Ok(0);
            }
            let avail = content.len() - offset;
            let to_read = (count as usize).min(avail);
            let slice = content
                .get(offset..offset + to_read)
                .ok_or(LinuxErrno::Efault)?;
            copy_to_buffer(buf_ptr, slice).map_err(|_| LinuxErrno::Efault)?;

            if let Some(Some(slot)) = ctx.fd_table.get_mut(fd as usize) {
                slot.object = CapObject::VfsFile {
                    kind,
                    offset: offset + to_read,
                };
            }
            Ok(to_read as u64)
        }
        CapObject::VfsDir { .. } => Err(LinuxErrno::Eisdir),
        _ => Err(LinuxErrno::Ebadf),
    }
}

fn sys_lseek(
    ctx: &mut KernelContext,
    fd: u64,
    offset_arg: i64,
    whence: u64,
) -> Result<u64, LinuxErrno> {
    let slot = ctx
        .fd_table
        .get_mut(fd as usize)
        .and_then(|c| c.as_mut())
        .ok_or(LinuxErrno::Ebadf)?;
    if let CapObject::VfsFile { kind, offset } = slot.object {
        let new_off = match whence {
            0 => offset_arg as usize,
            1 => (offset as i64 + offset_arg) as usize,
            _ => return Err(LinuxErrno::Einval),
        };
        slot.object = CapObject::VfsFile {
            kind,
            offset: new_off,
        };
        Ok(new_off as u64)
    } else {
        Err(LinuxErrno::Einval)
    }
}

fn sys_getdents64(
    ctx: &mut KernelContext,
    fd: u64,
    dirp_ptr: u64,
    count: u64,
) -> Result<u64, LinuxErrno> {
    let slot = ctx
        .fd_table
        .get_mut(fd as usize)
        .and_then(|c| c.as_mut())
        .ok_or(LinuxErrno::Ebadf)?;
    if let CapObject::VfsDir { offset } = slot.object {
        if offset > 0 {
            return Ok(0);
        }
        if count < 48 {
            return Err(LinuxErrno::Einval);
        }
        let mut dirents = [0u8; 48];
        dirents[0..8].copy_from_slice(&1u64.to_le_bytes());
        dirents[8..16].copy_from_slice(&24u64.to_le_bytes());
        dirents[16..18].copy_from_slice(&24u16.to_le_bytes());
        dirents[18] = 4;
        dirents[19..23].copy_from_slice(b"bin\0");

        dirents[24..32].copy_from_slice(&2u64.to_le_bytes());
        dirents[32..40].copy_from_slice(&48u64.to_le_bytes());
        dirents[40..42].copy_from_slice(&24u16.to_le_bytes());
        dirents[42] = 4;
        dirents[43..47].copy_from_slice(b"etc\0");

        copy_to_buffer(dirp_ptr, &dirents).map_err(|_| LinuxErrno::Efault)?;
        slot.object = CapObject::VfsDir { offset: 2 };
        Ok(48)
    } else {
        Err(LinuxErrno::Enotdir)
    }
}

fn sys_getcwd(ctx: &KernelContext, buf_ptr: u64, size: u64) -> Result<u64, LinuxErrno> {
    let needed = ctx.cwd_len + 1;
    if (size as usize) < needed {
        return Err(LinuxErrno::Erange);
    }
    let slice = ctx.cwd.get(..needed).ok_or(LinuxErrno::Efault)?;
    copy_to_buffer(buf_ptr, slice).map_err(|_| LinuxErrno::Efault)?;
    Ok(buf_ptr)
}

fn sys_chdir(ctx: &mut KernelContext, path_ptr: u64) -> Result<u64, LinuxErrno> {
    let mut path_buf = [0u8; 64];
    let path_slice = read_cstr_from_user(path_ptr, &mut path_buf)?;
    let kind = resolve_vfs_path(path_slice)?;
    if kind != VfsNodeKind::RootDir {
        return Err(LinuxErrno::Enotdir);
    }
    ctx.cwd = [0u8; 64];
    ctx.cwd[0] = b'/';
    ctx.cwd_len = 1;
    Ok(0)
}

fn sys_newfstatat(path_ptr: u64, statbuf_ptr: u64) -> Result<u64, LinuxErrno> {
    let mut path_buf = [0u8; 64];
    let path_slice = read_cstr_from_user(path_ptr, &mut path_buf)?;
    let _kind = resolve_vfs_path(path_slice)?;
    let zero_stat = [0u8; 144];
    copy_to_buffer(statbuf_ptr, &zero_stat).map_err(|_| LinuxErrno::Efault)?;
    Ok(0)
}

fn sys_faccessat(path_ptr: u64) -> Result<u64, LinuxErrno> {
    let mut path_buf = [0u8; 64];
    let path_slice = read_cstr_from_user(path_ptr, &mut path_buf)?;
    let _kind = resolve_vfs_path(path_slice)?;
    Ok(0)
}

fn sys_execve(frame: &mut TrapFrame, ctx: &mut KernelContext) -> Result<u64, LinuxErrno> {
    let mut path_buf = [0u8; 64];
    let path_slice = read_cstr_from_user(frame.rdi, &mut path_buf)?;
    let kind = resolve_vfs_path(path_slice)?;

    let elf_image = match kind {
        VfsNodeKind::UsrBinHelloDeb => {
            if !ctx.zfs.deb_installed {
                return Err(LinuxErrno::Enoent);
            }
            ctx.zfs.installed_deb_elf
        }
        VfsNodeKind::BinApp => build_embedded_linux_app_elf64(),
        VfsNodeKind::BinInit => build_embedded_linux_init_elf64(),
        _ => return Err(LinuxErrno::Eacces),
    };

    let loaded =
        load_elf64_into_user_space(&elf_image, &mut ctx.pmm).map_err(|_| LinuxErrno::Enomem)?;

    ctx.program_break = loaded.initial_brk;

    frame.rcx = loaded.entry_rip;
    frame.rsp = loaded.user_rsp;
    frame.rdi = 0;
    frame.rsi = 0;
    frame.rdx = 0;
    frame.r8 = 0;
    frame.r9 = 0;
    frame.r10 = 0;

    Ok(0)
}

fn sys_write(
    ctx: &mut KernelContext,
    fd: u64,
    buf_ptr: u64,
    count: u64,
) -> Result<u64, LinuxErrno> {
    let cap = ctx
        .fd_table
        .get(fd as usize)
        .and_then(|c| *c)
        .ok_or(LinuxErrno::Ebadf)?;
    if !cap.can_write {
        return Err(LinuxErrno::Ebadf);
    }
    if count == 0 {
        return Ok(0);
    }

    match cap.object {
        CapObject::SerialOut | CapObject::SerialErr => {
            let mut temp_buf = [0u8; 128];
            let mut written: u64 = 0;
            while written < count {
                let remaining = (count - written) as usize;
                let chunk_len = remaining.min(temp_buf.len());
                if let Some(slice) = temp_buf.get_mut(..chunk_len) {
                    copy_from_buffer(buf_ptr.wrapping_add(written), slice)
                        .map_err(|_| LinuxErrno::Efault)?;
                    SerialConsole::write_bytes(slice);
                    ctx.render_terminal_bytes(slice);
                    written += chunk_len as u64;
                } else {
                    return Err(LinuxErrno::Efault);
                }
            }
            Ok(written)
        }
        CapObject::PipeWrite(pipe_idx) => {
            let max_len = (count as usize).min(128);
            let mut tmp = [0u8; 128];
            let slice = tmp.get_mut(..max_len).ok_or(LinuxErrno::Efault)?;
            copy_from_buffer(buf_ptr, slice).map_err(|_| LinuxErrno::Efault)?;
            let pipe = ctx.pipes.get_mut(pipe_idx).ok_or(LinuxErrno::Ebadf)?;
            let n = pipe.write_slice(slice);
            Ok(n as u64)
        }
        CapObject::VfsFile {
            kind: VfsNodeKind::DevFb0,
            offset,
        } => {
            let max_bytes = (count as usize).min(128);
            let mut pix_buf = [0u8; 128];
            let slice = pix_buf.get_mut(..max_bytes).ok_or(LinuxErrno::Efault)?;
            copy_from_buffer(buf_ptr, slice).map_err(|_| LinuxErrno::Efault)?;

            let num_pixels = max_bytes / 4;
            for i in 0..num_pixels {
                let mut p_bytes = [0u8; 4];
                if let Some(src_px) = slice.get(i * 4..i * 4 + 4) {
                    p_bytes.copy_from_slice(src_px);
                    let color = u32::from_le_bytes(p_bytes);
                    let px_index = (offset / 4) + i;
                    let draw_x = (px_index as u32) % ctx.fb.width;
                    let draw_y = 320.min(ctx.fb.height.saturating_sub(1));
                    fb_draw_pixel(&ctx.fb, draw_x, draw_y, color);
                    if i == 0 {
                        ctx.last_fb_pixel_color = color;
                    }
                    ctx.fb_pixels_written += 1;
                }
            }

            if let Some(Some(slot)) = ctx.fd_table.get_mut(fd as usize) {
                slot.object = CapObject::VfsFile {
                    kind: VfsNodeKind::DevFb0,
                    offset: offset + max_bytes,
                };
            }
            Ok(max_bytes as u64)
        }
        CapObject::VfsFile {
            kind: VfsNodeKind::DevNull | VfsNodeKind::DevZero,
            ..
        } => Ok(count),
        _ => Err(LinuxErrno::Ebadf),
    }
}

fn sys_close(ctx: &mut KernelContext, fd: u64) -> Result<u64, LinuxErrno> {
    let slot = ctx.fd_table.get_mut(fd as usize).ok_or(LinuxErrno::Ebadf)?;
    if slot.is_none() {
        return Err(LinuxErrno::Ebadf);
    }
    *slot = None;
    Ok(0)
}

fn sys_dup(ctx: &mut KernelContext, oldfd: u64) -> Result<u64, LinuxErrno> {
    let cap = ctx
        .fd_table
        .get(oldfd as usize)
        .and_then(|c| *c)
        .ok_or(LinuxErrno::Ebadf)?;
    let new_fd = ctx.alloc_fd(cap)?;
    Ok(new_fd as u64)
}

fn sys_dup2(ctx: &mut KernelContext, oldfd: u64, newfd: u64) -> Result<u64, LinuxErrno> {
    let cap = ctx
        .fd_table
        .get(oldfd as usize)
        .and_then(|c| *c)
        .ok_or(LinuxErrno::Ebadf)?;
    if oldfd == newfd {
        return Ok(newfd);
    }
    let target_slot = ctx
        .fd_table
        .get_mut(newfd as usize)
        .ok_or(LinuxErrno::Ebadf)?;
    *target_slot = Some(cap);
    Ok(newfd)
}

fn sys_pipe2(ctx: &mut KernelContext, pipefd_user_ptr: u64) -> Result<u64, LinuxErrno> {
    let mut pipe_idx = None;
    for (i, p) in ctx.pipes.iter_mut().enumerate() {
        if !p.allocated {
            p.allocated = true;
            p.read_pos = 0;
            p.write_pos = 0;
            p.count = 0;
            pipe_idx = Some(i);
            break;
        }
    }
    let idx = pipe_idx.ok_or(LinuxErrno::Emfile)?;

    let read_fd = ctx.alloc_fd(CapabilityEntry {
        object: CapObject::PipeRead(idx),
        can_read: true,
        can_write: false,
    })?;

    let write_fd = match ctx.alloc_fd(CapabilityEntry {
        object: CapObject::PipeWrite(idx),
        can_read: false,
        can_write: true,
    }) {
        Ok(fd) => fd,
        Err(e) => {
            if let Some(slot) = ctx.fd_table.get_mut(read_fd) {
                *slot = None;
            }
            return Err(e);
        }
    };

    let mut fds_bytes = [0u8; 8];
    fds_bytes[0..4].copy_from_slice(&(read_fd as i32).to_le_bytes());
    fds_bytes[4..8].copy_from_slice(&(write_fd as i32).to_le_bytes());
    copy_to_buffer(pipefd_user_ptr, &fds_bytes).map_err(|_| LinuxErrno::Efault)?;
    Ok(0)
}

fn sys_fstat(ctx: &KernelContext, fd: u64, statbuf_ptr: u64) -> Result<u64, LinuxErrno> {
    let _cap = ctx
        .fd_table
        .get(fd as usize)
        .and_then(|c| *c)
        .ok_or(LinuxErrno::Ebadf)?;
    let zero_stat = [0u8; 144];
    copy_to_buffer(statbuf_ptr, &zero_stat).map_err(|_| LinuxErrno::Efault)?;
    Ok(0)
}

fn sys_writev(
    ctx: &mut KernelContext,
    fd: u64,
    iov_ptr: u64,
    iovcnt: u64,
) -> Result<u64, LinuxErrno> {
    if iovcnt > 1024 {
        return Err(LinuxErrno::Einval);
    }
    let mut total_written: u64 = 0;
    for idx in 0..iovcnt {
        let entry_addr = iov_ptr.wrapping_add(idx.wrapping_mul(16));
        let mut raw_iov = [0u8; 16];
        copy_from_buffer(entry_addr, &mut raw_iov).map_err(|_| LinuxErrno::Efault)?;

        let mut base_bytes = [0u8; 8];
        let mut len_bytes = [0u8; 8];
        base_bytes.copy_from_slice(&raw_iov[0..8]);
        len_bytes.copy_from_slice(&raw_iov[8..16]);

        let iov_base = u64::from_le_bytes(base_bytes);
        let iov_len = u64::from_le_bytes(len_bytes);

        if iov_len > 0 {
            let n = sys_write(ctx, fd, iov_base, iov_len)?;
            total_written = total_written.wrapping_add(n);
        }
    }
    Ok(total_written)
}

fn sys_ioctl(ctx: &KernelContext, fd: u64, request: u64, argp: u64) -> Result<u64, LinuxErrno> {
    if fd <= 2 && request == TIOCGWINSZ {
        let winsize: [u8; 8] = [25, 0, 80, 0, 0, 0, 0, 0];
        copy_to_buffer(argp, &winsize).map_err(|_| LinuxErrno::Efault)?;
        return Ok(0);
    }

    let cap = ctx
        .fd_table
        .get(fd as usize)
        .and_then(|c| *c)
        .ok_or(LinuxErrno::Ebadf)?;
    if let CapObject::VfsFile {
        kind: VfsNodeKind::DevFb0,
        ..
    } = cap.object
    {
        if request == FBIOGET_VSCREENINFO || request == FBIOGET_FSCREENINFO {
            let mut vinfo = [0u8; 32];
            vinfo[0..4].copy_from_slice(&ctx.fb.width.to_le_bytes());
            vinfo[4..8].copy_from_slice(&ctx.fb.height.to_le_bytes());
            vinfo[8..12].copy_from_slice(&ctx.fb.width.to_le_bytes());
            vinfo[12..16].copy_from_slice(&ctx.fb.height.to_le_bytes());
            vinfo[24..28].copy_from_slice(&(ctx.fb.bpp as u32).to_le_bytes());
            copy_to_buffer(argp, &vinfo).map_err(|_| LinuxErrno::Efault)?;
            return Ok(0);
        }
    }

    Err(LinuxErrno::Einval)
}

fn sys_mmap(ctx: &mut KernelContext, addr_hint: u64, length: u64) -> Result<u64, LinuxErrno> {
    if length == 0 {
        return Err(LinuxErrno::Einval);
    }
    let page_sz = PAGE_SIZE as u64;
    let num_pages = length.wrapping_add(0xFFF) / page_sz;
    let base_va = if addr_hint != 0 && (addr_hint & 0xFFF) == 0 {
        addr_hint
    } else {
        ctx.next_mmap_va
    };

    for i in 0..num_pages {
        let page_va = base_va + i * page_sz;
        map_user_page(&mut ctx.pmm, page_va).map_err(|_| LinuxErrno::Enomem)?;
    }

    if base_va >= ctx.next_mmap_va {
        ctx.next_mmap_va = base_va + num_pages * page_sz;
    }
    Ok(base_va)
}

fn sys_brk(ctx: &mut KernelContext, new_brk: u64) -> Result<u64, LinuxErrno> {
    if new_brk != 0 && new_brk >= ctx.program_break {
        ctx.program_break = new_brk;
    }
    Ok(ctx.program_break)
}

fn sys_uname(uts_ptr: u64) -> Result<u64, LinuxErrno> {
    let uts = LinuxUtsName::zenith_default();
    let raw_bytes = uts.to_bytes();
    copy_to_buffer(uts_ptr, &raw_bytes).map_err(|_| LinuxErrno::Efault)?;
    Ok(0)
}

fn sys_arch_prctl(code: u64, addr: u64) -> Result<u64, LinuxErrno> {
    match code {
        ARCH_SET_FS => {
            write_msr(Msr::Ia32FsBase, addr);
            Ok(0)
        }
        ARCH_GET_FS => {
            let fs_val = read_msr(Msr::Ia32FsBase);
            copy_to_buffer(addr, &fs_val.to_ne_bytes()).map_err(|_| LinuxErrno::Efault)?;
            Ok(0)
        }
        _ => Err(LinuxErrno::Einval),
    }
}

fn sys_clock_gettime(_clock_id: u64, tp_user_ptr: u64) -> Result<u64, LinuxErrno> {
    let cycles = read_tsc();
    let total_ns = cycles / 2;
    let tv_sec = (total_ns / 1_000_000_000) as i64;
    let tv_nsec = (total_ns % 1_000_000_000) as i64;

    let mut ts_bytes = [0u8; 16];
    ts_bytes[0..8].copy_from_slice(&tv_sec.to_le_bytes());
    ts_bytes[8..16].copy_from_slice(&tv_nsec.to_le_bytes());
    copy_to_buffer(tp_user_ptr, &ts_bytes).map_err(|_| LinuxErrno::Efault)?;
    Ok(0)
}

fn sys_futex(ctx: &mut KernelContext, uaddr: u64, op: u64, val: u32) -> Result<u64, LinuxErrno> {
    let cmd = op & !FUTEX_PRIVATE_FLAG;
    match cmd {
        FUTEX_WAIT => {
            let mut cur_bytes = [0u8; 4];
            copy_from_buffer(uaddr, &mut cur_bytes).map_err(|_| LinuxErrno::Efault)?;
            let cur_val = u32::from_le_bytes(cur_bytes);
            if cur_val != val {
                return Err(LinuxErrno::Eagain);
            }
            Ok(0)
        }
        FUTEX_WAKE => {
            let mut woken = 0u64;
            for task in ctx.tasks.iter_mut() {
                if task.state == TaskState::BlockedOnFutex(uaddr) && woken < (val as u64) {
                    task.state = TaskState::Ready;
                    woken += 1;
                }
            }
            Ok(woken)
        }
        _ => Err(LinuxErrno::Einval),
    }
}

fn sys_clone(ctx: &mut KernelContext) -> Result<u64, LinuxErrno> {
    let child_pid = ctx.next_pid;
    for task in ctx.tasks.iter_mut() {
        if task.state == TaskState::Unused {
            task.pid = child_pid;
            task.ppid = ctx.current_pid;
            task.state = TaskState::Exited(0);
            ctx.next_pid += 1;
            return Ok(child_pid);
        }
    }
    Err(LinuxErrno::Eagain)
}

fn sys_wait4(
    ctx: &mut KernelContext,
    target_pid: u64,
    wstatus_ptr: u64,
) -> Result<u64, LinuxErrno> {
    for task in ctx.tasks.iter_mut() {
        if let TaskState::Exited(code) = task.state {
            if target_pid == u64::MAX || task.pid == target_pid {
                let reaped_pid = task.pid;
                task.state = TaskState::Unused;
                if wstatus_ptr != 0 {
                    let status_word: i32 = (code & 0xFF) << 8;
                    copy_to_buffer(wstatus_ptr, &status_word.to_le_bytes())
                        .map_err(|_| LinuxErrno::Efault)?;
                }
                return Ok(reaped_pid);
            }
        }
    }
    Err(LinuxErrno::Echild)
}
