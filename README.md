# ZenithOS

**A Research-Grade, 100% Bare-Metal Rust Capability Framekernel Operating System**  
*Featuring Native Linux (`ELF64`) & Windows NT (`PE32+`) Dual-ABI Subsystem Compatibility*

[![ZenithOS Fast CI](https://github.com/ZenithOS/ZenithOS/actions/workflows/ci.yml/badge.svg)](https://github.com/ZenithOS/ZenithOS/actions/workflows/ci.yml)
[![Language: 100% Rust](https://img.shields.io/badge/Language-100%25%20Rust%20%23!%5Bno__std%5D-orange.svg)](https://www.rust-lang.org/)
[![Architecture: x86_64](https://img.shields.io/badge/Arch-x86__64%20Long%20Mode-blue.svg)](#architecture)
[![Safety: #![forbid(unsafe_code)]](https://img.shields.io/badge/Kernel%20Core-%23!%5Bforbid(unsafe__code)%5D-brightgreen.svg)](#safety--engineering-invariants)
[![License: GPL v3.0](https://img.shields.io/badge/License-GPL%20v3.0-red.svg)](https://www.gnu.org/licenses/gpl-3.0)

</div>

---

## Overview

**ZenithOS** is a next-generation operating system written from scratch in **100% `#![no_std]` Rust** for the `x86_64` architecture. Inspired by the **Framekernel** paradigm (Asterinas OSTD), **seL4/Zircon** capability object models, and **Starnix/Drawbridge** pico-process translation engines, ZenithOS is engineered to achieve two historically conflicting goals:

1. **Mathematical Memory Safety in Ring 0**: All low-level hardware interactions (`unsafe` operations) are strictly confined to a minimal, auditable **OS Standard Framework (`ostd`)** (<10% of the codebase). The entire remainder of the kernel—including the scheduler, virtual memory manager, ELF64 loader, and ABI syscall routers—is enforced with **`#![forbid(unsafe_code)]`** and a **Zero-Panic** policy.
2. **Zero-VM Dual-ABI Binary Compatibility**: ZenithOS executes unmodified **Linux (`ELF64` / System V AMD64 ABI)** binaries directly in Ring 3 (`CPL = 3`) via hardware `SYSCALL`/`SYSRETQ`, with architectural support for **Windows 64-bit (`PE32+` / Microsoft x64 NT ABI)** processes over a unified set of kernel primitives.

---

## System Architecture (Capability-Based Framekernel)

```text
+-----------------------------------------------------------------------------------+
|                               USER SPACE (RING 3)                                 |
|                                                                                   |
|  +------------------------+  +------------------------+  +---------------------+  |
|  |   Linux App (ELF64)    |  |  Windows App (PE32+)   |  | Microkernel Servers |  |
|  |  musl / glibc / static |  | kernel32 / user32.dll  |  |  - VFS (Ext4/NTFS)  |  |
|  +-----------+------------+  +-----------+------------+  |  - NetStack         |  |
|              | syscall                   | call          |  - VirtIO Drivers   |  |
|              v                           v               |  - GUI Compositor   |  |
|  +------------------------+  +------------------------+  +----------+----------+  |
|  |  vDSO (Linux Time/CPU) |  | Clean-room ntdll.dll   |             |             |
|  +-----------+------------+  +-----------+------------+             |             |
+--------------|---------------------------|--------------------------|-------------+
               | SYSCALL (SysV ABI)        | SYSCALL (MS x64 NT ABI)  | Native IPC
===============|===========================|==========================|==============
               v                           v                          v
+-----------------------------------------------------------------------------------+
|                        SAFE KERNEL CORE (RING 0 - 100% SAFE RUST)                 |
|                        #![forbid(unsafe_code)]                                    |
|                                                                                   |
|  +-----------------------------------------------------------------------------+  |
|  |               Personality Syscall Router (Per-Thread Tagging)               |  |
|  +-----------------+-------------------------------+---------------------------+  |
|                    |                               |                              |
|                    v                               v                              |
|  +----------------------------------+   +--------------------------------------+  |
|  |   Linux Personality Engine       |   |     Windows NT Personality Engine    |  |
|  |   - ELF64 Loader & SysV Stack    |   |     - PE32+ Loader & TEB/PEB         |  |
|  |   - fd table -> Capabilities     |   |     - HANDLE table -> Capabilities   |  |
|  |   - mmap/brk -> VMO              |   |     - NtAllocateVirtualMemory -> VMO |  |
|  |   - futex/epoll -> WaitPort      |   |     - IOCP/KeyedEvent -> WaitPort    |  |
|  +-----------------+----------------+   +------------------+-------------------+  |
|                    |                                       |                      |
|                    +-------------------+-------------------+                      |
|                                        v                                          |
|  +-----------------------------------------------------------------------------+  |
|  |           Unified Native Primitives (Zircon-inspired Object Core)           |  |
|  |   [VMO (Virtual Memory Objects)]  [Capability Table]  [WaitPort]  [Channel] |  |
|  +-----------------------------------------------------------------------------+  |
+----------------------------------------|------------------------------------------+
                                         | Safe Rust Trait Boundary
+----------------------------------------v------------------------------------------+
|                       OSTD FRAMEWORK (RING 0 - AUDITED UNSAFE)                    |
|   [4-Level MMU / CR3] [Context Switch & TLS] [MSR/GS/FS Swap] [GDT/TSS/IDT/IRQ]   |
+-----------------------------------------------------------------------------------+
```

---

## Engineering Roadmap & Current Status

- [x] **Phase 1: OSTD Hardware Frame & Fast Syscall Infrastructure**
  - Limine Boot Protocol integration (Higher-Half Kernel at `0xFFFFFFFF80000000` + HHDM).
  - 64-bit GDT with Ring 0 / Ring 3 selectors ordered for hardware `SYSRETQ` (`CS=0x23`, `SS=0x1B`) and 64-bit TSS (`RSP0` & `IST1` Double-Fault emergency stack).
  - 256-entry IDT with hardware exception handlers (`#BP`, `#DF`, `#GP`, `#PF`) and automated `int3` boot self-test.
  - Bitmap Physical Frame Allocator (PMM) & Model-Specific Registers (`IA32_EFER`, `IA32_STAR`, `IA32_LSTAR`, `IA32_SFMASK`, `IA32_FS_BASE`, `IA32_KERNEL_GS_BASE`).
- [x] **Phase 2: 4-Level User-Space MMU, Safe ELF64 Loader & Ring 3 Execution**
  - Dynamic 4-level page table walker (`PML4` $\rightarrow$ `PDPT` $\rightarrow$ `PD` $\rightarrow$ `PT`) mapping `PAGE_USER` frames into lower-half canonical user memory.
  - 100% Safe Rust Linux `ELF64` parser and segment loader (`PT_LOAD`).
  - System V AMD64 ABI initial user stack construction (`argc`, `argv`, `envp`, and Auxiliary Vectors `AT_PAGESZ`, `AT_ENTRY`, `AT_PHENT`, `AT_PHNUM`).
  - Bidirectional Ring 0 $\leftrightarrow$ Ring 3 transitions via `sysretq` and `syscall` (`swapgs`), executing embedded Linux `/bin/init` in `CPL = 3` with `SYS_read`, `SYS_write`, `SYS_writev`, `SYS_ioctl`, `SYS_mmap`, `SYS_brk`, `SYS_uname`, `SYS_getpid`, `SYS_gettid`, `SYS_set_tid_address`, `SYS_arch_prctl`, and `SYS_exit_group`.
- [ ] **Phase 3: Preemptive SMP Scheduler, VMO & Capability Engine**
  - Per-CPU runqueues, APIC Timer preemption, Copy-on-Write (`fork` / `clone`), and `futex` / `WaitPort` synchronization.
- [ ] **Phase 4: VFS, Initramfs (CPIO/Tar) & Interactive Linux Userland (`busybox` / `musl`)**
  - In-memory VFS, VirtIO Block/Network drivers, dynamic linker (`ld-musl-x86_64.so.1`) support, and interactive shell.
- [ ] **Phase 5: Windows NT (`PE32+`) Subsystem & Clean-Room `ntdll.dll`**
  - `PE32+` image loader, `TEB`/`PEB` & `KUSER_SHARED_DATA` (`0x7FFE0000`) initialization, and Rust `ntdll.dll` / `kernelbase.dll`.
- [ ] **Phase 6: Unified Wayland / Win32 Display Compositor**

---

## Safety & Engineering Invariants

ZenithOS enforces strict systems engineering rules across the entire repository:

1. **Strict `unsafe` Encapsulation**: Only `mod ostd` may contain `unsafe` blocks. Every single `unsafe` block must be preceded by a `// SAFETY:` proof documenting pointer validity, alignment, aliasing, and architectural state invariants.
2. **`#![forbid(unsafe_code)]` in Kernel Subsystems**: All higher-level kernel subsystems (`mod linux_abi`, ELF64 loader, process state management) are compiled with `#![forbid(unsafe_code)]`.
3. **Zero-Panic Kernel Execution**: Runtime panics are prohibited. `unwrap()`, `expect()`, and unchecked slice indexing are banned in favor of explicit `Result<T, E>` error propagation and safe bounds-checked accessors (`.get()` / `.get_mut()`).

---

## Building & Running Locally

### Prerequisites
- **Rust Nightly** (automatically managed via `rust-toolchain.toml`)
- **QEMU** (`qemu-system-x86_64`), **xorriso**, and **Limine** (`v8.x-binary`)

### Compile & Run Automated Verification Suite
```bash
# Format check & static analysis
cargo fmt --all -- --check
cargo clippy -- -D warnings

# Build the bare-metal x86_64 kernel
cargo build
```

When booted in QEMU, ZenithOS outputs full hardware and Ring 3 Linux ABI telemetry over `COM1` (`0x3F8`):

```text
================================================================
  ZenithOS v0.2.0 - 100% Rust Framekernel (Linux ABI Phase 2)   
================================================================
[ZENITH-OSTD] GDT/TSS, IDT (#BP), and SYSCALL/SYSRETQ MSRs ready.
[ZENITH-ELF] Loaded Linux x86_64 ELF64 '/bin/init': entry=0x401000, user_rsp=0x801f40, brk=0x403000
[ZENITH-CORE] Transitioning to Ring 3 User-Space via SYSRETQ...
[RING-3 ELF64] Hello from Linux User-Space (CPL=3) via SYSCALL/SYSRETQ!
[RING-3 ELF64] musl-style SYS_writev(iovcnt=2) verified in Ring 3!
[ZENITH-CORE] Ring 3 process '/bin/init' exited cleanly back to Kernel with token=0x25.
================================================================
[ZENITH-CI] ALL PHASE 2 ELF64 & RING 3 USER-SPACE TESTS PASSED! 
================================================================
```

---

## License

Copyright © 2026 **ZenithOS Contributors**.

This project is free and open-source software licensed under the **GNU General Public License v3.0 (GPL-3.0-only)**. See the [GNU GPL v3.0](https://www.gnu.org/licenses/gpl-3.0.html) specification for full license terms.
