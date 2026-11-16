// framework/zenith-frame/src/trap.rs
//! CPU trap frame (`UserContext`) and interrupt/syscall gate initialization.

use crate::{Error, UserContextApi};

/// Saved x86_64 user-space register state on trap/syscall entry.
///
/// UNCERTAIN: UserContext x86_64 hardware iretq/sysretq frame layout — needs verification
/// against the exact assembly stub pushing order in the low-level trap entry point.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct UserContext {
    /// System call number or return value (`rax`).
    pub rax: usize,
    /// Syscall argument 1 (`rdi`).
    pub rdi: usize,
    /// Syscall argument 2 (`rsi`).
    pub rsi: usize,
    /// Syscall argument 3 (`rdx`).
    pub rdx: usize,
    /// Syscall argument 4 (`r10` in Linux x86_64 syscall ABI).
    pub r10: usize,
    /// Syscall argument 5 (`r8`).
    pub r8: usize,
    /// Syscall argument 6 (`r9`).
    pub r9: usize,
    /// User instruction pointer (`rip` saved in `rcx` by `syscall`).
    pub rip: usize,
    /// User stack pointer (`rsp`).
    pub rsp: usize,
    /// User RFLAGS (`rflags` saved in `r11` by `syscall`).
    pub rflags: usize,
}

impl UserContext {
    /// Creates a zeroed `UserContext` suitable for initializing a new user thread.
    ///
    /// # Safety Invariants
    /// Initializes all registers to deterministic zero values on the stack.
    pub const fn new() -> Self {
        Self {
            rax: 0,
            rdi: 0,
            rsi: 0,
            rdx: 0,
            r10: 0,
            r8: 0,
            r9: 0,
            rip: 0,
            rsp: 0,
            rflags: 0x202, // IF (Interrupt Enable) flag set by default
        }
    }

    /// Transfers control from kernel mode to Ring 3 user space using this context.
    ///
    /// # Safety Invariants
    /// Requires hardware `sysretq`/`iretq` assembly routine.
    pub fn enter_userspace(&mut self) -> Result<(), Error> {
        // TODO: requires arch_x86_64_enter_userspace assembly routine
        Err(Error::NotImplemented)
    }
}

impl UserContextApi for UserContext {
    fn syscall_num(&self) -> usize {
        self.rax
    }

    fn syscall_args(&self) -> [usize; 6] {
        [self.rdi, self.rsi, self.rdx, self.r10, self.r8, self.r9]
    }

    fn set_syscall_ret(&mut self, ret: usize) {
        self.rax = ret;
    }
}

/// Initializes the x86_64 Interrupt Descriptor Table (IDT) and `STAR`/`LSTAR`/`FMASK` MSRs.
///
/// # Safety Invariants
/// Must be called once on the bootstrap processor during Phase 1 init.
pub fn init_trap_subsystem() -> Result<(), Error> {
    // TODO: requires arch_x86_64_init_idt_and_syscall_msrs hardware register setup
    Err(Error::NotImplemented)
}
