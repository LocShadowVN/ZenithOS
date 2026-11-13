use core::arch::{asm, global_asm};

use crate::ostd::cpu::KERNEL_INTR_STACK;

#[repr(u32)]
#[derive(Debug, Clone, Copy)]
pub enum Msr {
    Ia32Efer = 0xC000_0080,
    Ia32Star = 0xC000_0081,
    Ia32Lstar = 0xC000_0082,
    Ia32SfMask = 0xC000_0084,
    Ia32FsBase = 0xC000_0100,
    Ia32GsBase = 0xC000_0101,
    Ia32KernelGsBase = 0xC000_0102,
}

#[inline(always)]
pub fn write_msr(msr: Msr, value: u64) {
    let low = value as u32;
    let high = (value >> 32) as u32;
    // SAFETY:
    // Writes architectural MSRs at CPL=0.
    unsafe {
        asm!(
            "wrmsr",
            in("ecx") msr as u32,
            in("eax") low,
            in("edx") high,
            options(nostack, preserves_flags)
        );
    }
}

#[inline(always)]
pub fn read_msr(msr: Msr) -> u64 {
    let low: u32;
    let high: u32;
    // SAFETY:
    // Reads architectural MSRs at CPL=0.
    unsafe {
        asm!(
            "rdmsr",
            in("ecx") msr as u32,
            out("eax") low,
            out("edx") high,
            options(nostack, preserves_flags)
        );
    }
    ((high as u64) << 32) | (low as u64)
}

#[repr(C, align(64))]
pub struct PerCpuData {
    pub kernel_rsp: u64,
    pub user_rsp_scratch: u64,
    pub kernel_return_rsp: u64,
}

static mut BSP_PER_CPU: PerCpuData = PerCpuData {
    kernel_rsp: 0,
    user_rsp_scratch: 0,
    kernel_return_rsp: 0,
};

#[repr(C, align(16))]
#[derive(Debug, Clone, Copy, Default)]
pub struct TrapFrame {
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub rbp: u64,
    pub rbx: u64,
    pub r11: u64,
    pub r10: u64,
    pub r9: u64,
    pub r8: u64,
    pub rax: u64,
    pub rcx: u64,
    pub rdx: u64,
    pub rsi: u64,
    pub rdi: u64,
    pub rsp: u64,
}

global_asm!(
    ".global zenith_syscall_trampoline",
    "zenith_syscall_trampoline:",
    "swapgs",
    "mov gs:[0x08], rsp",
    "mov rsp, gs:[0x00]",
    "push qword ptr gs:[0x08]",
    "push rdi",
    "push rsi",
    "push rdx",
    "push rcx",
    "push rax",
    "push r8",
    "push r9",
    "push r10",
    "push r11",
    "push rbx",
    "push rbp",
    "push r12",
    "push r13",
    "push r14",
    "push r15",
    "mov rdi, rsp",
    "call zenith_syscall_entry_from_asm",
    "pop r15",
    "pop r14",
    "pop r13",
    "pop r12",
    "pop rbp",
    "pop rbx",
    "pop r11",
    "pop r10",
    "pop r9",
    "pop r8",
    "pop rax",
    "pop rcx",
    "pop rdx",
    "pop rsi",
    "pop rdi",
    "pop rsp",
    "swapgs",
    "sysretq",
    ".global zenith_enter_user_space_asm",
    "zenith_enter_user_space_asm:",
    "push rbp",
    "push rbx",
    "push r12",
    "push r13",
    "push r14",
    "push r15",
    "mov gs:[0x10], rsp",
    "mov rcx, rdi",
    "mov rsp, rsi",
    "mov r11, 0x002",
    "xor rax, rax",
    "xor rbx, rbx",
    "xor rdx, rdx",
    "xor rsi, rsi",
    "xor rdi, rdi",
    "xor rbp, rbp",
    "xor r8, r8",
    "xor r9, r9",
    "xor r10, r10",
    "xor r12, r12",
    "xor r13, r13",
    "xor r14, r14",
    "xor r15, r15",
    "swapgs",
    "sysretq",
    ".global zenith_exit_user_space_asm",
    "zenith_exit_user_space_asm:",
    "mov eax, edi",
    "mov rsp, gs:[0x10]",
    "pop r15",
    "pop r14",
    "pop r13",
    "pop r12",
    "pop rbx",
    "pop rbp",
    "ret",
);

extern "C" {
    fn zenith_syscall_trampoline();
    fn zenith_enter_user_space_asm(entry_rip: u64, user_rsp: u64) -> i32;
    fn zenith_exit_user_space_asm(exit_code: i32) -> !;
}

pub fn init_syscall_msrs() {
    // SAFETY:
    // Initializes BSP per-CPU data and SYSCALL/SYSRETQ MSRs during boot.
    unsafe {
        let kstack_top = (&raw const KERNEL_INTR_STACK.0 as u64) + 16384;
        let per_cpu_ptr = &raw mut BSP_PER_CPU;
        (*per_cpu_ptr).kernel_rsp = kstack_top;
        let per_cpu_addr = per_cpu_ptr as u64;

        write_msr(Msr::Ia32GsBase, per_cpu_addr);
        write_msr(Msr::Ia32KernelGsBase, per_cpu_addr);
    }

    let efer = read_msr(Msr::Ia32Efer);
    write_msr(Msr::Ia32Efer, efer | 1);

    let star: u64 = (0x0010u64 << 48) | (0x0008u64 << 32);
    write_msr(Msr::Ia32Star, star);

    write_msr(
        Msr::Ia32Lstar,
        zenith_syscall_trampoline as *const () as u64,
    );
    write_msr(Msr::Ia32SfMask, 0x0600);
}

pub fn enter_user_space(entry_rip: u64, user_rsp: u64) -> i32 {
    // SAFETY:
    // `entry_rip` and `user_rsp` point to valid user-mapped pages prepared by the ELF64 loader.
    unsafe { zenith_enter_user_space_asm(entry_rip, user_rsp) }
}

pub fn return_to_kernel_from_exit(exit_code: i32) -> ! {
    // SAFETY:
    // Invoked from syscall context after `swapgs` has restored the kernel GS base.
    unsafe { zenith_exit_user_space_asm(exit_code) }
}
