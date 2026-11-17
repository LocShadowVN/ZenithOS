// services/zenith-kernel/src/lib.rs
#![no_std]
// OS Services layer: 100% safe Rust enforced across the entire crate.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! ZenithOS Safe Kernel Services.
//!
//! Coordinates memory initialization, process address spaces, scheduling, ELF64 loading,
//! VFS management, and dual-personality (Linux POSIX & Windows NT) system call
//! dispatching using 100% safe Rust abstractions provided by `zenith-frame`.

pub mod elf;
pub mod process;
pub mod syscall;
pub mod vfs;

use crate::process::{Pid, ProcessTable};
use crate::syscall::{dispatch_linux_syscall, dispatch_winnt_syscall, AbiPersonality};
use crate::vfs::FileDescriptor;
use zenith_frame::memory::{FrameAllocator, VirtAddr, VmReader, VmWriter, PAGE_SIZE};
use zenith_frame::sched::{Scheduler, TaskId};
use zenith_frame::vm::{PageFlags, VmSpace};
use zenith_frame::{Error, UserContextApi};

/// Default base virtual address for user-space process heap (`brk`).
pub const DEFAULT_USER_HEAP_BASE: VirtAddr = VirtAddr(0x0000_0000_1000_0000);

/// Static state for Phase 1 & Phase 2 Kernel Services bootstrap.
#[derive(Debug)]
pub struct KernelServices {
    /// Physical frame allocator managing 256 frames (1 MiB pool).
    pub allocator: FrameAllocator<4>,
    /// Pre-allocated scheduler for up to 16 concurrent tasks.
    pub scheduler: Scheduler<16>,
    /// Pre-allocated process table for up to 16 concurrent processes.
    pub processes: ProcessTable<16>,
    /// Initial process File Descriptor table (`stdin`, `stdout`, `stderr` + 13 slots).
    pub fd_table: vfs::FdTable<16>,
    /// In-memory `initramfs` file table holding up to 8 boot files.
    pub ramfs: vfs::RamFs<8>,
}

impl Default for KernelServices {
    fn default() -> Self {
        Self::new()
    }
}

impl KernelServices {
    /// Constructs a new `KernelServices` instance in static/stack memory.
    ///
    /// # Safety Invariants
    /// Uses 100% safe `const` constructors without heap allocation.
    pub const fn new() -> Self {
        Self {
            allocator: FrameAllocator::new(),
            scheduler: Scheduler::new(),
            processes: ProcessTable::new(),
            fd_table: vfs::FdTable::new(),
            ramfs: vfs::RamFs::new(),
        }
    }

    /// Creates an isolated user `VmSpace`, maps its initial stack page, and spawns a task.
    ///
    /// # Safety Invariants
    /// Relies exclusively on safe capability tokens (`VmFrame`, `VmSpace`, `TaskId`).
    pub fn bootstrap_initial_user_task(
        &mut self,
        entry_rip: usize,
        stack_page_vaddr: VirtAddr,
    ) -> Result<(VmSpace, TaskId), Error> {
        let root_frame = self.allocator.alloc_frame()?;
        let stack_frame = self.allocator.alloc_frame()?;

        let mut vm_space = VmSpace::new(root_frame.start_paddr())?;
        let flags = PageFlags::PRESENT
            .union(PageFlags::WRITABLE)
            .union(PageFlags::USER)
            .union(PageFlags::NO_EXECUTE);

        vm_space.map_page(stack_page_vaddr, &stack_frame, flags)?;

        let initial_rsp = stack_page_vaddr.0 + PAGE_SIZE;
        let task_id = self.scheduler.spawn_task(entry_rip, initial_rsp)?;
        let _pid = self.processes.spawn_process(
            Pid(0),
            task_id,
            AbiPersonality::LinuxPosix,
            DEFAULT_USER_HEAP_BASE,
        )?;

        Ok((vm_space, task_id))
    }

    /// Dispatches a syscall on behalf of active process `pid` with full access to process state.
    ///
    /// # Safety Invariants
    /// Mutates process `brk`, `fd_table`, and lifecycle states in 100% safe Rust.
    pub fn dispatch_process_syscall<C: UserContextApi>(&mut self, pid: Pid, ctx: &mut C) {
        let num = ctx.syscall_num();
        let args = ctx.syscall_args();

        let personality = match self.processes.get(pid) {
            Ok(proc_info) => proc_info.personality,
            Err(err) => {
                ctx.set_syscall_ret(err.to_posix_errno() as usize);
                return;
            }
        };

        let result = match (personality, num) {
            (AbiPersonality::LinuxPosix, syscall::SYS_GETPID) => Ok(pid.0),
            (AbiPersonality::LinuxPosix, syscall::SYS_BRK) => self
                .processes
                .get_mut(pid)
                .map(|proc_mut| proc_mut.update_brk(args[0])),
            (AbiPersonality::LinuxPosix, syscall::SYS_CLOSE) => self
                .fd_table
                .close(FileDescriptor(args[0]))
                .map(|()| 0usize),
            (AbiPersonality::LinuxPosix, syscall::SYS_EXIT | syscall::SYS_EXIT_GROUP) => self
                .processes
                .exit_process(pid, args[0] as i32)
                .map(|()| 0usize),
            (AbiPersonality::LinuxPosix, _) => dispatch_linux_syscall(num, args),
            (AbiPersonality::WindowsNt, _) => dispatch_winnt_syscall(num, args),
        };

        let ret_val = match result {
            Ok(val) => val,
            Err(err) => err.to_posix_errno() as usize,
        };
        ctx.set_syscall_ret(ret_val);
    }

    /// Transfers bytes from a safe `VmReader` to a safe `VmWriter` using a stack buffer.
    ///
    /// # Safety Invariants
    /// Zero heap allocation and zero raw pointer usage; bounded to a 64-byte stack buffer.
    pub fn pipe_buffer_copy(
        reader: &mut VmReader<'_>,
        writer: &mut VmWriter<'_>,
    ) -> Result<usize, Error> {
        let mut temp = [0u8; 64];
        let to_read = temp.len().min(reader.remaining()).min(writer.remaining());
        let read_count = reader.read_bytes(&mut temp[..to_read])?;
        writer.write_bytes(&temp[..read_count])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elf::{map_elf64_segments, ELF64_EHDR_SIZE, ELF64_PHDR_SIZE};
    use crate::process::{
        encode_sysv_initial_stack, AuxvEntry, ProcessState, AT_ENTRY, AT_PAGESZ,
    };
    use crate::syscall::dispatch_syscall;
    use crate::vfs::{FdEntry, InodeKind, STDERR_FD, STDIN_FD, STDOUT_FD};
    use zenith_frame::trap::UserContext;

    #[test]
    fn test_phase1_bootstrap_and_scheduler() {
        let mut kernel = KernelServices::new();
        let stack_vaddr = VirtAddr(0x0000_7FFF_F000_0000);
        let (vm_space, task_id) = kernel
            .bootstrap_initial_user_task(0x0040_1000, stack_vaddr)
            .expect("bootstrap should succeed");

        assert_eq!(task_id, TaskId(0));
        let (mapped_paddr, flags) = vm_space
            .query_page(stack_vaddr)
            .expect("stack page must be mapped");
        assert!(mapped_paddr.is_page_aligned());
        assert_e
