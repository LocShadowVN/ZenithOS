// services/zenith-kernel/src/lib.rs
#![no_std]
// OS Services layer: unsafe is strictly forbidden across the entire crate.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! ZenithOS Safe Kernel Services.
//!
//! Coordinates memory initialization, process address spaces, scheduling, and
//! dual-personality (Linux POSIX & Windows NT) system call dispatching using
//! 100% safe Rust abstractions provided by `zenith-frame`.

pub mod syscall;

use zenith_frame::memory::{FrameAllocator, VirtAddr, VmReader, VmWriter, PAGE_SIZE};
use zenith_frame::sched::{Scheduler, TaskId};
use zenith_frame::vm::{PageFlags, VmSpace};
use zenith_frame::Error;

/// Static state for Phase 1 Kernel Services bootstrap.
#[derive(Debug)]
pub struct KernelServices {
    /// Physical frame allocator managing 256 frames (1 MiB pool).
    pub allocator: FrameAllocator<4>,
    /// Pre-allocated scheduler for up to 16 concurrent tasks.
    pub scheduler: Scheduler<16>,
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

        Ok((vm_space, task_id))
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
    use crate::syscall::{dispatch_syscall, AbiPersonality, SYS_WRITE};
    use zenith_frame::trap::UserContext;
    use zenith_frame::UserContextApi;

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
        assert_eq!(
            flags,
            PageFlags::PRESENT
                .union(PageFlags::WRITABLE)
                .union(PageFlags::USER)
                .union(PageFlags::NO_EXECUTE)
        );

        let next = kernel.scheduler.pick_next_runnable();
        assert_eq!(next, Some(TaskId(0)));
    }

    #[test]
    fn test_vm_reader_writer_safe_copy() {
        let src_data = *b"ZenithOS Framekernel Phase 1";
        let mut dst_data = [0u8; 28];

        let mut reader = VmReader::from_slice(&src_data);
        let mut writer = VmWriter::from_slice(&mut dst_data);

        let copied = KernelServices::pipe_buffer_copy(&mut reader, &mut writer)
            .expect("safe copy should succeed");
        assert_eq!(copied, 28);
        assert_eq!(&dst_data, b"ZenithOS Framekernel Phase 1");
    }

    #[test]
    fn test_dispatch_syscall_returns_enosys_for_stubs() {
        let mut ctx = UserContext::new();
        ctx.rax = SYS_WRITE;
        ctx.rdi = 1; // stdout

        dispatch_syscall(&mut ctx, AbiPersonality::LinuxPosix);
        assert_eq!(ctx.syscall_num() as isize, -38); // -ENOSYS
    }
}
