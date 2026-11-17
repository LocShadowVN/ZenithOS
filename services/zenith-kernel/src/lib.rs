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
        ctx.rax = syscall::SYS_WRITE;
        ctx.rdi = 1; // stdout

        dispatch_syscall(&mut ctx, AbiPersonality::LinuxPosix);
        assert_eq!(ctx.syscall_num() as isize, -38); // -ENOSYS
    }

    #[test]
    fn test_phase2_elf64_loader_and_segment_mapping() {
        let mut fake_elf = [0u8; ELF64_EHDR_SIZE + ELF64_PHDR_SIZE];
        fake_elf[0..4].copy_from_slice(&[0x7F, b'E', b'L', b'F']);
        fake_elf[4] = 2;
        fake_elf[5] = 1;
        fake_elf[16..18].copy_from_slice(&2u16.to_le_bytes());
        fake_elf[18..20].copy_from_slice(&62u16.to_le_bytes());
        fake_elf[24..32].copy_from_slice(&0x0040_1000u64.to_le_bytes());
        fake_elf[32..40].copy_from_slice(&(ELF64_EHDR_SIZE as u64).to_le_bytes());
        fake_elf[54..56].copy_from_slice(&(ELF64_PHDR_SIZE as u16).to_le_bytes());
        fake_elf[56..58].copy_from_slice(&1u16.to_le_bytes());

        let ph = &mut fake_elf[ELF64_EHDR_SIZE..];
        ph[0..4].copy_from_slice(&1u32.to_le_bytes());
        ph[4..8].copy_from_slice(&5u32.to_le_bytes());
        ph[16..24].copy_from_slice(&0x0040_0000u64.to_le_bytes());
        ph[32..40].copy_from_slice(&4096u64.to_le_bytes());
        ph[40..48].copy_from_slice(&8192u64.to_le_bytes());

        let mut kernel = KernelServices::new();
        let root_frame = kernel.allocator.alloc_frame().expect("root frame");
        let mut vm_space = VmSpace::new(root_frame.start_paddr()).expect("vm_space");

        let mapped = map_elf64_segments(&fake_elf, &mut vm_space, &mut kernel.allocator)
            .expect("valid ELF64 should map cleanly");

        assert_eq!(mapped.entry_rip, 0x0040_1000);
        assert_eq!(mapped.mapped_pages, 2);
        assert!(vm_space.query_page(VirtAddr(0x0040_0000)).is_ok());
        assert!(vm_space.query_page(VirtAddr(0x0040_1000)).is_ok());
    }

    #[test]
    fn test_phase2_vfs_ramfs_and_fd_table() {
        let mut kernel = KernelServices::new();
        assert!(kernel.fd_table.get(STDIN_FD).is_ok());
        assert!(kernel.fd_table.get(STDOUT_FD).is_ok());
        assert!(kernel.fd_table.get(STDERR_FD).is_ok());

        let idx = kernel
            .ramfs
            .register_file(b"/sbin/init", b"ELF_INIT_PAYLOAD")
            .expect("register init file");
        let fd = kernel
            .fd_table
            .alloc_fd(FdEntry {
                kind: InodeKind::RamFile { file_idx: idx },
                offset: 0,
                readable: true,
                writable: false,
            })
            .expect("alloc fd 3");

        assert_eq!(fd.0, 3);
        let mut out_buf = [0u8; 16];
        let mut writer = VmWriter::from_slice(&mut out_buf);
        let read_len = kernel
            .ramfs
            .read_into(idx, 0, &mut writer)
            .expect("read from ramfs");
        assert_eq!(read_len, 16);
        assert_eq!(&out_buf, b"ELF_INIT_PAYLOAD");
    }

    #[test]
    fn test_phase2_process_syscalls_and_sysv_stack() {
        let mut kernel = KernelServices::new();
        let stack_vaddr = VirtAddr(0x0000_7FFF_F000_0000);
        let (_vm, _task) = kernel
            .bootstrap_initial_user_task(0x0040_1000, stack_vaddr)
            .expect("bootstrap init");
        let init_pid = Pid(1);

        // 1. Test SYS_GETPID
        let mut ctx = UserContext::new();
        ctx.rax = syscall::SYS_GETPID;
        kernel.dispatch_process_syscall(init_pid, &mut ctx);
        assert_eq!(ctx.syscall_num(), 1);

        // 2. Test SYS_BRK query and expansion
        ctx.rax = syscall::SYS_BRK;
        ctx.rdi = 0;
        kernel.dispatch_process_syscall(init_pid, &mut ctx);
        assert_eq!(ctx.syscall_num(), DEFAULT_USER_HEAP_BASE.0);

        let new_brk = DEFAULT_USER_HEAP_BASE.0 + PAGE_SIZE * 2;
        ctx.rax = syscall::SYS_BRK;
        ctx.rdi = new_brk;
        kernel.dispatch_process_syscall(init_pid, &mut ctx);
        assert_eq!(ctx.syscall_num(), new_brk);

        // 3. Test System V AMD64 initial stack layout encoding
        let mut stack_buf = [0u8; 128];
        let mut writer = VmWriter::from_slice(&mut stack_buf);
        let auxv = [
            AuxvEntry {
                a_type: AT_PAGESZ,
                a_val: PAGE_SIZE as u64,
            },
            AuxvEntry {
                a_type: AT_ENTRY,
                a_val: 0x0040_1000,
            },
        ];
        let bytes_written =
            encode_sysv_initial_stack(&mut writer, 1, &auxv).expect("encode sysv stack");
        assert_eq!(bytes_written, 72);

        // 4. Test SYS_EXIT transitions process to Zombie
        ctx.rax = syscall::SYS_EXIT;
        ctx.rdi = 42;
        kernel.dispatch_process_syscall(init_pid, &mut ctx);
        assert_eq!(ctx.syscall_num(), 0);
        assert_eq!(
            kernel.processes.get(init_pid).expect("proc 1").state,
            ProcessState::Zombie { exit_code: 42 }
        );
    }
}
