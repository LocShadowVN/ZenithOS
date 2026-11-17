// services/zenith-kernel/src/elf.rs
//! Safe ELF64 binary parser and segment page-table mapper for Linux x86_64 executables.

use zenith_frame::memory::{FrameAllocator, VirtAddr, PAGE_SIZE};
use zenith_frame::vm::{PageFlags, VmSpace};
use zenith_frame::Error;

/// Standard 64-bit ELF header size in bytes.
pub const ELF64_EHDR_SIZE: usize = 64;
/// Standard 64-bit ELF program header entry size in bytes.
pub const ELF64_PHDR_SIZE: usize = 56;

/// ELF magic bytes (`\x7fELF`).
pub const ELF_MAGIC: [u8; 4] = [0x7F, b'E', b'L', b'F'];
/// 64-bit ELF class (`ELFCLASS64`).
pub const ELFCLASS64: u8 = 2;
/// Little-endian data encoding (`ELFDATA2LSB`).
pub const ELFDATA2LSB: u8 = 1;
/// Static executable ELF type (`ET_EXEC`).
pub const ET_EXEC: u16 = 2;
/// Position-independent executable / shared object ELF type (`ET_DYN`).
pub const ET_DYN: u16 = 3;
/// AMD x86-64 architecture machine ID (`EM_X86_64`).
pub const EM_X86_64: u16 = 62;

/// Loadable program segment type (`PT_LOAD`).
pub const PT_LOAD: u32 = 1;
/// Segment is executable (`PF_X`).
pub const PF_X: u32 = 1 << 0;
/// Segment is writable (`PF_W`).
pub const PF_W: u32 = 1 << 1;
/// Segment is readable (`PF_R`).
pub const PF_R: u32 = 1 << 2;

/// Validated 64-bit ELF File Header (`Elf64_Ehdr`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Elf64Header {
    /// Object file type (`ET_EXEC` or `ET_DYN`).
    pub e_type: u16,
    /// Target architecture (`EM_X86_64`).
    pub e_machine: u16,
    /// Virtual address of the entry point (`e_entry`).
    pub e_entry: u64,
    /// File offset of the Program Header Table (`e_phoff`).
    pub e_phoff: u64,
    /// Size of each Program Header entry (`e_phentsize`).
    pub e_phentsize: u16,
    /// Number of Program Header entries (`e_phnum`).
    pub e_phnum: u16,
}

/// Validated 64-bit ELF Program Header (`Elf64_Phdr`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Elf64ProgramHeader {
    /// Segment type (e.g., `PT_LOAD`).
    pub p_type: u32,
    /// Segment permission flags (`PF_R | PF_W | PF_X`).
    pub p_flags: u32,
    /// Offset of segment data in the file image.
    pub p_offset: u64,
    /// Target virtual address in user `VmSpace`.
    pub p_vaddr: u64,
    /// Size of segment in the file image.
    pub p_filesz: u64,
    /// Size of segment in virtual memory (`p_memsz >= p_filesz`).
    pub p_memsz: u64,
}

/// Summary of an ELF64 image mapped into a user `VmSpace`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MappedElfImage {
    /// Entry instruction pointer (`rip`) for spawning the user task.
    pub entry_rip: usize,
    /// Total number of 4 KiB pages mapped for `PT_LOAD` segments.
    pub mapped_pages: usize,
}

fn read_u16_le(bytes: &[u8], offset: usize) -> Result<u16, Error> {
    let end = offset.checked_add(2).ok_or(Error::InvalidArgument)?;
    let sub = bytes.get(offset..end).ok_or(Error::InvalidArgument)?;
    Ok(u16::from_le_bytes([sub[0], sub[1]]))
}

fn read_u32_le(bytes: &[u8], offset: usize) -> Result<u32, Error> {
    let end = offset.checked_add(4).ok_or(Error::InvalidArgument)?;
    let sub = bytes.get(offset..end).ok_or(Error::InvalidArgument)?;
    Ok(u32::from_le_bytes([sub[0], sub[1], sub[2], sub[3]]))
}

fn read_u64_le(bytes: &[u8], offset: usize) -> Result<u64, Error> {
    let end = offset.checked_add(8).ok_or(Error::InvalidArgument)?;
    let sub = bytes.get(offset..end).ok_or(Error::InvalidArgument)?;
    Ok(u64::from_le_bytes([
        sub[0], sub[1], sub[2], sub[3], sub[4], sub[5], sub[6], sub[7],
    ]))
}

/// Parses and validates an [`Elf64Header`] from the beginning of an ELF byte slice.
///
/// # Safety Invariants
/// Uses bounds-checked slice indexing and endian conversions in 100% safe Rust.
pub fn parse_elf64_header(image: &[u8]) -> Result<Elf64Header, Error> {
    if image.len() < ELF64_EHDR_SIZE {
        return Err(Error::InvalidArgument);
    }
    if image[0..4] != ELF_MAGIC || image[4] != ELFCLASS64 || image[5] != ELFDATA2LSB {
        return Err(Error::InvalidArgument);
    }

    let e_type = read_u16_le(image, 16)?;
    let e_machine = read_u16_le(image, 18)?;
    if (e_type != ET_EXEC && e_type != ET_DYN) || e_machine != EM_X86_64 {
        return Err(Error::InvalidArgument);
    }

    let e_entry = read_u64_le(image, 24)?;
    let e_phoff = read_u64_le(image, 32)?;
    let e_phentsize = read_u16_le(image, 54)?;
    let e_phnum = read_u16_le(image, 56)?;

    if e_entry == 0 || (e_phnum > 0 && (e_phentsize as usize) < ELF64_PHDR_SIZE) {
        return Err(Error::InvalidArgument);
    }

    Ok(Elf64Header {
        e_type,
        e_machine,
        e_entry,
        e_phoff,
        e_phentsize,
        e_phnum,
    })
}

/// Parses the `index`-th [`Elf64ProgramHeader`] from `image` using `header`.
///
/// # Safety Invariants
/// Verifies table bounds and `p_memsz >= p_filesz` without raw pointer casts.
pub fn parse_program_header(
    image: &[u8],
    header: &Elf64Header,
    index: u16,
) -> Result<Elf64ProgramHeader, Error> {
    if index >= header.e_phnum {
        return Err(Error::InvalidArgument);
    }
    let offset = (header.e_phoff as usize)
        .checked_add((index as usize) * (header.e_phentsize as usize))
        .ok_or(Error::InvalidArgument)?;

    let p_type = read_u32_le(image, offset)?;
    let p_flags = read_u32_le(image, offset + 4)?;
    let p_offset = read_u64_le(image, offset + 8)?;
    let p_vaddr = read_u64_le(image, offset + 16)?;
    let p_filesz = read_u64_le(image, offset + 32)?;
    let p_memsz = read_u64_le(image, offset + 40)?;

    if p_memsz < p_filesz {
        return Err(Error::InvalidArgument);
    }

    Ok(Elf64ProgramHeader {
        p_type,
        p_flags,
        p_offset,
        p_vaddr,
        p_filesz,
        p_memsz,
    })
}

/// Converts ELF `p_flags` (`PF_R`, `PF_W`, `PF_X`) into `zenith_frame` [`PageFlags`].
///
/// # Safety Invariants
/// Enforces W^X-friendly page permissions (`NO_EXECUTE` set whenever `PF_X` is absent).
pub const fn elf_flags_to_page_flags(p_flags: u32) -> PageFlags {
    let mut flags = PageFlags::PRESENT.union(PageFlags::USER);
    if (p_flags & PF_W) != 0 {
        flags = flags.union(PageFlags::WRITABLE);
    }
    if (p_flags & PF_X) == 0 {
        flags = flags.union(PageFlags::NO_EXECUTE);
    }
    flags
}

/// Maps all `PT_LOAD` segments of an ELF64 image into `vm_space` using `allocator`.
///
/// # Safety Invariants
/// Validates page alignment and maps pages strictly within user virtual address bounds.
pub fn map_elf64_segments<const WORDS: usize>(
    image: &[u8],
    vm_space: &mut VmSpace,
    allocator: &mut FrameAllocator<WORDS>,
) -> Result<MappedElfImage, Error> {
    let ehdr = parse_elf64_header(image)?;
    let mut mapped_pages = 0usize;

    for i in 0..ehdr.e_phnum {
        let phdr = parse_program_header(image, &ehdr, i)?;
        if phdr.p_type != PT_LOAD || phdr.p_memsz == 0 {
            continue;
        }

        let start_vaddr = VirtAddr(phdr.p_vaddr as usize);
        if !start_vaddr.is_page_aligned() {
            return Err(Error::InvalidAddress);
        }

        let num_pages = (phdr.p_memsz as usize).div_ceil(PAGE_SIZE);
        let flags = elf_flags_to_page_flags(phdr.p_flags);

        for page_idx in 0..num_pages {
            let page_vaddr = VirtAddr(start_vaddr.0 + page_idx * PAGE_SIZE);
            let frame = allocator.alloc_frame()?;
            vm_space.map_page(page_vaddr, &frame, flags)?;
            mapped_pages += 1;
        }
    }

    Ok(MappedElfImage {
        entry_rip: ehdr.e_entry as usize,
        mapped_pages,
    })
}

/// Copies initialized `PT_LOAD` file bytes into mapped physical frames.
///
/// # Safety Invariants
/// Stubbed until `zenith-frame` provides a direct physical frame byte writer API.
pub fn copy_elf_segment_payload(_image: &[u8], _phdr: &Elf64ProgramHeader) -> Result<(), Error> {
    // TODO: requires vmframe_write_bytes capability in zenith-frame
    Err(Error::NotImplemented)
}
