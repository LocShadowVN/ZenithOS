pub mod cpu;
pub mod fb;
pub mod mmu;
pub mod syscall;

use core::arch::asm;
use core::fmt::{self, Write};

pub use cpu::{
    init_gdt_and_tss, init_idt, trigger_breakpoint_self_test, AlignedStack, BREAKPOINT_HIT_COUNT,
};
pub use fb::{fb_draw_pixel, fb_fill_rect, fb_read_pixel, FramebufferInfo, Rect};
pub use mmu::{
    copy_from_buffer, copy_to_buffer, disable_smap_for_loader, flush_tlb, map_user_page,
    BitmapFrameAllocator, PageFlags, PageTable, PAGE_SIZE,
};
pub use syscall::{
    enter_user_space, init_syscall_msrs, read_msr, return_to_kernel_from_exit, write_msr, Msr,
    PerCpuData, TrapFrame,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OstdError {
    OutOfPhysicalFrames,
    InvalidFrameIndex,
    UnalignedAddress,
    PageTableWalkFailed,
    InvalidUserPointer,
    BootInfoMissing,
}

#[inline(always)]
pub fn outb(port: u16, val: u8) {
    // SAFETY:
    // Executes port I/O at CPL=0 on standard PC architecture ports.
    unsafe {
        asm!(
            "out dx, al",
            in("dx") port,
            in("al") val,
            options(nostack, preserves_flags)
        );
    }
}

#[inline(always)]
pub fn inb(port: u16) -> u8 {
    let val: u8;
    // SAFETY:
    // Executes port I/O at CPL=0 on standard PC architecture ports.
    unsafe {
        asm!(
            "in al, dx",
            out("al") val,
            in("dx") port,
            options(nostack, preserves_flags)
        );
    }
    val
}

#[inline(always)]
pub fn outl(port: u16, val: u32) {
    // SAFETY:
    // Writes 32-bit value to the QEMU isa-debug-exit port (0xF4) at CPL=0.
    unsafe {
        asm!(
            "out dx, eax",
            in("dx") port,
            in("eax") val,
            options(nostack, preserves_flags)
        );
    }
}

#[inline]
pub fn poll_ps2_scancode() -> Option<u8> {
    if (inb(0x64) & 0x01) != 0 {
        Some(inb(0x60))
    } else {
        None
    }
}

#[inline(always)]
pub fn read_tsc() -> u64 {
    let low: u32;
    let high: u32;
    // SAFETY:
    // Reads the architectural Time Stamp Counter register without memory side effects.
    unsafe {
        asm!(
            "rdtsc",
            out("eax") low,
            out("edx") high,
            options(nostack, preserves_flags)
        );
    }
    ((high as u64) << 32) | (low as u64)
}

#[repr(u32)]
#[derive(Debug, Clone, Copy)]
pub enum QemuExitCode {
    Success = 0x10,
    Failed = 0x11,
}

pub fn exit_qemu(exit_code: QemuExitCode) -> ! {
    outl(0xF4, exit_code as u32);
    loop {
        // SAFETY:
        // Halts the processor at CPL=0 if the hypervisor has not yet terminated.
        unsafe {
            asm!("hlt", options(nostack, preserves_flags));
        }
    }
}

pub struct SerialConsole;

impl SerialConsole {
    const COM1: u16 = 0x3F8;

    pub fn init() {
        outb(Self::COM1 + 1, 0x00);
        outb(Self::COM1 + 3, 0x80);
        outb(Self::COM1, 0x03);
        outb(Self::COM1 + 1, 0x00);
        outb(Self::COM1 + 3, 0x03);
        outb(Self::COM1 + 2, 0xC7);
        outb(Self::COM1 + 4, 0x0B);
    }

    #[inline]
    pub fn write_byte(byte: u8) {
        while (inb(Self::COM1 + 5) & 0x20) == 0 {
            core::hint::spin_loop();
        }
        outb(Self::COM1, byte);
    }

    pub fn write_bytes(bytes: &[u8]) {
        for &b in bytes {
            if b == b'\n' {
                Self::write_byte(b'\r');
            }
            Self::write_byte(b);
        }
    }
}

impl Write for SerialConsole {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        Self::write_bytes(s.as_bytes());
        Ok(())
    }
}

pub fn print_fmt(args: fmt::Arguments<'_>) {
    let mut console = SerialConsole;
    let _ = console.write_fmt(args);
}
