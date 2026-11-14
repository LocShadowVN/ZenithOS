use core::arch::asm;

use crate::ostd::OstdError;

pub const PAGE_SIZE: usize = 4096;
const POOL_FRAMES: usize = 512;

#[repr(C, align(4096))]
struct FramePool([[u8; PAGE_SIZE]; POOL_FRAMES]);

static mut STATIC_FRAME_POOL: FramePool = FramePool([[0; PAGE_SIZE]; POOL_FRAMES]);

pub struct BitmapFrameAllocator {
    bitmap: [u64; 8],
    base_addr: u64,
    pub hhdm_offset: u64,
    pub kernel_phys_base: u64,
    pub kernel_virt_base: u64,
}

impl Default for BitmapFrameAllocator {
    fn default() -> Self {
        Self::new()
    }
}

impl BitmapFrameAllocator {
    pub const fn const_uninit() -> Self {
        Self {
            bitmap: [0; 8],
            base_addr: 0,
            hhdm_offset: 0xFFFF_8000_0000_0000,
            kernel_phys_base: 0,
            kernel_virt_base: 0xFFFF_FFFF_8000_0000,
        }
    }

    pub fn new() -> Self {
        let base_addr = &raw const STATIC_FRAME_POOL as u64;
        let mut hhdm_offset = 0xFFFF_8000_0000_0000;
        let mut kernel_phys_base = 0x0020_0000;
        let mut kernel_virt_base = 0xFFFF_FFFF_8000_0000;

        // SAFETY:
        // Volatile reads prevent the compiler from constant-folding Limine response pointers.
        unsafe {
            let req_ptr = crate::boot::LIMINE_REQUESTS.0.get();
            let hhdm_resp = core::ptr::read_volatile(&(*req_ptr).hhdm.response);
            if !hhdm_resp.is_null() {
                hhdm_offset = core::ptr::read_volatile(&(*hhdm_resp).offset);
            }
            let ka_resp = core::ptr::read_volatile(&(*req_ptr).kernel_addr.response);
            if !ka_resp.is_null() {
                kernel_phys_base = core::ptr::read_volatile(&(*ka_resp).physical_base);
                kernel_virt_base = core::ptr::read_volatile(&(*ka_resp).virtual_base);
            }
        }

        Self {
            bitmap: [0; 8],
            base_addr,
            hhdm_offset,
            kernel_phys_base,
            kernel_virt_base,
        }
    }

    pub fn allocate_frame(&mut self) -> Result<u64, OstdError> {
        for (word_idx, word) in self.bitmap.iter_mut().enumerate() {
            if *word != u64::MAX {
                let bit_idx = (*word).trailing_ones() as usize;
                *word |= 1u64 << bit_idx;
                let frame_index = word_idx * 64 + bit_idx;
                let addr = self.base_addr + (frame_index * PAGE_SIZE) as u64;
                // SAFETY:
                // `addr` points to a uniquely allocated 4KiB-aligned frame in `STATIC_FRAME_POOL`.
                unsafe {
                    core::ptr::write_bytes(addr as *mut u8, 0, PAGE_SIZE);
                }
                return Ok(addr);
            }
        }
        Err(OstdError::OutOfPhysicalFrames)
    }

    pub fn deallocate_frame(&mut self, frame_addr: u64) -> Result<(), OstdError> {
        if frame_addr < self.base_addr || (frame_addr & 0xFFF) != 0 {
            return Err(OstdError::UnalignedAddress);
        }
        let offset = (frame_addr - self.base_addr) as usize;
        let frame_index = offset / PAGE_SIZE;
        if frame_index >= POOL_FRAMES {
            return Err(OstdError::InvalidFrameIndex);
        }
        let word_idx = frame_index / 64;
        let bit_idx = frame_index % 64;
        if let Some(word) = self.bitmap.get_mut(word_idx) {
            *word &= !(1u64 << bit_idx);
            Ok(())
        } else {
            Err(OstdError::InvalidFrameIndex)
        }
    }

    #[inline]
    pub const fn kernel_virt_to_phys(&self, virt_addr: u64) -> u64 {
        virt_addr
            .wrapping_sub(self.kernel_virt_base)
            .wrapping_add(self.kernel_phys_base)
    }

    #[inline]
    pub const fn phys_to_hhdm_virt(&self, phys_addr: u64) -> u64 {
        phys_addr.wrapping_add(self.hhdm_offset)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageFlags(pub u64);

impl PageFlags {
    pub const PRESENT: u64 = 1 << 0;
    pub const WRITABLE: u64 = 1 << 1;
    pub const USER: u64 = 1 << 2;
    pub const HUGE_PAGE: u64 = 1 << 7;
    pub const USER_RWX: u64 = Self::PRESENT | Self::WRITABLE | Self::USER;
}

#[repr(C, align(4096))]
pub struct PageTable {
    pub entries: [u64; 512],
}

impl PageTable {
    pub const fn indices(virt_addr: u64) -> (usize, usize, usize, usize) {
        let p4 = ((virt_addr >> 39) & 0x1FF) as usize;
        let p3 = ((virt_addr >> 30) & 0x1FF) as usize;
        let p2 = ((virt_addr >> 21) & 0x1FF) as usize;
        let p1 = ((virt_addr >> 12) & 0x1FF) as usize;
        (p4, p3, p2, p1)
    }
}

#[inline(always)]
pub fn flush_tlb(virt_addr: u64) {
    // SAFETY:
    // Invalidates the TLB entry for `virt_addr`.
    unsafe {
        asm!("invlpg [{}]", in(reg) virt_addr, options(nostack, preserves_flags));
    }
}

pub fn disable_smap_for_loader() {
    let mut cr4: u64;
    // SAFETY:
    // Clears CR4.SMAP (bit 21) at CPL=0 so the kernel ELF loader can populate user pages.
    unsafe {
        asm!("mov {}, cr4", out(reg) cr4, options(nostack, preserves_flags));
        cr4 &= !(1u64 << 21);
        asm!("mov cr4, {}", in(reg) cr4, options(nostack, preserves_flags));
    }
}

pub fn map_user_page(pmm: &mut BitmapFrameAllocator, user_va: u64) -> Result<u64, OstdError> {
    if (user_va & 0xFFF) != 0 {
        return Err(OstdError::UnalignedAddress);
    }

    let (p4_idx, p3_idx, p2_idx, p1_idx) = PageTable::indices(user_va);

    let cr3_raw: u64;
    // SAFETY:
    // Reads the active PML4 physical base address from CR3.
    unsafe {
        asm!("mov {}, cr3", out(reg) cr3_raw, options(nostack, preserves_flags));
    }
    let pml4_phys = cr3_raw & 0x000F_FFFF_FFFF_F000;
    let pml4_virt = pmm.phys_to_hhdm_virt(pml4_phys);

    let pdpt_phys = ensure_next_table(pmm, pml4_virt, p4_idx)?;
    let pdpt_virt = pmm.phys_to_hhdm_virt(pdpt_phys);

    let pd_phys = ensure_next_table(pmm, pdpt_virt, p3_idx)?;
    let pd_virt = pmm.phys_to_hhdm_virt(pd_phys);

    let pt_phys = ensure_next_table(pmm, pd_virt, p2_idx)?;
    let pt_virt = pmm.phys_to_hhdm_virt(pt_phys) as *mut PageTable;

    // SAFETY:
    // `pt_virt` is a valid HHDM-mapped level-1 page table and `p1_idx < 512`.
    // If the page is already mapped (e.g. shared page between adjacent GCC ELF segments),
    // preserve the existing frame instead of overwriting it.
    unsafe {
        let existing = (*pt_virt).entries[p1_idx];
        if (existing & PageFlags::PRESENT) != 0 {
            let clean_phys = existing & 0x000F_FFFF_FFFF_F000;
            (*pt_virt).entries[p1_idx] = clean_phys | PageFlags::USER_RWX;
            flush_tlb(user_va);
            return Ok(pmm.phys_to_hhdm_virt(clean_phys));
        }
    }

    let frame_kvirt = pmm.allocate_frame()?;
    let frame_phys = pmm.kernel_virt_to_phys(frame_kvirt);

    // SAFETY:
    // Installs the newly allocated physical frame into the level-1 page table entry.
    unsafe {
        (*pt_virt).entries[p1_idx] = (frame_phys & 0x000F_FFFF_FFFF_F000) | PageFlags::USER_RWX;
    }

    flush_tlb(user_va);
    Ok(frame_kvirt)
}

fn ensure_next_table(
    pmm: &mut BitmapFrameAllocator,
    table_virt_addr: u64,
    idx: usize,
) -> Result<u64, OstdError> {
    let table_virt = table_virt_addr as *mut PageTable;
    // SAFETY:
    // `table_virt` points to a valid page table via HHDM and `idx < 512`.
    unsafe {
        let entry = (*table_virt).entries[idx];
        if (entry & PageFlags::PRESENT) != 0 && (entry & PageFlags::HUGE_PAGE) == 0 {
            let phys = entry & 0x000F_FFFF_FFFF_F000;
            (*table_virt).entries[idx] = phys | PageFlags::USER_RWX;
            Ok(phys)
        } else {
            let new_table_kvirt = pmm.allocate_frame()?;
            let new_table_phys = pmm.kernel_virt_to_phys(new_table_kvirt);
            let clean_phys = new_table_phys & 0x000F_FFFF_FFFF_F000;
            (*table_virt).entries[idx] = clean_phys | PageFlags::USER_RWX;
            Ok(clean_phys)
        }
    }
}

pub fn copy_to_buffer(dst_addr: u64, src: &[u8]) -> Result<(), OstdError> {
    if dst_addr == 0 {
        return Err(OstdError::InvalidUserPointer);
    }
    // SAFETY:
    // `dst_addr` is non-null and mapped in the active address space.
    unsafe {
        core::ptr::copy_nonoverlapping(src.as_ptr(), dst_addr as *mut u8, src.len());
    }
    Ok(())
}

pub fn copy_from_buffer(src_addr: u64, dst: &mut [u8]) -> Result<(), OstdError> {
    if src_addr == 0 {
        return Err(OstdError::InvalidUserPointer);
    }
    // SAFETY:
    // `src_addr` is non-null and mapped in the active address space.
    unsafe {
        core::ptr::copy_nonoverlapping(src_addr as *const u8, dst.as_mut_ptr(), dst.len());
    }
    Ok(())
}
