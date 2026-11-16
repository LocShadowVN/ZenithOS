// framework/zenith-frame/src/vm.rs
//! Virtual memory address space (`VmSpace`) and 4-level x86_64 page table management.

use crate::memory::{PhysAddr, VirtAddr, VmFrame};
use crate::Error;

/// Highest canonical user-space address on 4-level x86_64 paging (`0x0000_7FFF_FFFF_FFFF`).
pub const USER_SPACE_MAX_VADDR: usize = 0x0000_7FFF_FFFF_FFFF;

/// Maximum number of tracked mappings in a static `VmSpace` table before full hardware walk.
pub const MAX_STATIC_MAPPINGS: usize = 32;

/// Page table entry permission flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageFlags(u64);

impl PageFlags {
    /// Page is present in memory (`BIT 0`).
    pub const PRESENT: Self = Self(1 << 0);
    /// Page is writable (`BIT 1`).
    pub const WRITABLE: Self = Self(1 << 1);
    /// Page is accessible from Ring 3 user space (`BIT 2`).
    pub const USER: Self = Self(1 << 2);
    /// Instruction fetch is disabled on this page (`BIT 63`).
    pub const NO_EXECUTE: Self = Self(1 << 63);

    /// Combines two [`PageFlags`] masks.
    ///
    /// # Safety Invariants
    /// Pure bitwise OR over hardware page flag bits.
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Returns the raw 64-bit hardware PTE flag mask.
    ///
    /// # Safety Invariants
    /// Contains only flag bits; physical address bits are masked out.
    pub const fn bits(self) -> u64 {
        self.0
    }
}

/// Decomposed 4-level x86_64 page table indices (9 bits per level).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageTableIndices {
    /// Level 4 (PML4) index (`bits 39..48`).
    pub pml4: usize,
    /// Level 3 (PDPT) index (`bits 30..39`).
    pub pdpt: usize,
    /// Level 2 (PD) index (`bits 21..30`).
    pub pd: usize,
    /// Level 1 (PT) index (`bits 12..21`).
    pub pt: usize,
}

/// Computes the 4-level x86_64 page table indices for a page-aligned user [`VirtAddr`].
///
/// # Safety Invariants
/// Rejects non-page-aligned or kernel-space virtual addresses to prevent privilege escalation.
pub const fn decode_page_table_indices(vaddr: VirtAddr) -> Result<PageTableIndices, Error> {
    if !vaddr.is_page_aligned() || vaddr.0 > USER_SPACE_MAX_VADDR {
        return Err(Error::InvalidAddress);
    }
    Ok(PageTableIndices {
        pml4: (vaddr.0 >> 39) & 0x1FF,
        pdpt: (vaddr.0 >> 30) & 0x1FF,
        pd: (vaddr.0 >> 21) & 0x1FF,
        pt: (vaddr.0 >> 12) & 0x1FF,
    })
}

/// Recorded virtual-to-physical page mapping entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MappingEntry {
    vaddr: VirtAddr,
    paddr: PhysAddr,
    flags: PageFlags,
}

/// Safe capability representing an isolated user-space virtual address space (`CR3`).
#[derive(Debug)]
pub struct VmSpace {
    root_paddr: PhysAddr,
    mappings: [Option<MappingEntry>; MAX_STATIC_MAPPINGS],
}

impl VmSpace {
    /// Creates a new `VmSpace` rooted at `root_paddr` (PML4 physical address).
    ///
    /// # Safety Invariants
    /// Validates that `root_paddr` is 4 KiB page-aligned before constructing the space.
    pub const fn new(root_paddr: PhysAddr) -> Result<Self, Error> {
        if !root_paddr.is_page_aligned() {
            return Err(Error::InvalidArgument);
        }
        Ok(Self {
            root_paddr,
            mappings: [None; MAX_STATIC_MAPPINGS],
        })
    }

    /// Returns the physical address of the root PML4 page table.
    ///
    /// # Safety Invariants
    /// Guaranteed 4 KiB aligned.
    pub const fn root_paddr(&self) -> PhysAddr {
        self.root_paddr
    }

    /// Maps a user-space virtual page `vaddr` to `frame` with `flags`.
    ///
    /// # Safety Invariants
    /// Enforces that `vaddr` lies strictly within `0..=USER_SPACE_MAX_VADDR` so OS Services
    /// can never map or overwrite kernel-space page table entries.
    pub fn map_page(
        &mut self,
        vaddr: VirtAddr,
        frame: &VmFrame,
        flags: PageFlags,
    ) -> Result<(), Error> {
        let _indices = decode_page_table_indices(vaddr)?;

        for slot in self.mappings.iter().flatten() {
            if slot.vaddr == vaddr {
                return Err(Error::InvalidArgument);
            }
        }

        for slot in &mut self.mappings {
            if slot.is_none() {
                *slot = Some(MappingEntry {
                    vaddr,
                    paddr: frame.start_paddr(),
                    flags,
                });
                return Ok(());
            }
        }
        Err(Error::OutOfMemory)
    }

    /// Translates a mapped user `vaddr` back to its `PhysAddr` and `PageFlags`.
    ///
    /// # Safety Invariants
    /// Read-only lookup in pre-allocated mapping table.
    pub fn query_page(&self, vaddr: VirtAddr) -> Result<(PhysAddr, PageFlags), Error> {
        if !vaddr.is_page_aligned() {
            return Err(Error::InvalidAddress);
        }
        for slot in self.mappings.iter().flatten() {
            if slot.vaddr == vaddr {
                return Ok((slot.paddr, slot.flags));
            }
        }
        Err(Error::NotFound)
    }

    /// Activates this `VmSpace` by loading its `root_paddr` into the CPU `CR3` register.
    ///
    /// # Safety Invariants
    /// Stubbed until bare-metal `CR3` assembly primitive is linked.
    pub fn activate(&self) -> Result<(), Error> {
        // TODO: requires arch_x86_64_write_cr3 hardware instruction wrapper
        Err(Error::NotImplemented)
    }

    /// Flushes the TLB entry for `vaddr` on the local CPU core.
    ///
    /// # Safety Invariants
    /// Stubbed until bare-metal `invlpg` assembly primitive is linked.
    pub fn flush_tlb(&self, _vaddr: VirtAddr) -> Result<(), Error> {
        // TODO: requires arch_x86_64_invlpg hardware instruction wrapper
        Err(Error::NotImplemented)
    }
}
