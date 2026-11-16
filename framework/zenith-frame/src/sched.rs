// framework/zenith-frame/src/sched.rs
//! Pre-allocated Round-Robin scheduler core with zero heap allocation in hot paths.

use crate::trap::UserContext;
use crate::Error;

/// Unique identifier for a scheduled task/thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskId(pub usize);

/// Execution state of a task inside the Framework scheduler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    /// Slot is unused and available for spawning.
    Empty,
    /// Task is ready to be picked by the scheduler.
    Runnable,
    /// Task is currently executing on a CPU core.
    Running,
    /// Task is blocked waiting on an I/O or IPC event.
    Blocked,
}

/// Static Task Control Block (TCB) stored in the pre-allocated scheduler pool.
#[derive(Debug, Clone, Copy)]
pub struct TaskControlBlock {
    /// Task identifier.
    pub id: TaskId,
    /// Current scheduling state.
    pub state: TaskState,
    /// Saved user-space register context.
    pub user_ctx: UserContext,
}

impl TaskControlBlock {
    const fn empty(index: usize) -> Self {
        Self {
            id: TaskId(index),
            state: TaskState::Empty,
            user_ctx: UserContext::new(),
        }
    }
}

/// Fixed-capacity Round-Robin scheduler requiring zero heap allocation.
#[derive(Debug)]
pub struct Scheduler<const MAX_TASKS: usize> {
    tasks: [TaskControlBlock; MAX_TASKS],
    current_idx: usize,
}

impl<const MAX_TASKS: usize> Default for Scheduler<MAX_TASKS> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const MAX_TASKS: usize> Scheduler<MAX_TASKS> {
    /// Creates a new pre-allocated scheduler with `MAX_TASKS` empty slots.
    ///
    /// # Safety Invariants
    /// Uses const array initialization; never allocates on the heap.
    pub const fn new() -> Self {
        let mut tasks = [TaskControlBlock::empty(0); MAX_TASKS];
        let mut i = 0;
        while i < MAX_TASKS {
            tasks[i] = TaskControlBlock::empty(i);
            i += 1;
        }
        Self {
            tasks,
            current_idx: 0,
        }
    }

    /// Spawns a new runnable task with initial user `entry_rip` and `user_rsp`.
    ///
    /// # Safety Invariants
    /// Allocates from the static task array and returns `Err(Error::OutOfMemory)` if full.
    pub fn spawn_task(&mut self, entry_rip: usize, user_rsp: usize) -> Result<TaskId, Error> {
        for slot in &mut self.tasks {
            if slot.state == TaskState::Empty {
                slot.state = TaskState::Runnable;
                slot.user_ctx = UserContext::new();
                slot.user_ctx.rip = entry_rip;
                slot.user_ctx.rsp = user_rsp;
                return Ok(slot.id);
            }
        }
        Err(Error::OutOfMemory)
    }

    /// Selects the next runnable task in round-robin order without heap allocation.
    ///
    /// # Safety Invariants
    /// Bounded loop of at most `MAX_TASKS` iterations; safe for interrupt/scheduler hot paths.
    pub fn pick_next_runnable(&mut self) -> Option<TaskId> {
        if MAX_TASKS == 0 {
            return None;
        }
        for offset in 1..=MAX_TASKS {
            let idx = (self.current_idx + offset) % MAX_TASKS;
            if self.tasks[idx].state == TaskState::Runnable {
                if self.tasks[self.current_idx].state == TaskState::Running {
                    self.tasks[self.current_idx].state = TaskState::Runnable;
                }
                self.tasks[idx].state = TaskState::Running;
                self.current_idx = idx;
                return Some(self.tasks[idx].id);
            }
        }
        None
    }

    /// Performs a low-level CPU register context switch to `next_task`.
    ///
    /// # Safety Invariants
    /// Validates `next_task` index before attempting hardware stack switch.
    pub fn context_switch_to(&mut self, next_task: TaskId) -> Result<(), Error> {
        if next_task.0 >= MAX_TASKS || self.tasks[next_task.0].state == TaskState::Empty {
            return Err(Error::NotFound);
        }
        // TODO: requires arch_x86_64_context_switch callee-saved register switch assembly
        Err(Error::NotImplemented)
    }
}
