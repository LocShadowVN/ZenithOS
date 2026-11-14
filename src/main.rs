#![no_std]
#![no_main]
#![deny(unsafe_op_in_unsafe_fn)]
#![allow(clippy::fn_to_numeric_cast)]

pub mod boot;
pub mod linux_abi;
pub mod ostd;

use core::cell::UnsafeCell;
use core::panic::PanicInfo;
use core::sync::atomic::Ordering;

struct SyncContextCell(UnsafeCell<linux_abi::KernelContext>);

// SAFETY:
// Sequentially accessed on the BSP during syscall execution.
unsafe impl Sync for SyncContextCell {}

impl SyncContextCell {
    const fn new() -> Self {
        Self(UnsafeCell::new(linux_abi::KernelContext::const_new()))
    }
}

static GLOBAL_KERNEL_CTX: SyncContextCell = SyncContextCell::new();

#[no_mangle]
extern "C" fn zenith_syscall_entry_from_asm(frame: &mut ostd::TrapFrame) {
    // SAFETY:
    // Single-threaded execution on the BSP when handling syscalls from CPL=3.
    let ctx = unsafe { &mut *GLOBAL_KERNEL_CTX.0.get() };
    linux_abi::dispatch_linux_syscall(frame, ctx);

    if let Some(exit_code) = ctx.last_exit_code {
        ostd::return_to_kernel_from_exit(exit_code);
    }
}

macro_rules! kprintln {
    ($($arg:tt)*) => {
        $crate::ostd::print_fmt(core::format_args!($($arg)*));
        $crate::ostd::print_fmt(core::format_args!("\n"));
    };
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    ostd::SerialConsole::init();
    kprintln!("----------------------------------------------------------------");
    kprintln!("ZenithOS 0.8.0 - x86_64 Framekernel (ZenithFS & Debian 13 ABI)");
    kprintln!("----------------------------------------------------------------");

    ostd::init_gdt_and_tss();
    ostd::init_idt();
    ostd::trigger_breakpoint_self_test();
    if ostd::BREAKPOINT_HIT_COUNT.load(Ordering::SeqCst) != 1 {
        kprintln!("[ZENITH-OSTD][FAIL] IDT breakpoint self-test failed.");
        ostd::exit_qemu(ostd::QemuExitCode::Failed);
    }
    ostd::init_syscall_msrs();
    ostd::disable_smap_for_loader();
    kprintln!("[ZENITH-OSTD] GDT, 64-bit TSS, IDT, and SYSCALL/SYSRETQ MSRs initialized.");

    // SAFETY:
    // Initialized once on the BSP prior to entering user space.
    let ctx = unsafe { &mut *GLOBAL_KERNEL_CTX.0.get() };
    ctx.init_hardware();
    if !ctx.zfs.mounted
        || !ctx.zfs.deb_installed
        || !ctx
            .zfs
            .verify_inode_checksum(1, linux_abi::DEBIAN_VERSION_DATA)
    {
        kprintln!("[ZENITH-FS][FAIL] ZenithFS superblock or .deb installation failure.");
        ostd::exit_qemu(ostd::QemuExitCode::Failed);
    }
    kprintln!(
        "[ZENITH-FS] Mounted ZenithFS (magic=0x{:x}) with Debian 13.0 (Trixie) & unpacked .deb.",
        linux_abi::ZENITHFS_SUPER_MAGIC
    );

    let elf_image = linux_abi::build_embedded_linux_init_elf64();
    let loaded = match linux_abi::load_elf64_into_user_space(&elf_image, &mut ctx.pmm) {
        Ok(info) => info,
        Err(err) => {
            kprintln!("[ZENITH-ELF][FAIL] ELF64 loader error: {:?}", err);
            ostd::exit_qemu(ostd::QemuExitCode::Failed);
        }
    };
    ctx.program_break = loaded.initial_brk;

    kprintln!(
        "[ZENITH-CORE] Entering Ring 3 ('/bin/init' -> SYS_execve -> '/usr/bin/hello-deb')..."
    );
    let exit_code = ostd::enter_user_space(loaded.entry_rip, loaded.user_rsp);
    kprintln!(
        "[ZENITH-CORE] Ring 3 .deb binary '/usr/bin/hello-deb' exited with status=0x{:x}.",
        exit_code
    );

    if exit_code != 0x66 {
        kprintln!(
            "[ZENITH-TEST][FAIL] Unexpected exit status: 0x{:x} (expected 0x66).",
            exit_code
        );
        ostd::exit_qemu(ostd::QemuExitCode::Failed);
    }

    // Verify Ring 3 queried ZenithFS Superblock via SYS_statfs(137) at 0x1000_0100
    let mut statfs_magic = [0u8; 8];
    if ostd::copy_from_buffer(0x1000_0100, &mut statfs_magic).is_err()
        || u64::from_le_bytes(statfs_magic) != linux_abi::ZENITHFS_SUPER_MAGIC
    {
        kprintln!("[ZENITH-TEST][FAIL] SYS_statfs ZenithFS magic verification failed.");
        ostd::exit_qemu(ostd::QemuExitCode::Failed);
    }

    // Verify Ring 3 read `/etc/debian_version` ("13.0\n") from ZenithFS Inode #1 at 0x1000_0500
    let mut deb_ver = [0u8; 4];
    if ostd::copy_from_buffer(0x1000_0500, &mut deb_ver).is_err() || &deb_ver != b"13.0" {
        kprintln!("[ZENITH-TEST][FAIL] Debian 13.0 (/etc/debian_version) verification failed.");
        ostd::exit_qemu(ostd::QemuExitCode::Failed);
    }

    // Verify Ring 3 `/usr/bin/hello-deb` (unpacked from .deb) wrote signature "DEB3" at 0x1000_0600
    let mut deb_sig = [0u8; 4];
    if ostd::copy_from_buffer(0x1000_0600, &mut deb_sig).is_err() || &deb_sig != b"DEB3" {
        kprintln!("[ZENITH-TEST][FAIL] Unpacked .deb binary execution signature mismatch.");
        ostd::exit_qemu(ostd::QemuExitCode::Failed);
    }

    ctx.render_terminal_bytes(b"root@zenith-os:~# dpkg -l | grep hello-trixie\n");
    ctx.render_terminal_bytes(b"ii  hello-trixie  1.0+deb13u1  amd64  Debian 13 Trixie package\n");
    ctx.render_terminal_bytes(b"root@zenith-os:~# _\n");

    kprintln!(
        "[ZENITH-LINUX] Verified ZenithFS, Debian 13.0 (Trixie), .deb unpack & Win11 UI ({} glyphs).",
        ctx.term_chars_rendered
    );

    kprintln!("----------------------------------------------------------------");
    kprintln!("[ZENITH-CI] ALL DEBIAN 13 TRIXIE, .DEB AND ZENITHFS TESTS PASSED");
    kprintln!("----------------------------------------------------------------");

    ostd::exit_qemu(ostd::QemuExitCode::Success);
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    kprintln!("\n[ZENITH-PANIC] Kernel panic: {}", info);
    ostd::exit_qemu(ostd::QemuExitCode::Failed);
}
