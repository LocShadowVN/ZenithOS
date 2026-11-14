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
    kprintln!("ZenithOS 1.1.0 - x86_64 Framekernel (Z-Cap Security & ZenithFS)");
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
    kprintln!("[ZENITH-OSTD] GDT, 64-bit TSS, IDT, SSE/SIMD, and SYSCALL MSRs initialized.");

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
        "[ZENITH-PCI] Enumerated {} PCIe hardware controllers on Bus 0.",
        ctx.pci_count
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

    // Verify `/home/user/zenith.txt` was dynamically created on ZenithFS with valid FNV-1a checksum
    let user_ino_idx = match ctx.zfs.find_inode_by_path(b"/home/user/zenith.txt") {
        Some(idx) => idx,
        None => {
            kprintln!("[ZENITH-TEST][FAIL] /home/user/zenith.txt was not created on ZenithFS.");
            ostd::exit_qemu(ostd::QemuExitCode::Failed);
        }
    };
    if !ctx
        .zfs
        .verify_inode_checksum((user_ino_idx + 1) as u32, b"ZFS1_USER_FILE_OK")
        || ctx.zcap_blocks_count != 1
    {
        kprintln!("[ZENITH-TEST][FAIL] ZenithFS FNV-1a checksum or Z-Cap guard check failed.");
        ostd::exit_qemu(ostd::QemuExitCode::Failed);
    }

    ctx.render_terminal_bytes(b"user@zenith-os:~$ cat /home/user/zenith.txt\n");
    ctx.render_terminal_bytes(b"ZFS1_USER_FILE_OK (FNV-1a Checksum Verified)\n");
    ctx.render_terminal_bytes(b"user@zenith-os:~$ _\n");

    kprintln!(
        "[ZENITH-ZCAP] Verified passwordless file creation & blocked {} unauthorized system write.",
        ctx.zcap_blocks_count
    );

    kprintln!("----------------------------------------------------------------");
    kprintln!("[ZENITH-CI] ALL Z-CAP SECURITY, ZENITHFS & DEBIAN 13 TESTS PASS");
    kprintln!("----------------------------------------------------------------");

    ostd::exit_qemu(ostd::QemuExitCode::Success);
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    kprintln!("\n[ZENITH-PANIC] Kernel panic: {}", info);
    ostd::exit_qemu(ostd::QemuExitCode::Failed);
}
