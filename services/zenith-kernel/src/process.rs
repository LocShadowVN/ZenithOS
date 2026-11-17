// services/zenith-kernel/src/process.rs
//! Process Control Block (`ProcessControlBlock`), `ProcessTable`, and System V AMD64 stack builder.

use crate::syscall::AbiPersonality;
use zenith_frame::memory::{VirtAddr, VmWriter, PAGE_SIZE};
use zenith_frame::sched::TaskId;
use zenith_frame::Error;

/// End of Auxiliary Vector (`AT_NULL`).
pub const AT_NULL: u64 = 0;
/// Address of ELF program headers in memory (`AT_PHDR`).
pub const AT_PHDR: u64 = 3;
/// Size of one ELF program header entry (`AT_PHENT`).
pub const AT_PHENT: u64 = 4;
/// Number of ELF program headers (`AT_PHNUM`).
pub const AT_PHNUM: u64 = 5;
/// System page size in bytes (`AT_PAGESZ`).
pub const AT_PAGESZ: u64 = 6;
/// Entry point virtual address of the executable (`AT_ENTRY`).
pub const AT_ENTRY: u64 = 9;

/// Strongly-typed POSIX Process Identifier (`PID >= 1`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pid(pub usize);

/// Lifecycle state of a process in [`ProcessTable`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessState {
    /// Table slot is free.
    Unused,
    /// Process is actively running or runnable.
    Active,
    /// Process has terminated and awaits parent `wait4` collection.
    Zombie {
        /// Exit status code passed to `_exit` / `exit_group`.
        exit_code: i32,
    },
}

/// Key-value pair in the System V AMD64 ELF Auxiliary Vector (`auxv`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuxvEntry {
    /// Auxiliary vector tag (`AT_*`).
    pub a_type: u64,
    /// Auxiliary vector 64-bit value.
    pub a_val: u64,
}

/// Encodes an initial System V AMD64 ABI stack frame (`argc`, `argv`, `envp`, `auxv`) into `writer`.
///
/// # Safety Invariants
/// Writes strictly bounds-checked 8-byte little-endian words via `VmWriter` in 100% safe Rust.
pub fn encode_sysv_initial_stack(
    writer: &mut VmWriter<'_>,
    argc: u64,
    auxv: &[AuxvEntry],
) -> Result<usize, Error> {
    // Words needed: argc (1) + argv NULL (1) + envp NULL (1) + (auxv.len() + 1) * 2
    let total_words = 3usize
        .checked_add((auxv.len().checked_add(1).ok_or(Error::InvalidArgument)?) * 2)
        .ok_or(Error::InvalidArgument)?;
    let total_bytes = total_words.checked_mul(8).ok_or(Error::InvalidArgument)?;

    if writer.remaining() < total_bytes {
        return Err(Error::OutOfMemory);
    }

    let mut written = 0usize;
    written += writer.write_bytes(&argc.to_le_bytes())?;
    written += writer.write_bytes(&0u64.to_le_bytes())?; // argv[0] = NULL terminator
    written += writer.write_bytes(&0u64.to_le_bytes())?; // envp[0] = NULL terminator

    for entry in auxv {
        written += writer.write_bytes(&entry.a_type.to_le_bytes())?;
        written += writer.write_bytes(&entry.a_val.to_le_bytes())?;
    }
    // Mandatory AT_NULL terminator entry
    written += writer.write_bytes(&AT_NULL.to_le_bytes())?;
    written += writer.write_bytes(&0u64.to_le_bytes())?;

    Ok(written)
}

/// Per-process metadata block managed by [`ProcessTable`].
#[derive(Debug, Clone, Copy)]
pub struct ProcessControlBlock {
    /// Unique process ID (`PID >= 1`).
    pub pid: Pid,
    /// Parent process ID (`Pid(0)` for initial kernel-spawned init process).
    pub parent_pid: Pid,
    /// Associated scheduler thread ID.
    pub task_id: TaskId,
    /// Active syscall ABI personality (Linux POSIX or Windows NT).
    pub personality: AbiPersonality,
    /// Current lifecycle state.
    pub state: ProcessState,
    /// Initial program break (start of user heap).
    pub brk_start: VirtAddr,
    /// Current program break modified by `sys_brk`.
    pub brk_current: VirtAddr,
    /// Maximum allowed program break for the pre-mapped heap region.
    pub brk_max: VirtAddr,
}

impl ProcessControlBlock {
    const fn unused(index: usize) -> Self {
        Self {
            pid: Pid(index + 1),
            parent_pid: Pid(0),
            task_id: TaskId(0),
            personality: AbiPersonality::LinuxPosix,
            state: ProcessState::Unused,
            brk_start: VirtAddr(0),
            brk_current: VirtAddr(0),
            brk_max: VirtAddr(0),
        }
    }

    /// Updates or queries the user heap program break following Linux `sys_brk` semantics.
    ///
    /// # Safety Invariants
    /// Never dereferences user addresses; clamps break strictly within `brk_start..=brk_max`.
    pub fn update_brk(&mut self, requested_vaddr: usize) -> usize {
        if requested_vaddr >= self.brk_start.0 && requested_vaddr <= self.brk_max.0 {
            self.brk_current = VirtAddr(requested_vaddr);
        }
        self.brk_current.0
    }
}

/// Fixed-capacity Process Table requiring zero heap allocation.
#[derive(Debug)]
pub struct ProcessTable<const MAX_PROCS: usize> {
    procs: [ProcessControlBlock; MAX_PROCS],
}

impl<const MAX_PROCS: usize> Default for ProcessTable<MAX_PROCS> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const MAX_PROCS: usize> ProcessTable<MAX_PROCS> {
    /// Creates an empty `ProcessTable` with `MAX_PROCS` slots.
    ///
    /// # Safety Invariants
    /// Uses `const` array initialization without heap allocation.
    pub const fn new() -> Self {
        let mut procs = [ProcessControlBlock::unused(0); MAX_PROCS];
        let mut i = 0;
        while i < MAX_PROCS {
            procs[i] = ProcessControlBlock::unused(i);
            i += 1;
        }
        Self { procs }
    }

    /// Allocates a new active process slot and initializes its heap break bounds.
    ///
    /// # Safety Invariants
    /// Validates page alignment of `heap_start` and returns `Err(Error::OutOfMemory)` if full.
    pub fn spawn_process(
        &mut self,
        parent_pid: Pid,
        task_id: TaskId,
        personality: AbiPersonality,
        heap_start: VirtAddr,
    ) -> Result<Pid, Error> {
        if !heap_start.is_page_aligned() {
            return Err(Error::InvalidAddress);
        }
        let heap_max = VirtAddr(
            heap_start
                .0
                .checked_add(PAGE_SIZE * 16)
                .ok_or(Error::InvalidAddress)?,
        );

        for slot in &mut self.procs {
            if slot.state == ProcessState::Unused {
                slot.parent_pid = parent_pid;
                slot.task_id = task_id;
                slot.personality = personality;
                slot.state = ProcessState::Active;
                slot.brk_start = heap_start;
                slot.brk_current = heap_start;
                slot.brk_max = heap_max;
                return Ok(slot.pid);
            }
        }
        Err(Error::OutOfMemory)
    }

    /// Looks up an active or zombie process by `pid`.
    ///
    /// # Safety Invariants
    /// Bounds-checked lookup over `1..=MAX_PROCS`.
    pub fn get(&self, pid: Pid) -> Result<&ProcessControlBlock, Error> {
        if pid.0 == 0 || pid.0 > MAX_PROCS {
            return Err(Error::NotFound);
        }
        let slot = &self.procs[pid.0 - 1];
        if slot.state == ProcessState::Unused {
            return Err(Error::NotFound);
        }
        Ok(slot)
    }

    /// Looks up a mutable reference to a process by `pid`.
    ///
    /// # Safety Invariants
    /// Bounds-checked mutable lookup over `1..=MAX_PROCS`.
    pub fn get_mut(&mut self, pid: Pid) -> Result<&mut ProcessControlBlock, Error> {
        if pid.0 == 0 || pid.0 > MAX_PROCS {
            return Err(Error::NotFound);
        }
        let slot = &mut self.procs[pid.0 - 1];
        if slot.state == ProcessState::Unused {
            return Err(Error::NotFound);
        }
        Ok(slot)
    }

    /// Marks `pid` as [`ProcessState::Zombie`] with `exit_code`.
    ///
    /// # Safety Invariants
    /// Transitions process state in-place without freeing active kernel stack frames prematurely.
    pub fn exit_process(&mut self, pid: Pid, exit_code: i32) -> Result<(), Error> {
        let proc_slot = self.get_mut(pid)?;
        proc_slot.state = ProcessState::Zombie { exit_code };
        Ok(())
    }
}
