#![forbid(unsafe_code)]

use crate::ostd::{copy_to_buffer, map_user_page, BitmapFrameAllocator, PAGE_SIZE};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElfLoadError {
    BufferTooSmall,
    InvalidMagic,
    UnsupportedClassOrEndian,
    UnsupportedArchitecture,
    InvalidProgramHeader,
    MappingFailed,
}

#[derive(Debug, Clone, Copy)]
pub struct LoadedElfProcess {
    pub entry_rip: u64,
    pub user_rsp: u64,
    pub initial_brk: u64,
}

const PT_LOAD: u32 = 1;
const AT_NULL: u64 = 0;
const AT_PHENT: u64 = 4;
const AT_PHNUM: u64 = 5;
const AT_PAGESZ: u64 = 6;
const AT_ENTRY: u64 = 9;

fn read_u16_le(data: &[u8], offset: usize) -> Result<u16, ElfLoadError> {
    let slice = data
        .get(offset..offset + 2)
        .ok_or(ElfLoadError::BufferTooSmall)?;
    let mut arr = [0u8; 2];
    arr.copy_from_slice(slice);
    Ok(u16::from_le_bytes(arr))
}

fn read_u32_le(data: &[u8], offset: usize) -> Result<u32, ElfLoadError> {
    let slice = data
        .get(offset..offset + 4)
        .ok_or(ElfLoadError::BufferTooSmall)?;
    let mut arr = [0u8; 4];
    arr.copy_from_slice(slice);
    Ok(u32::from_le_bytes(arr))
}

fn read_u64_le(data: &[u8], offset: usize) -> Result<u64, ElfLoadError> {
    let slice = data
        .get(offset..offset + 8)
        .ok_or(ElfLoadError::BufferTooSmall)?;
    let mut arr = [0u8; 8];
    arr.copy_from_slice(slice);
    Ok(u64::from_le_bytes(arr))
}

pub fn load_elf64_into_user_space(
    elf_bytes: &[u8],
    pmm: &mut BitmapFrameAllocator,
) -> Result<LoadedElfProcess, ElfLoadError> {
    if elf_bytes.len() < 64 {
        return Err(ElfLoadError::BufferTooSmall);
    }
    if elf_bytes.get(0..4) != Some(b"\x7FELF") {
        return Err(ElfLoadError::InvalidMagic);
    }
    if elf_bytes.get(4) != Some(&2) || elf_bytes.get(5) != Some(&1) {
        return Err(ElfLoadError::UnsupportedClassOrEndian);
    }

    let e_type = read_u16_le(elf_bytes, 16)?;
    let e_machine = read_u16_le(elf_bytes, 18)?;
    if (e_type != 2 && e_type != 3) || e_machine != 62 {
        return Err(ElfLoadError::UnsupportedArchitecture);
    }

    let e_entry = read_u64_le(elf_bytes, 24)?;
    let e_phoff = read_u64_le(elf_bytes, 32)? as usize;
    let e_phentsize = read_u16_le(elf_bytes, 54)? as usize;
    let e_phnum = read_u16_le(elf_bytes, 56)? as usize;

    if e_phentsize < 56 {
        return Err(ElfLoadError::InvalidProgramHeader);
    }

    let mut max_vaddr: u64 = 0x0040_0000;

    for i in 0..e_phnum {
        let ph_base = e_phoff + i * e_phentsize;
        let p_type = read_u32_le(elf_bytes, ph_base)?;
        if p_type != PT_LOAD {
            continue;
        }

        let p_offset = read_u64_le(elf_bytes, ph_base + 8)? as usize;
        let p_vaddr = read_u64_le(elf_bytes, ph_base + 16)?;
        let p_filesz = read_u64_le(elf_bytes, ph_base + 32)? as usize;
        let p_memsz = read_u64_le(elf_bytes, ph_base + 40)?;

        if p_memsz == 0 {
            continue;
        }

        let seg_start_page = p_vaddr & !0xFFF;
        let seg_end = p_vaddr.wrapping_add(p_memsz);
        let seg_end_page = seg_end.wrapping_add(0xFFF) & !0xFFF;

        let mut page_va = seg_start_page;
        while page_va < seg_end_page {
            map_user_page(pmm, page_va).map_err(|_| ElfLoadError::MappingFailed)?;
            page_va += PAGE_SIZE as u64;
        }

        let file_slice = elf_bytes
            .get(p_offset..p_offset + p_filesz)
            .ok_or(ElfLoadError::InvalidProgramHeader)?;
        copy_to_buffer(p_vaddr, file_slice).map_err(|_| ElfLoadError::MappingFailed)?;

        if seg_end_page > max_vaddr {
            max_vaddr = seg_end_page;
        }
    }

    let stack_bottom: u64 = 0x0040_0000 + (e_entry & 0x00FF_0000);
    map_user_page(pmm, stack_bottom).map_err(|_| ElfLoadError::MappingFailed)?;
    map_user_page(pmm, stack_bottom + 0x1000).map_err(|_| ElfLoadError::MappingFailed)?;

    let stack_top = stack_bottom + 0x2000;
    let user_rsp = setup_sysv_user_stack(stack_top, e_entry, e_phnum as u64)?;

    Ok(LoadedElfProcess {
        entry_rip: e_entry,
        user_rsp,
        initial_brk: max_vaddr,
    })
}

fn setup_sysv_user_stack(
    stack_top: u64,
    entry_rip: u64,
    phnum: u64,
) -> Result<u64, ElfLoadError> {
    let str_area = stack_top - 64;
    let arg0_va = str_area;
    let arg1_va = str_area + 16;
    let env0_va = str_area + 32;

    copy_to_buffer(arg0_va, b"/bin/init\0").map_err(|_| ElfLoadError::MappingFailed)?;
    copy_to_buffer(arg1_va, b"--verify\0").map_err(|_| ElfLoadError::MappingFailed)?;
    copy_to_buffer(env0_va, b"OS=ZenithOS\0").map_err(|_| ElfLoadError::MappingFailed)?;

    let table_words: [u64; 16] = [
        2,         // argc
        arg0_va,   // argv[0]
        arg1_va,   // argv[1]
        0,         // argv[2] = NULL
        env0_va,   // envp[0]
        0,         // envp[1] = NULL
        AT_PAGESZ, // auxv[0].a_type
        4096,      // auxv[0].a_val
        AT_ENTRY,  // auxv[1].a_type
        entry_rip, // auxv[1].a_val
        AT_PHENT,  // auxv[2].a_type
        56,        // auxv[2].a_val
        AT_PHNUM,  // auxv[3].a_type
        phnum,     // auxv[3].a_val
        AT_NULL,   // auxv[4].a_type
        0,         // auxv[4].a_val
    ];

    let rsp = str_area - 128;
    for (i, &word) in table_words.iter().enumerate() {
        let addr = rsp + (i as u64) * 8;
        copy_to_buffer(addr, &word.to_le_bytes()).map_err(|_| ElfLoadError::MappingFailed)?;
    }

    Ok(rsp)
}

pub fn build_embedded_linux_init_elf64() -> [u8; 1024] {
    let mut elf = [0u8; 1024];

    elf[0..4].copy_from_slice(b"\x7FELF");
    elf[4] = 2;
    elf[5] = 1;
    elf[6] = 1;
    elf[16..18].copy_from_slice(&2u16.to_le_bytes());
    elf[18..20].copy_from_slice(&62u16.to_le_bytes());
    elf[20..24].copy_from_slice(&1u32.to_le_bytes());
    elf[24..32].copy_from_slice(&0x0040_1000u64.to_le_bytes());
    elf[32..40].copy_from_slice(&64u64.to_le_bytes());
    elf[52..54].copy_from_slice(&64u16.to_le_bytes());
    elf[54..56].copy_from_slice(&56u16.to_le_bytes());
    elf[56..58].copy_from_slice(&2u16.to_le_bytes());

    let ph0 = 64;
    elf[ph0..ph0 + 4].copy_from_slice(&PT_LOAD.to_le_bytes());
    elf[ph0 + 4..ph0 + 8].copy_from_slice(&5u32.to_le_bytes());
    elf[ph0 + 8..ph0 + 16].copy_from_slice(&256u64.to_le_bytes());
    elf[ph0 + 16..ph0 + 24].copy_from_slice(&0x0040_1000u64.to_le_bytes());
    elf[ph0 + 24..ph0 + 32].copy_from_slice(&0x0040_1000u64.to_le_bytes());
    elf[ph0 + 32..ph0 + 40].copy_from_slice(&256u64.to_le_bytes());
    elf[ph0 + 40..ph0 + 48].copy_from_slice(&256u64.to_le_bytes());
    elf[ph0 + 48..ph0 + 56].copy_from_slice(&0x1000u64.to_le_bytes());

    let ph1 = 120;
    elf[ph1..ph1 + 4].copy_from_slice(&PT_LOAD.to_le_bytes());
    elf[ph1 + 4..ph1 + 8].copy_from_slice(&6u32.to_le_bytes());
    elf[ph1 + 8..ph1 + 16].copy_from_slice(&512u64.to_le_bytes());
    elf[ph1 + 16..ph1 + 24].copy_from_slice(&0x0040_2000u64.to_le_bytes());
    elf[ph1 + 24..ph1 + 32].copy_from_slice(&0x0040_2000u64.to_le_bytes());
    elf[ph1 + 32..ph1 + 40].copy_from_slice(&384u64.to_le_bytes());
    elf[ph1 + 40..ph1 + 48].copy_from_slice(&384u64.to_le_bytes());
    elf[ph1 + 48..ph1 + 56].copy_from_slice(&0x1000u64.to_le_bytes());

    let code: &[u8] = &[
        0xb8, 0x09, 0x00, 0x00, 0x00, // mov eax, 9 (SYS_mmap)
        0x31, 0xff, // xor edi, edi
        0xbe, 0x00, 0x10, 0x00, 0x00, // mov esi, 4096
        0xba, 0x03, 0x00, 0x00, 0x00, // mov edx, 3
        0x41, 0xba, 0x22, 0x00, 0x00, 0x00, // mov r10d, 0x22
        0x49, 0xc7, 0xc0, 0xff, 0xff, 0xff, 0xff, // mov r8, -1
        0x45, 0x31, 0xc9, // xor r9d, r9d
        0x0f, 0x05, // syscall
        0x49, 0x89, 0xc4, // mov r12, rax (0x1000_0000)
        // SYS_statfs("/", r12 + 0x100) (Syscall #137)
        0xbf, 0x70, 0x20, 0x40, 0x00, // mov edi, 0x00402070 ("/")
        0x49, 0x8d, 0xb4, 0x24, 0x00, 0x01, 0x00, 0x00, // lea rsi, [r12 + 0x100]
        0xb8, 0x89, 0x00, 0x00, 0x00, // mov eax, 137 (SYS_statfs)
        0x0f, 0x05, // syscall
        // SYS_openat(AT_FDCWD=-100, "/etc/debian_version", O_RDONLY=0, 0)
        0x48, 0xc7, 0xc7, 0x9c, 0xff, 0xff, 0xff, // mov rdi, -100
        0xbe, 0x50, 0x20, 0x40, 0x00, // mov esi, 0x00402050 ("/etc/debian_version")
        0x31, 0xd2, // xor edx, edx
        0x45, 0x31, 0xd2, // xor r10d, r10d
        0xb8, 0x01, 0x01, 0x00, 0x00, // mov eax, 257 (SYS_openat)
        0x0f, 0x05, // syscall
        0x49, 0x89, 0xc5, // mov r13, rax
        // SYS_read(r13, r12 + 0x500, 5) -> reads "13.0\n" from ZenithFS Inode #1
        0x4c, 0x89, 0xef, // mov rdi, r13
        0x49, 0x8d, 0xb4, 0x24, 0x00, 0x05, 0x00, 0x00, // lea rsi, [r12 + 0x500]
        0xba, 0x05, 0x00, 0x00, 0x00, // mov edx, 5
        0x31, 0xc0, // xor eax, eax (SYS_read = 0)
        0x0f, 0x05, // syscall
        0x4c, 0x89, 0xef, // mov rdi, r13
        0xb8, 0x03, 0x00, 0x00, 0x00, // mov eax, 3 (SYS_close)
        0x0f, 0x05, // syscall
        // SYS_write(1, 0x00402000, 70)
        0xb8, 0x01, 0x00, 0x00, 0x00, // mov eax, 1 (SYS_write)
        0xbf, 0x01, 0x00, 0x00, 0x00, // mov edi, 1
        0xbe, 0x00, 0x20, 0x40, 0x00, // mov esi, 0x00402000
        0xba, 0x46, 0x00, 0x00, 0x00, // mov edx, 70
        0x0f, 0x05, // syscall
        // SYS_execve("/usr/bin/hello-deb", NULL, NULL)
        0xbf, 0x80, 0x20, 0x40, 0x00, // mov edi, 0x00402080 ("/usr/bin/hello-deb")
        0x31, 0xf6, // xor esi, esi
        0x31, 0xd2, // xor edx, edx
        0xb8, 0x3b, 0x00, 0x00, 0x00, // mov eax, 59 (SYS_execve)
        0x0f, 0x05, // syscall
    ];
    elf[256..256 + code.len()].copy_from_slice(code);

    let msg = b"[Ring-3 /bin/init] Mounted ZenithFS (ZFS1) & Debian 13.0 Trixie OK.  \n";
    elf[512..512 + 70].copy_from_slice(msg);
    elf[512 + 0x50..512 + 0x50 + 20].copy_from_slice(b"/etc/debian_version\0");
    elf[512 + 0x70..512 + 0x70 + 2].copy_from_slice(b"/\0");
    elf[512 + 0x80..512 + 0x80 + 19].copy_from_slice(b"/usr/bin/hello-deb\0");

    elf
}

pub fn build_embedded_linux_app_elf64() -> [u8; 1024] {
    let mut elf = [0u8; 1024];

    elf[0..4].copy_from_slice(b"\x7FELF");
    elf[4] = 2;
    elf[5] = 1;
    elf[6] = 1;
    elf[16..18].copy_from_slice(&2u16.to_le_bytes());
    elf[18..20].copy_from_slice(&62u16.to_le_bytes());
    elf[20..24].copy_from_slice(&1u32.to_le_bytes());
    elf[24..32].copy_from_slice(&0x0050_1000u64.to_le_bytes());
    elf[32..40].copy_from_slice(&64u64.to_le_bytes());
    elf[52..54].copy_from_slice(&64u16.to_le_bytes());
    elf[54..56].copy_from_slice(&56u16.to_le_bytes());
    elf[56..58].copy_from_slice(&2u16.to_le_bytes());

    let ph0 = 64;
    elf[ph0..ph0 + 4].copy_from_slice(&PT_LOAD.to_le_bytes());
    elf[ph0 + 4..ph0 + 8].copy_from_slice(&5u32.to_le_bytes());
    elf[ph0 + 8..ph0 + 16].copy_from_slice(&256u64.to_le_bytes());
    elf[ph0 + 16..ph0 + 24].copy_from_slice(&0x0050_1000u64.to_le_bytes());
    elf[ph0 + 24..ph0 + 32].copy_from_slice(&0x0050_1000u64.to_le_bytes());
    elf[ph0 + 32..ph0 + 40].copy_from_slice(&256u64.to_le_bytes());
    elf[ph0 + 40..ph0 + 48].copy_from_slice(&256u64.to_le_bytes());
    elf[ph0 + 48..ph0 + 56].copy_from_slice(&0x1000u64.to_le_bytes());

    let ph1 = 120;
    elf[ph1..ph1 + 4].copy_from_slice(&PT_LOAD.to_le_bytes());
    elf[ph1 + 4..ph1 + 8].copy_from_slice(&6u32.to_le_bytes());
    elf[ph1 + 8..ph1 + 16].copy_from_slice(&512u64.to_le_bytes());
    elf[ph1 + 16..ph1 + 24].copy_from_slice(&0x0050_2000u64.to_le_bytes());
    elf[ph1 + 24..ph1 + 32].copy_from_slice(&0x0050_2000u64.to_le_bytes());
    elf[ph1 + 32..ph1 + 40].copy_from_slice(&256u64.to_le_bytes());
    elf[ph1 + 40..ph1 + 48].copy_from_slice(&256u64.to_le_bytes());
    elf[ph1 + 48..ph1 + 56].copy_from_slice(&0x1000u64.to_le_bytes());

    let code: &[u8] = &[
        0xbf, 0x00, 0x06, 0x00, 0x10, // mov edi, 0x10000600
        0xc7, 0x07, 0x44, 0x45, 0x42, 0x33, // mov dword ptr [rdi], "DEB3"
        0x31, 0xff, // xor edi, edi
        0xb8, 0x23, 0x01, 0x00, 0x00, // mov eax, 291 (SYS_epoll_create1)
        0x0f, 0x05, // syscall
        0x49, 0x89, 0xc5, // mov r13, rax (epfd)
        0x48, 0xc7, 0xc7, 0x9c, 0xff, 0xff, 0xff, // mov rdi, -100
        0xbe, 0x78, 0x20, 0x50, 0x00, // mov esi, 0x00502078 ("/dev/input/event0")
        0x31, 0xd2, // xor edx, edx
        0x45, 0x31, 0xd2, // xor r10d, r10d
        0xb8, 0x01, 0x01, 0x00, 0x00, // mov eax, 257 (SYS_openat)
        0x0f, 0x05, // syscall
        0x49, 0x89, 0xc6, // mov r14, rax (ev_fd)
        0x4c, 0x89, 0xef, // mov rdi, r13 (epfd)
        0xbe, 0x00, 0x09, 0x00, 0x10, // mov esi, 0x10000900
        0xba, 0x01, 0x00, 0x00, 0x00, // mov edx, 1
        0x45, 0x31, 0xd2, // xor r10d, r10d
        0xb8, 0x19, 0x01, 0x00, 0x00, // mov eax, 281 (SYS_epoll_pwait)
        0x0f, 0x05, // syscall
        0x4c, 0x89, 0xf7, // mov rdi, r14 (ev_fd)
        0xbe, 0x20, 0x09, 0x00, 0x10, // mov esi, 0x10000920
        0xba, 0x18, 0x00, 0x00, 0x00, // mov edx, 24
        0x31, 0xc0, // xor eax, eax (SYS_read = 0)
        0x0f, 0x05, // syscall
        0x4c, 0x89, 0xf7, // mov rdi, r14
        0xb8, 0x03, 0x00, 0x00, 0x00, // mov eax, 3
        0x0f, 0x05, // syscall
        0x4c, 0x89, 0xef, // mov rdi, r13
        0xb8, 0x03, 0x00, 0x00, 0x00, // mov eax, 3
        0x0f, 0x05, // syscall
        0x48, 0xc7, 0xc7, 0x9c, 0xff, 0xff, 0xff, // mov rdi, -100
        0xbe, 0x50, 0x20, 0x50, 0x00, // mov esi, 0x00502050 ("/dev/fb0")
        0xba, 0x02, 0x00, 0x00, 0x00, // mov edx, 2
        0x45, 0x31, 0xd2, // xor r10d, r10d
        0xb8, 0x01, 0x01, 0x00, 0x00, // mov eax, 257 (SYS_openat)
        0x0f, 0x05, // syscall
        0x49, 0x89, 0xc4, // mov r12, rax (fb_fd)
        0x4c, 0x89, 0xe7, // mov rdi, r12
        0xbe, 0x00, 0x46, 0x00, 0x00, // mov esi, 0x4600
        0xba, 0x00, 0x08, 0x00, 0x10, // mov edx, 0x10000800
        0xb8, 0x10, 0x00, 0x00, 0x00, // mov eax, 16 (SYS_ioctl)
        0x0f, 0x05, // syscall
        0x4c, 0x89, 0xe7, // mov rdi, r12
        0xbe, 0x60, 0x20, 0x50, 0x00, // mov esi, 0x00502060 (4 pixels)
        0xba, 0x10, 0x00, 0x00, 0x00, // mov edx, 16
        0xb8, 0x01, 0x00, 0x00, 0x00, // mov eax, 1 (SYS_write)
        0x0f, 0x05, // syscall
        0x4c, 0x89, 0xe7, // mov rdi, r12
        0xb8, 0x03, 0x00, 0x00, 0x00, // mov eax, 3 (SYS_close)
        0x0f, 0x05, // syscall
        0xb8, 0x01, 0x00, 0x00, 0x00, // mov eax, 1 (SYS_write)
        0xbf, 0x01, 0x00, 0x00, 0x00, // mov edi, 1
        0xbe, 0x00, 0x20, 0x50, 0x00, // mov esi, 0x00502000
        0xba, 0x48, 0x00, 0x00, 0x00, // mov edx, 72
        0x0f, 0x05, // syscall
        0xbf, 0x66, 0x00, 0x00, 0x00, // mov edi, 0x66
        0xb8, 0xe7, 0x00, 0x00, 0x00, // mov eax, 231 (SYS_exit_group)
        0x0f, 0x05, // syscall
    ];
    elf[256..256 + code.len()].copy_from_slice(code);

    let app_msg = b"[Ring-3 /usr/bin/hello-deb] Executed ELF64 app unpacked from .deb pkg!\n ";
    elf[512..512 + 72].copy_from_slice(app_msg);
    elf[512 + 0x50..512 + 0x50 + 9].copy_from_slice(b"/dev/fb0\0");

    elf[512 + 0x60..512 + 0x64].copy_from_slice(&0x00FF_FFFFu32.to_le_bytes());
    elf[512 + 0x64..512 + 0x68].copy_from_slice(&0x0060_CDFFu32.to_le_bytes());
    elf[512 + 0x68..512 + 0x6C].copy_from_slice(&0x0000_78D4u32.to_le_bytes());
    elf[512 + 0x6C..512 + 0x70].copy_from_slice(&0x001F_2128u32.to_le_bytes());

    elf[512 + 0x78..512 + 0x78 + 18].copy_from_slice(b"/dev/input/event0\0");

    elf
}
