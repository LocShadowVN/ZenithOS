pub mod cpu;
pub mod fb;
pub mod mmu;
pub mod pci;
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
pub use pci::{read_cpu_brand, scan_pci_bus, PciDeviceInfo, RtcDateTime};
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
    // Writes 32-bit value to PCI config space (0xCF8) or QEMU debug exit (0xF4) at CPL=0.
    unsafe {
        asm!(
            "out dx, eax",
            in("dx") port,
            in("eax") val,
            options(nostack, preserves_flags)
        );
    }
}

#[inline(always)]
pub fn inl(port: u16) -> u32 {
    let val: u32;
    // SAFETY:
    // Reads 32-bit value from PCI config data port (0xCFC) at CPL=0.
    unsafe {
        asm!(
            "in eax, dx",
            out("eax") val,
            in("dx") port,
            options(nostack, preserves_flags)
        );
    }
    val
}

#[inline]
pub fn poll_ps2_scancode() -> Option<u8> {
    if (inb(0x64) & 0x01) != 0 {
        Some(inb(0x60))
    } else {
        None
    }
}

/// Translates PC AT PS/2 Set 1 make-codes (and UART serial input) into ASCII characters.
pub fn poll_input_char() -> Option<u8> {
    // 1. Check PS/2 keyboard controller (port 0x64 / 0x60)
    if (inb(0x64) & 0x01) != 0 {
        let sc = inb(0x60);
        // Ignore key release (break code has bit 7 set)
        if (sc & 0x80) == 0 {
            let ch = match sc {
                0x02 => b'1',
                0x03 => b'2',
                0x04 => b'3',
                0x05 => b'4',
                0x06 => b'5',
                0x07 => b'6',
                0x08 => b'7',
                0x09 => b'8',
                0x0A => b'9',
                0x0B => b'0',
                0x0C => b'-',
                0x0E => 0x08, // Backspace
                0x10 => b'q',
                0x11 => b'w',
                0x12 => b'e',
                0x13 => b'r',
                0x14 => b't',
                0x15 => b'y',
                0x16 => b'u',
                0x17 => b'i',
                0x18 => b'o',
                0x19 => b'p',
                0x1C => b'\n', // Enter
                0x1E => b'a',
                0x1F => b's',
                0x20 => b'd',
                0x21 => b'f',
                0x22 => b'g',
                0x23 => b'h',
                0x24 => b'j',
                0x25 => b'k',
                0x26 => b'l',
                0x2C => b'z',
                0x2D => b'x',
                0x2E => b'c',
                0x2F => b'v',
                0x30 => b'b',
                0x31 => b'n',
                0x32 => b'm',
                0x34 => b'.',
                0x35 => b'/',
                0x39 => b' ',
                _ => 0,
            };
            if ch != 0 {
                return Some(ch);
            }
        }
    }

    // 2. Also check COM1 UART receiver buffer (port 0x3F8)
    if (inb(0x3F8 + 5) & 0x01) != 0 {
        let b = inb(0x3F8);
        if b == b'\r' {
            return Some(b'\n');
        }
        if b == 0x7F {
            return Some(0x08);
        }
        return Some(b);
    }

    None
}

/// Pulses the 8042 keyboard controller CPU reset line (0xFE) to reboot the machine.
pub fn reboot_machine() -> ! {
    outb(0x64, 0xFE);
    loop {
        // SAFETY:
        // Halts the CPU while waiting for the hardware reset pulse.
        unsafe {
            asm!("hlt", options(nostack, preserves_flags));
        }
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

/// Signals QEMU `isa-debug-exit` if present (in CI). If running in a normal VM or real PC,
/// this function returns normally so the OS can enter its interactive desktop & shell loop!
#[inline]
pub fn notify_ci_success() {
    outl(0xF4, QemuExitCode::Success as u32);
}

pub fn exit_qemu(exit_code: QemuExitCode) -> ! {
    outl(0xF4, exit_code as u32);
    loop {
        // SAFETY:
        // Halts the processor on fatal error.
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
