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
    kprintln!("ZenithOS 0.6.0 - x86_64 Rust Framekernel (Linux ABI Subsystem)");
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
    kprintln!(
        "[ZENITH-FBCON] Linear framebuffer {}x{}x{}bpp initialized (hw={}).",
        ctx.fb.width,
        ctx.fb.height,
        ctx.fb.bpp,
        ctx.fb.is_hardware
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

    kprintln!("[ZENITH-CORE] Entering Ring 3 ('/bin/init' -> SYS_execve -> '/bin/app')...");
    let exit_code = ostd::enter_user_space(loaded.entry_rip, loaded.user_rsp);
    kprintln!(
        "[ZENITH-CORE] Ring 3 process '/bin/app' terminated with status=0x{:x}.",
        exit_code
    );

    if exit_code != 0x66 {
        kprintln!(
            "[ZENITH-TEST][FAIL] Unexpected exit status: 0x{:x} (expected 0x66).",
            exit_code
        );
        ostd::exit_qemu(ostd::QemuExitCode::Failed);
    }

    let mut ev_type_bytes = [0u8; 2];
    if ostd::copy_from_buffer(0x1000_0930, &mut ev_type_bytes).is_err()
        || u16::from_le_bytes(ev_type_bytes) != linux_abi::EV_KEY
        || ctx.evdev_events_read != 1
    {
        kprintln!("[ZENITH-TEST][FAIL] /dev/input/event0 evdev packet verification failed.");
        ostd::exit_qemu(ostd::QemuExitCode::Failed);
    }

    let draw_y = 320.min(ctx.fb.height.saturating_sub(1));
    let vram_px0 = ostd::fb_read_pixel(&ctx.fb, 0, draw_y);
    if ctx.term_chars_rendered < 120 || vram_px0 != 0x00FF_FFFF {
        kprintln!(
            "[ZENITH-TEST][FAIL] fbcon or /dev/fb0 verification failed (chars={}, px0=0x{:x}).",
            ctx.term_chars_rendered,
            vram_px0
        );
        ostd::exit_qemu(ostd::QemuExitCode::Failed);
    }
    kprintln!(
        "[ZENITH-LINUX] Verified fbcon ({} glyphs), epoll, evdev, and /dev/fb0.",
        ctx.term_chars_rendered
    );

    kprintln!("----------------------------------------------------------------");
    kprintln!("[ZENITH-CI] ALL KERNEL AND LINUX ABI VERIFICATION TESTS PASSED.");
    kprintln!("----------------------------------------------------------------");

    ostd::exit_qemu(ostd::QemuExitCode::Success);
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    kprintln!("\n[ZENITH-PANIC] Kernel panic: {}", info);
    ostd::exit_qemu(ostd::QemuExitCode::Failed);
}
