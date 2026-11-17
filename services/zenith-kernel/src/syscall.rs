// services/zenith-kernel/src/syscall.rs
//! ZenithOS System Call Dispatcher (100% Safe Rust).
//!
//! Implements the primary Linux System V AMD64 ABI syscall table and provides
//! the translation hook for the secondary Windows NT (`ntdll`) personality.

use zenith_frame::{Error, UserContextApi};

/// Execution personality of the calling thread/process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AbiPersonality {
    /// Native Linux ELF64 ABI (System V AMD64).
    LinuxPosix,
    /// Windows NT (`ntdll`) syscall ABI translated to POSIX primitives.
    WindowsNt,
}

/// Linux x86_64 `read` syscall number.
pub const SYS_READ: usize = 0;
/// Linux x86_64 `write` syscall number.
pub const SYS_WRITE: usize = 1;
/// Linux x86_64 `close` syscall number.
pub const SYS_CLOSE: usize = 3;
/// Linux x86_64 `brk` syscall number.
pub const SYS_BRK: usize = 12;
/// Linux x86_64 `getpid` syscall number.
pub const SYS_GETPID: usize = 39;
/// Linux x86_64 `execve` syscall number.
pub const SYS_EXECVE: usize = 59;
/// Linux x86_64 `exit` syscall number.
pub const SYS_EXIT: usize = 60;
/// Linux x86_64 `uname` syscall number.
pub const SYS_UNAME: usize = 63;
/// Linux x86_64 `arch_prctl` syscall number.
pub const SYS_ARCH_PRCTL: usize = 158;
/// Linux x86_64 `exit_group` syscall number.
pub const SYS_EXIT_GROUP: usize = 231;
/// Linux x86_64 `openat` syscall number.
pub const SYS_OPENAT: usize = 257;

/// `arch_prctl` sub-code to set the 64-bit `FS` segment base for Thread-Local Storage.
pub const ARCH_SET_FS: usize = 0x1002;

/// Top-level stateless syscall entry point invoked by the Framework trap handler.
///
/// Dispatches based on the active [`AbiPersonality`] without allocating on the heap
/// and writes the POSIX or NTSTATUS return code back into `ctx`.
///
/// # Safety Invariants
/// Relies strictly on `UserContextApi` safe trait methods; 100% safe Rust.
pub fn dispatch_syscall<C: UserContextApi>(ctx: &mut C, personality: AbiPersonality) {
    let num = ctx.syscall_num();
    let args = ctx.syscall_args();

    let result = match personality {
        AbiPersonality::LinuxPosix => dispatch_linux_syscall(num, args),
        AbiPersonality::WindowsNt => dispatch_winnt_syscall(num, args),
    };

    let ret_val = match result {
        Ok(val) => val,
        Err(err) => err.to_posix_errno() as usize,
    };

    ctx.set_syscall_ret(ret_val);
}

/// Dispatches a Linux x86_64 syscall using stack-allocated arguments.
///
/// # Safety Invariants
/// All user-space pointers in `args` are treated as opaque `usize` tokens and must be
/// validated through `VmSpace` reader/writer capabilities before dereferencing.
pub fn dispatch_linux_syscall(num: usize, args: [usize; 6]) -> Result<usize, Error> {
    match num {
        SYS_READ => sys_read(args[0], args[1], args[2]),
        SYS_WRITE => sys_write(args[0], args[1], args[2]),
        SYS_CLOSE => sys_close(args[0]),
        SYS_BRK => sys_brk(args[0]),
        SYS_GETPID => sys_getpid(),
        SYS_EXECVE => sys_execve(args[0], args[1], args[2]),
        SYS_EXIT | SYS_EXIT_GROUP => sys_exit(args[0] as i32),
        SYS_UNAME => sys_uname(args[0]),
        SYS_ARCH_PRCTL => sys_arch_prctl(args[0], args[1]),
        SYS_OPENAT => sys_openat(args[0], args[1], args[2], args[3]),
        _ => {
            // TODO: requires full Linux x86_64 syscall table implementation
            Err(Error::NotImplemented)
        }
    }
}

/// Handles Linux `sys_read(fd, buf_ptr, count)`.
///
/// # Safety Invariants
/// Does not dereference `buf_ptr` directly; delegates to safe VFS/VmSpace APIs.
pub fn sys_read(_fd: usize, _buf_ptr: usize, _count: usize) -> Result<usize, Error> {
    // TODO: requires vfs_read and VmSpace user-buffer writer capability
    Err(Error::NotImplemented)
}

/// Handles Linux `sys_write(fd, buf_ptr, count)`.
///
/// # Safety Invariants
/// Does not dereference `buf_ptr` directly; delegates to safe VFS/VmSpace APIs.
pub fn sys_write(_fd: usize, _buf_ptr: usize, _count: usize) -> Result<usize, Error> {
    // TODO: requires vfs_write and VmSpace user-buffer reader capability
    Err(Error::NotImplemented)
}

/// Handles stateless Linux `sys_close(fd)` fallback when no process context is bound.
///
/// # Safety Invariants
/// Returns `Err(Error::NotImplemented)` unless invoked via stateful `KernelServices`.
pub fn sys_close(_fd: usize) -> Result<usize, Error> {
    // TODO: requires current_cpu_process_context when called outside KernelServices
    Err(Error::NotImplemented)
}

/// Handles stateless Linux `sys_brk(addr)` fallback when no process context is bound.
///
/// # Safety Invariants
/// Returns `Err(Error::NotImplemented)` unless invoked via stateful `KernelServices`.
pub fn sys_brk(_addr: usize) -> Result<usize, Error> {
    // TODO: requires current_cpu_process_context when called outside KernelServices
    Err(Error::NotImplemented)
}

/// Handles stateless Linux `sys_getpid()` fallback when no process context is bound.
///
/// # Safety Invariants
/// Returns `Err(Error::NotImplemented)` unless invoked via stateful `KernelServices`.
pub fn sys_getpid() -> Result<usize, Error> {
    // TODO: requires current_cpu_process_context when called outside KernelServices
    Err(Error::NotImplemented)
}

/// Handles Linux `sys_execve(filename_ptr, argv_ptr, envp_ptr)`.
///
/// # Safety Invariants
/// Validates ELF headers and builds a new `VmSpace` without raw pointer manipulation.
pub fn sys_execve(
    _filename_ptr: usize,
    _argv_ptr: usize,
    _envp_ptr: usize,
) -> Result<usize, Error> {
    // TODO: requires process_execve and ELF64 loader service
    Err(Error::NotImplemented)
}

/// Handles stateless Linux `sys_exit(status)` / `sys_exit_group(status)`.
///
/// # Safety Invariants
/// Returns `Err(Error::NotImplemented)` unless invoked via stateful `KernelServices`.
pub fn sys_exit(_status: i32) -> Result<usize, Error> {
    // TODO: requires current_cpu_process_context when called outside KernelServices
    Err(Error::NotImplemented)
}

/// Handles Linux `sys_uname(utsname_ptr)`.
///
/// # Safety Invariants
/// Does not dereference `utsname_ptr`; requires `VmSpace` user copy primitive.
pub fn sys_uname(_utsname_ptr: usize) -> Result<usize, Error> {
    // TODO: requires vmspace_copy_to_user to write utsname structure safely
    Err(Error::NotImplemented)
}

/// Handles Linux `sys_arch_prctl(code, addr)` for Thread-Local Storage (`FS` base).
///
/// # Safety Invariants
/// Rejects unknown sub-codes with `Err(Error::InvalidArgument)`.
pub fn sys_arch_prctl(code: usize, _addr: usize) -> Result<usize, Error> {
    if code != ARCH_SET_FS {
        return Err(Error::InvalidArgument);
    }
    // TODO: requires arch_x86_64_write_fsbase_msr in zenith-frame
    Err(Error::NotImplemented)
}

/// Handles Linux `sys_openat(dirfd, pathname_ptr, flags, mode)`.
///
/// # Safety Invariants
/// Does not dereference `pathname_ptr` directly; requires `VmSpace` C-string reader.
pub fn sys_openat(
    _dirfd: usize,
    _pathname_ptr: usize,
    _flags: usize,
    _mode: usize,
) -> Result<usize, Error> {
    // TODO: requires vmspace_read_cstr_from_user and VFS path resolver
    Err(Error::NotImplemented)
}

/// Translates Windows NT (`ntdll`) syscalls to ZenithOS POSIX syscall equivalents
/// following the Wine `ntdll` translation pattern.
///
/// # Safety Invariants
/// Translates handles and object attributes purely in safe Rust before invoking VFS.
pub fn dispatch_winnt_syscall(nt_syscall_num: usize, args: [usize; 6]) -> Result<usize, Error> {
    // UNCERTAIN: Windows NT syscall numbers (e.g., NtCreateFile = 0x0055) — needs verification
    // against specific Windows 10/11 build SSDT as NT syscall numbers vary across builds.
    const NT_CREATE_FILE_WIN10_1809: usize = 0x0055;

    match nt_syscall_num {
        NT_CREATE_FILE_WIN10_1809 => nt_create_file_to_posix(args),
        _ => {
            // TODO: requires Wine-style SSDT syscall translation table
            Err(Error::NotImplemented)
        }
    }
}

/// Translates `NtCreateFile` into an equivalent POSIX VFS open operation.
///
/// # Safety Invariants
/// Validates `OBJECT_ATTRIBUTES` through safe `VmSpace` reads before path resolution.
pub fn nt_create_file_to_posix(_args: [usize; 6]) -> Result<usize, Error> {
    // TODO: requires OBJECT_ATTRIBUTES reader and vfs_open_posix
    Err(Error::NotImplemented)
}
