// framework/zenith-frame/src/lib.rs
#![no_std]
// TCB crate: unsafe is permitted ONLY inside this crate when encapsulating hardware primitives.
#![allow(unsafe_code)]
#![deny(missing_docs)]

//! ZenithOS Framework (Trusted Computing Base - TCB).
//!
//! This crate encapsulates all architecture-specific hardware state, page tables,
//! MMIO regions, and trap contexts, exposing 100% safe trait and capability APIs
//! to upper-layer OS Services.

pub mod memory;
pub mod sched;
pub mod trap;
pub mod vm;

/// Standard error codes bridging Framework capabilities and POSIX/NT error numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// Operation or hardware primitive is not yet implemented (`-ENOSYS` = 38).
    NotImplemented,
    /// Invalid memory address or out-of-bounds access (`-EFAULT` = 14).
    InvalidAddress,
    /// Invalid argument passed to capability or syscall (`-EINVAL` = 22).
    InvalidArgument,
    /// Permission denied (`-EACCES` = 13).
    PermissionDenied,
    /// Out of physical frames or task slots (`-ENOMEM` = 12).
    OutOfMemory,
    /// Requested resource or task ID was not found (`-ENOENT` = 2).
    NotFound,
}

impl Error {
    /// Converts a Framework [`Error`] into a negative Linux POSIX errno value.
    ///
    /// # Safety Invariants
    /// Pure value conversion; no memory or hardware side effects.
    pub const fn to_posix_errno(self) -> isize {
        match self {
            Error::NotImplemented => -38,   // -ENOSYS
            Error::InvalidAddress => -14,   // -EFAULT
            Error::InvalidArgument => -22,  // -EINVAL
            Error::PermissionDenied => -13, // -EACCES
            Error::OutOfMemory => -12,      // -ENOMEM
            Error::NotFound => -2,          // -ENOENT
        }
    }
}

/// Safe capability representing a validated, bounds-checked MMIO region.
///
/// Drivers in `zenith-drivers` receive an `IoMem` instance from the bus manager
/// and perform register I/O without writing `unsafe` blocks.
#[derive(Debug)]
pub struct IoMem {
    phys_base: usize,
    size: usize,
}

impl IoMem {
    /// Creates a new `IoMem` capability for a verified physical address range.
    ///
    /// # Safety Invariants
    /// The caller (within the Framework bus enumerator) must ensure `phys_base..phys_base + size`
    /// belongs to a valid device MMIO BAR and does not alias normal RAM frames.
    pub const fn new_checked(phys_base: usize, size: usize) -> Option<Self> {
        if size == 0 || phys_base.checked_add(size).is_none() {
            return None;
        }
        Some(Self { phys_base, size })
    }

    /// Returns the size in bytes of this MMIO region.
    ///
    /// # Safety Invariants
    /// Guaranteed not to overflow address space bounds.
    pub const fn size(&self) -> usize {
        self.size
    }

    /// Reads a 32-bit value from `offset` within this MMIO region.
    ///
    /// # Safety Invariants
    /// Verifies that `offset + 4 <= self.size` and `offset` is 4-byte aligned
    /// before issuing any hardware bus transaction.
    pub fn read_u32(&self, offset: usize) -> Result<u32, Error> {
        if offset & (core::mem::align_of::<u32>() - 1) != 0 {
            return Err(Error::InvalidArgument);
        }
        let end = offset
            .checked_add(core::mem::size_of::<u32>())
            .ok_or(Error::InvalidAddress)?;
        if end > self.size {
            return Err(Error::InvalidAddress);
        }

        let _target_paddr = self.phys_base + offset;
        // TODO: requires arch_mmio_read_u32 virtual mapping in kernel page table
        Err(Error::NotImplemented)
    }

    /// Writes a 32-bit value to `offset` within this MMIO region.
    ///
    /// # Safety Invariants
    /// Verifies alignment and bounds (`offset + 4 <= self.size`) prior to MMIO write.
    pub fn write_u32(&self, offset: usize, _val: u32) -> Result<(), Error> {
        if offset & (core::mem::align_of::<u32>() - 1) != 0 {
            return Err(Error::InvalidArgument);
        }
        let end = offset
            .checked_add(core::mem::size_of::<u32>())
            .ok_or(Error::InvalidAddress)?;
        if end > self.size {
            return Err(Error::InvalidAddress);
        }

        let _target_paddr = self.phys_base + offset;
        // TODO: requires arch_mmio_write_u32 virtual mapping in kernel page table
        Err(Error::NotImplemented)
    }
}

/// Safe interface for inspecting and mutating user-space CPU state during traps/syscalls.
pub trait UserContextApi {
    /// Returns the system call number passed by user space (e.g., `rax` on x86_64).
    ///
    /// # Safety Invariants
    /// Reads from a saved trap frame in kernel stack memory.
    fn syscall_num(&self) -> usize;

    /// Returns the up-to-6 system call arguments (`rdi`, `rsi`, `rdx`, `r10`, `r8`, `r9` on x86_64).
    ///
    /// # Safety Invariants
    /// Returns values by copy on the stack; no heap allocation.
    fn syscall_args(&self) -> [usize; 6];

    /// Sets the return value register (`rax` on x86_64) before returning to user space.
    ///
    /// # Safety Invariants
    /// Modifies only the saved general-purpose return register of the trap frame.
    fn set_syscall_ret(&mut self, ret: usize);
}
