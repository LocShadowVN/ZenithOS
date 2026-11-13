#![no_std]
#![no_main]
#![deny(unsafe_op_in_unsafe_fn)]
#![allow(clippy::fn_to_numeric_cast)]

use core::cell::UnsafeCell;
use core::panic::PanicInfo;
use core::sync::atomic::Ordering;

// ============================================================================
// GIAO THỨC KHỞI ĐỘNG LIMINE (HHDM & KERNEL PHYSICAL ADDRESS)
// ============================================================================
pub mod boot_protocol {
    use core::cell::UnsafeCell;

    #[repr(C)]
    pub struct LimineHhdmResponse {
        pub revision: u64,
        pub offset: u64,
    }

    #[repr(C)]
    pub struct LimineHhdmRequest {
        pub id: [u64; 4],
        pub revision: u64,
        pub response: *const LimineHhdmResponse,
    }

    #[repr(C)]
    pub struct LimineKernelAddressResponse {
        pub revision: u64,
        pub physical_base: u64,
        pub virtual_base: u64,
    }

    #[repr(C)]
    pub struct LimineKernelAddressRequest {
        pub id: [u64; 4],
        pub revision: u64,
        pub response: *const LimineKernelAddressResponse,
    }

    #[repr(C)]
    pub struct LimineBootRequests {
        pub start_marker: [u64; 4],
        pub base_revision: [u64; 3],
        pub hhdm: LimineHhdmRequest,
        pub kernel_addr: LimineKernelAddressRequest,
        pub end_marker: [u64; 2],
    }

    #[repr(transparent)]
    pub struct BootRequestsCell(pub UnsafeCell<LimineBootRequests>);

    // SAFETY:
    // Chỉ được ghi bởi Bootloader Limine trước khi vào `_start` và chỉ đọc trên BSP.
    unsafe impl Sync for BootRequestsCell {}

    #[used]
    #[link_section = ".data"]
    pub static LIMINE_REQUESTS: BootRequestsCell =
        BootRequestsCell(UnsafeCell::new(LimineBootRequests {
            start_marker: [
                0xf6b8_f4b3_9de7_d1ae,
                0xfab9_1a69_40fc_b9cf,
                0x785c_6ed0_15d3_e316,
                0x181e_920a_7852_b9d9,
            ],
            base_revision: [0xf956_2b2d_5c95_a6c8, 0x6a7b_3849_4453_6bdc, 2],
            hhdm: LimineHhdmRequest {
                id: [
                    0xc7b1_dd30_df4c_8b88,
                    0x0a82_e883_a194_f07b,
                    0x48dc_f1cb_8ad2_b852,
                    0x6398_4e95_9a98_244b,
                ],
                revision: 0,
                response: core::ptr::null(),
            },
            kernel_addr: LimineKernelAddressRequest {
                id: [
                    0xc7b1_dd30_df4c_8b88,
                    0x0a82_e883_a194_f07b,
                    0x71ba_7686_3cc5_5f63,
                    0xb264_4a48_c516_a487,
                ],
                revision: 0,
                response: core::ptr::null(),
            },
            end_marker: [0xadc0_e053_1bb1_0d03, 0x9572_709f_3176_4c62],
        }));
}

// ============================================================================
// MODULE 1: OSTD (OS STANDARD FRAMEWORK)
// Tầng cô lập phần cứng duy nhất được phép sử dụng `unsafe`.
// ============================================================================
pub mod ostd {
    use core::arch::{asm, global_asm};
    use core::fmt::{self, Write};
    use core::sync::atomic::{AtomicU64, Ordering};

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum OstdError {
        OutOfPhysicalFrames,
        InvalidFrameIndex,
        UnalignedAddress,
        PageTableWalkFailed,
        InvalidUserPointer,
        BootInfoMissing,
    }

    // ------------------------------------------------------------------------
    // 1.1: PORT I/O, RDTSC, SERIAL CONSOLE (UART 16550) & QEMU EXIT DEVICE
    // ------------------------------------------------------------------------
    #[inline(always)]
    pub fn outb(port: u16, val: u8) {
        // SAFETY:
        // Thực thi ở Ring 0 trên các cổng I/O kiến trúc PC chuẩn.
        unsafe {
            asm!(
                "out dx, al",
                in("dx") port,
                in("al") val,
                options(nostack, preserves_flags)
            );
        }
    }

    #[inline(always)]
    pub fn inb(port: u16) -> u8 {
        let val: u8;
        // SAFETY:
        // Thực thi ở Ring 0 trên các cổng I/O phần cứng chuẩn.
        unsafe {
            asm!(
                "in al, dx",
                out("al") val,
                in("dx") port,
                options(nostack, preserves_flags)
            );
        }
        val
    }

    #[inline(always)]
    pub fn outl(port: u16, val: u32) {
        // SAFETY:
        // Thực thi ở Ring 0 phục vụ ghi 32-bit ra cổng `isa-debug-exit` (0xF4).
        unsafe {
            asm!(
                "out dx, eax",
                in("dx") port,
                in("eax") val,
                options(nostack, preserves_flags)
            );
        }
    }

    #[inline(always)]
    pub fn read_tsc() -> u64 {
        let low: u32;
        let high: u32;
        // SAFETY:
        // `rdtsc` là lệnh đọc bộ đếm thời gian kiến trúc x86_64 không tác dụng phụ bộ nhớ.
        unsafe {
            asm!(
                "rdtsc",
                out("eax") low,
                out("edx") high,
                options(nostack, preserves_flags)
            );
        }
        ((high as u64) << 32) | (low as u64)
    }

    #[repr(u32)]
    #[derive(Debug, Clone, Copy)]
    pub enum QemuExitCode {
        Success = 0x10,
        Failed = 0x11,
    }

    pub fn exit_qemu(exit_code: QemuExitCode) -> ! {
        outl(0xF4, exit_code as u32);
        loop {
            // SAFETY:
            // Dừng lõi CPU nếu QEMU chưa đóng.
            unsafe {
                asm!("hlt", options(nostack, preserves_flags));
            }
        }
    }

    pub struct SerialConsole;

    impl SerialConsole {
        const COM1: u16 = 0x3F8;

        pub fn init() {
            outb(Self::COM1 + 1, 0x00);
            outb(Self::COM1 + 3, 0x80);
            outb(Self::COM1, 0x03);
            outb(Self::COM1 + 1, 0x00);
            outb(Self::COM1 + 3, 0x03);
            outb(Self::COM1 + 2, 0xC7);
            outb(Self::COM1 + 4, 0x0B);
        }

        #[inline]
        pub fn write_byte(byte: u8) {
            while (inb(Self::COM1 + 5) & 0x20) == 0 {
                core::hint::spin_loop();
            }
            outb(Self::COM1, byte);
        }

        pub fn write_bytes(bytes: &[u8]) {
            for &b in bytes {
                if b == b'\n' {
                    Self::write_byte(b'\r');
                }
                Self::write_byte(b);
            }
        }
    }

    impl Write for SerialConsole {
        fn write_str(&mut self, s: &str) -> fmt::Result {
            Self::write_bytes(s.as_bytes());
            Ok(())
        }
    }

    pub fn print_fmt(args: fmt::Arguments<'_>) {
        let mut console = SerialConsole;
        let _ = console.write_fmt(args);
    }

    // ------------------------------------------------------------------------
    // 1.2: GDT & TSS (CHUẨN BỐ CỤC CHO LỆNH SYSCALL / SYSRETQ)
    // ------------------------------------------------------------------------
    #[repr(C, packed)]
    pub struct TaskStateSegment {
        pub reserved_1: u32,
        pub privilege_stack_table: [u64; 3],
        pub reserved_2: u64,
        pub interrupt_stack_table: [u64; 7],
        pub reserved_3: u64,
        pub reserved_4: u16,
        pub iomap_base: u16,
    }

    #[repr(C, align(16))]
    pub struct AlignedStack(pub [u8; 16384]);

    static mut KERNEL_INTR_STACK: AlignedStack = AlignedStack([0; 16384]);
    static mut DOUBLE_FAULT_IST_STACK: AlignedStack = AlignedStack([0; 16384]);

    static mut TSS: TaskStateSegment = TaskStateSegment {
        reserved_1: 0,
        privilege_stack_table: [0; 3],
        reserved_2: 0,
        interrupt_stack_table: [0; 7],
        reserved_3: 0,
        reserved_4: 0,
        iomap_base: core::mem::size_of::<TaskStateSegment>() as u16,
    };

    #[repr(C, align(16))]
    pub struct GdtTable {
        pub entries: [u64; 7],
    }

    static mut GDT: GdtTable = GdtTable {
        entries: [
            0x0000_0000_0000_0000, // 0x00: Null Descriptor
            0x00AF_9A00_0000_FFFF, // 0x08: Kernel Code 64-bit (DPL = 0)
            0x00CF_9200_0000_FFFF, // 0x10: Kernel Data 64-bit (DPL = 0)
            0x00CF_F200_0000_FFFF, // 0x18: User Data 64-bit   (DPL = 3 -> Selector 0x1B)
            0x00AF_FA00_0000_FFFF, // 0x20: User Code 64-bit   (DPL = 3 -> Selector 0x23)
            0x0000_0000_0000_0000, // 0x28: TSS Low
            0x0000_0000_0000_0000, // 0x30: TSS High
        ],
    };

    #[repr(C, packed)]
    struct DescriptorTablePointer {
        limit: u16,
        base: u64,
    }

    pub fn init_gdt_and_tss() {
        // SAFETY:
        // Khởi tạo GDT/TSS duy nhất 1 lần trên BSP thông qua con trỏ thô `&raw mut`.
        unsafe {
            let rsp0_top = (&raw const KERNEL_INTR_STACK.0 as u64) + 16384;
            let ist1_top = (&raw const DOUBLE_FAULT_IST_STACK.0 as u64) + 16384;

            let tss_ptr = &raw mut TSS;
            (*tss_ptr).privilege_stack_table[0] = rsp0_top;
            (*tss_ptr).interrupt_stack_table[0] = ist1_top;

            let tss_base = tss_ptr as u64;
            let tss_limit = (core::mem::size_of::<TaskStateSegment>() - 1) as u64;

            let mut tss_low: u64 = 0;
            tss_low |= tss_limit & 0xFFFF;
            tss_low |= (tss_base & 0xFF_FFFF) << 16;
            tss_low |= 0x89u64 << 40;
            tss_low |= ((tss_limit >> 16) & 0xF) << 48;
            tss_low |= ((tss_base >> 24) & 0xFF) << 56;

            let tss_high: u64 = tss_base >> 32;

            let gdt_ptr = &raw mut GDT;
            (*gdt_ptr).entries[5] = tss_low;
            (*gdt_ptr).entries[6] = tss_high;

            let gdtr = DescriptorTablePointer {
                limit: (core::mem::size_of::<GdtTable>() - 1) as u16,
                base: gdt_ptr as u64,
            };

            asm!(
                "lgdt [{gdtr}]",
                "push 0x08",
                "lea {tmp}, [rip + 2f]",
                "push {tmp}",
                "retfq",
                "2:",
                "mov ax, 0x10",
                "mov ds, ax",
                "mov es, ax",
                "mov ss, ax",
                "xor ax, ax",
                "mov fs, ax",
                "mov gs, ax",
                "mov ax, 0x28",
                "ltr ax",
                gdtr = in(reg) &gdtr,
                tmp = lateout(reg) _,
                out("ax") _,
                options(preserves_flags)
            );
        }
    }

    // ------------------------------------------------------------------------
    // 1.3: IDT (INTERRUPT DESCRIPTOR TABLE)
    // ------------------------------------------------------------------------
    #[repr(C, packed)]
    #[derive(Clone, Copy)]
    pub struct IdtEntry {
        pub offset_low: u16,
        pub selector: u16,
        pub ist: u8,
        pub type_attr: u8,
        pub offset_mid: u16,
        pub offset_high: u32,
        pub reserved: u32,
    }

    impl IdtEntry {
        pub const fn missing() -> Self {
            Self {
                offset_low: 0,
                selector: 0,
                ist: 0,
                type_attr: 0,
                offset_mid: 0,
                offset_high: 0,
                reserved: 0,
            }
        }

        pub fn new_handler(handler_addr: u64, ist_index: u8) -> Self {
            Self {
                offset_low: handler_addr as u16,
                selector: 0x08,
                ist: ist_index & 0x7,
                type_attr: 0x8E,
                offset_mid: (handler_addr >> 16) as u16,
                offset_high: (handler_addr >> 32) as u32,
                reserved: 0,
            }
        }
    }

    #[repr(C, align(16))]
    pub struct IdtTable {
        pub entries: [IdtEntry; 256],
    }

    static mut IDT: IdtTable = IdtTable {
        entries: [IdtEntry::missing(); 256],
    };

    pub static BREAKPOINT_HIT_COUNT: AtomicU64 = AtomicU64::new(0);

    #[no_mangle]
    extern "C" fn zenith_breakpoint_handler() {
        BREAKPOINT_HIT_COUNT.fetch_add(1, Ordering::SeqCst);
    }

    #[no_mangle]
    extern "C" fn zenith_unhandled_exception(vector: u64, error_code: u64, rip: u64) -> ! {
        let cr2: u64;
        // SAFETY:
        // Đọc thanh ghi CR2 để chẩn đoán địa chỉ gây lỗi nếu là Page Fault (#PF).
        unsafe {
            asm!("mov {}, cr2", out(reg) cr2, options(nostack, preserves_flags));
        }
        let mut console = SerialConsole;
        let _ = writeln!(
            console,
            "\n[ZENITH-OSTD][FATAL] CPU Exception #{} (err=0x{:x}, cr2=0x{:x}) at RIP=0x{:016x}",
            vector, error_code, cr2, rip
        );
        exit_qemu(QemuExitCode::Failed);
    }

    global_asm!(
        ".global isr_breakpoint_stub",
        "isr_breakpoint_stub:",
        "push rax",
        "push rcx",
        "push rdx",
        "push rsi",
        "push rdi",
        "push r8",
        "push r9",
        "push r10",
        "push r11",
        "call zenith_breakpoint_handler",
        "pop r11",
        "pop r10",
        "pop r9",
        "pop r8",
        "pop rdi",
        "pop rsi",
        "pop rdx",
        "pop rcx",
        "pop rax",
        "iretq",
        ".global isr_double_fault_stub",
        "isr_double_fault_stub:",
        "mov rdi, 8",
        "pop rsi",
        "mov rdx, [rsp]",
        "call zenith_unhandled_exception",
        ".global isr_gp_fault_stub",
        "isr_gp_fault_stub:",
        "mov rdi, 13",
        "pop rsi",
        "mov rdx, [rsp]",
        "call zenith_unhandled_exception",
        ".global isr_page_fault_stub",
        "isr_page_fault_stub:",
        "mov rdi, 14",
        "pop rsi",
        "mov rdx, [rsp]",
        "call zenith_unhandled_exception",
    );

    extern "C" {
        fn isr_breakpoint_stub();
        fn isr_double_fault_stub();
        fn isr_gp_fault_stub();
        fn isr_page_fault_stub();
    }

    pub fn init_idt() {
        // SAFETY:
        // Đăng ký các cổng ngắt kiến trúc chuẩn vào bảng IDT trên BSP.
        unsafe {
            let idt_ptr = &raw mut IDT;
            (*idt_ptr).entries[3] =
                IdtEntry::new_handler(isr_breakpoint_stub as *const () as u64, 0);
            (*idt_ptr).entries[8] =
                IdtEntry::new_handler(isr_double_fault_stub as *const () as u64, 1);
            (*idt_ptr).entries[13] =
                IdtEntry::new_handler(isr_gp_fault_stub as *const () as u64, 0);
            (*idt_ptr).entries[14] =
                IdtEntry::new_handler(isr_page_fault_stub as *const () as u64, 0);

            let idtr = DescriptorTablePointer {
                limit: (core::mem::size_of::<IdtTable>() - 1) as u16,
                base: idt_ptr as u64,
            };

            asm!("lidt [{}]", in(reg) &idtr, options(nostack, preserves_flags));
        }
    }

    pub fn trigger_breakpoint_self_test() {
        // SAFETY:
        // Gọi ngắt phần mềm Vector #3 đã được cấu hình trong IDT.
        unsafe {
            asm!("int3", options(nostack, preserves_flags));
        }
    }

    // ------------------------------------------------------------------------
    // 1.4: QUẢN LÝ BỘ NHỚ VẬT LÝ (PMM) & BẢNG TRANG 4 CẤP USER-SPACE (MMU)
    // ------------------------------------------------------------------------
    pub const PAGE_SIZE: usize = 4096;
    const POOL_FRAMES: usize = 256;

    #[repr(C, align(4096))]
    struct FramePool([[u8; PAGE_SIZE]; POOL_FRAMES]);

    static mut STATIC_FRAME_POOL: FramePool = FramePool([[0; PAGE_SIZE]; POOL_FRAMES]);

    pub struct BitmapFrameAllocator {
        bitmap: [u64; 4],
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
                bitmap: [0; 4],
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
            // Sử dụng `read_volatile` trên vùng `UnsafeCell` để ngăn trình biên dịch LLVM
            // tối ưu hóa hằng số (constant-fold) các con trỏ phản hồi của Limine thành `null`.
            unsafe {
                let req_ptr = crate::boot_protocol::LIMINE_REQUESTS.0.get();
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
                bitmap: [0; 4],
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
                    // `addr` nằm trong `STATIC_FRAME_POOL` đã căn lề 4KiB và thuộc quyền sở hữu độc quyền.
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
        // Làm mới bộ đệm TLB của trang ảo vừa cập nhật.
        unsafe {
            asm!("invlpg [{}]", in(reg) virt_addr, options(nostack, preserves_flags));
        }
    }

    pub fn disable_smap_for_loader() {
        let mut cr4: u64;
        // SAFETY:
        // Đọc và xóa bit SMAP (bit 21) trên thanh ghi điều khiển CR4 ở Ring 0.
        unsafe {
            asm!("mov {}, cr4", out(reg) cr4, options(nostack, preserves_flags));
            cr4 &= !(1u64 << 21);
            asm!("mov cr4, {}", in(reg) cr4, options(nostack, preserves_flags));
        }
    }

    /// Ánh xạ một trang 4KiB tại địa chỉ ảo User-space (`user_va`) vào bảng trang 4 cấp `CR3` hiện hành.
    pub fn map_user_page(pmm: &mut BitmapFrameAllocator, user_va: u64) -> Result<u64, OstdError> {
        if (user_va & 0xFFF) != 0 {
            return Err(OstdError::UnalignedAddress);
        }

        let frame_kvirt = pmm.allocate_frame()?;
        let frame_phys = pmm.kernel_virt_to_phys(frame_kvirt);
        let (p4_idx, p3_idx, p2_idx, p1_idx) = PageTable::indices(user_va);

        let cr3_raw: u64;
        // SAFETY:
        // Đọc địa chỉ vật lý của bảng PML4 hiện tại từ thanh ghi CR3.
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
        // `pt_virt` trỏ tới bảng Page Table cấp 1 hợp lệ qua HHDM và `p1_idx < 512`.
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
        // `table_virt` là địa chỉ ảo HHDM của bảng trang hiện tại, `idx` nằm trong khoảng `0..512`.
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

    // ------------------------------------------------------------------------
    // 1.5: MSR, PER-CPU STATE & RING 0 <-> RING 3 CONTEXT SWITCHING
    // ------------------------------------------------------------------------
    #[repr(u32)]
    #[derive(Debug, Clone, Copy)]
    pub enum Msr {
        Ia32Efer = 0xC000_0080,
        Ia32Star = 0xC000_0081,
        Ia32Lstar = 0xC000_0082,
        Ia32SfMask = 0xC000_0084,
        Ia32FsBase = 0xC000_0100,
        Ia32GsBase = 0xC000_0101,
        Ia32KernelGsBase = 0xC000_0102,
    }

    #[inline(always)]
    pub fn write_msr(msr: Msr, value: u64) {
        let low = value as u32;
        let high = (value >> 32) as u32;
        // SAFETY:
        // Ghi vào các MSR kiến trúc chuẩn ở Ring 0.
        unsafe {
            asm!(
                "wrmsr",
                in("ecx") msr as u32,
                in("eax") low,
                in("edx") high,
                options(nostack, preserves_flags)
            );
        }
    }

    #[inline(always)]
    pub fn read_msr(msr: Msr) -> u64 {
        let low: u32;
        let high: u32;
        // SAFETY:
        // Đọc các MSR kiến trúc chuẩn ở Ring 0.
        unsafe {
            asm!(
                "rdmsr",
                in("ecx") msr as u32,
                out("eax") low,
                out("edx") high,
                options(nostack, preserves_flags)
            );
        }
        ((high as u64) << 32) | (low as u64)
    }

    #[repr(C, align(64))]
    pub struct PerCpuData {
        pub kernel_rsp: u64,
        pub user_rsp_scratch: u64,
        pub kernel_return_rsp: u64,
    }

    static mut BSP_PER_CPU: PerCpuData = PerCpuData {
        kernel_rsp: 0,
        user_rsp_scratch: 0,
        kernel_return_rsp: 0,
    };

    #[repr(C, align(16))]
    #[derive(Debug, Clone, Copy, Default)]
    pub struct TrapFrame {
        pub r15: u64,
        pub r14: u64,
        pub r13: u64,
        pub r12: u64,
        pub rbp: u64,
        pub rbx: u64,
        pub r11: u64,
        pub r10: u64,
        pub r9: u64,
        pub r8: u64,
        pub rax: u64,
        pub rcx: u64,
        pub rdx: u64,
        pub rsi: u64,
        pub rdi: u64,
        pub rsp: u64,
    }

    global_asm!(
        ".global zenith_syscall_trampoline",
        "zenith_syscall_trampoline:",
        "swapgs",
        "mov gs:[0x08], rsp",
        "mov rsp, gs:[0x00]",
        "push qword ptr gs:[0x08]",
        "push rdi",
        "push rsi",
        "push rdx",
        "push rcx",
        "push rax",
        "push r8",
        "push r9",
        "push r10",
        "push r11",
        "push rbx",
        "push rbp",
        "push r12",
        "push r13",
        "push r14",
        "push r15",
        "mov rdi, rsp",
        "call zenith_syscall_entry_from_asm",
        "pop r15",
        "pop r14",
        "pop r13",
        "pop r12",
        "pop rbp",
        "pop rbx",
        "pop r11",
        "pop r10",
        "pop r9",
        "pop r8",
        "pop rax",
        "pop rcx",
        "pop rdx",
        "pop rsi",
        "pop rdi",
        "pop rsp",
        "swapgs",
        "sysretq",
        ".global zenith_enter_user_space_asm",
        "zenith_enter_user_space_asm:",
        "push rbp",
        "push rbx",
        "push r12",
        "push r13",
        "push r14",
        "push r15",
        "mov gs:[0x10], rsp",
        "mov rcx, rdi",
        "mov rsp, rsi",
        "mov r11, 0x002",
        "xor rax, rax",
        "xor rbx, rbx",
        "xor rdx, rdx",
        "xor rsi, rsi",
        "xor rdi, rdi",
        "xor rbp, rbp",
        "xor r8, r8",
        "xor r9, r9",
        "xor r10, r10",
        "xor r12, r12",
        "xor r13, r13",
        "xor r14, r14",
        "xor r15, r15",
        "swapgs",
        "sysretq",
        ".global zenith_exit_user_space_asm",
        "zenith_exit_user_space_asm:",
        "mov eax, edi",
        "mov rsp, gs:[0x10]",
        "pop r15",
        "pop r14",
        "pop r13",
        "pop r12",
        "pop rbx",
        "pop rbp",
        "ret",
    );

    extern "C" {
        fn zenith_syscall_trampoline();
        fn zenith_enter_user_space_asm(entry_rip: u64, user_rsp: u64) -> i32;
        fn zenith_exit_user_space_asm(exit_code: i32) -> !;
    }

    pub fn init_syscall_msrs() {
        // SAFETY:
        // Khởi tạo PerCpuData và các thanh ghi MSR cho lệnh `syscall`/`sysretq`.
        unsafe {
            let kstack_top = (&raw const KERNEL_INTR_STACK.0 as u64) + 16384;
            let per_cpu_ptr = &raw mut BSP_PER_CPU;
            (*per_cpu_ptr).kernel_rsp = kstack_top;
            let per_cpu_addr = per_cpu_ptr as u64;

            write_msr(Msr::Ia32GsBase, per_cpu_addr);
            write_msr(Msr::Ia32KernelGsBase, per_cpu_addr);
        }

        let efer = read_msr(Msr::Ia32Efer);
        write_msr(Msr::Ia32Efer, efer | 1);

        let star: u64 = (0x0010u64 << 48) | (0x0008u64 << 32);
        write_msr(Msr::Ia32Star, star);

        write_msr(
            Msr::Ia32Lstar,
            zenith_syscall_trampoline as *const () as u64,
        );
        write_msr(Msr::Ia32SfMask, 0x0600);
    }

    pub fn enter_user_space(entry_rip: u64, user_rsp: u64) -> i32 {
        // SAFETY:
        // `entry_rip` và `user_rsp` đã được trình nạp ELF64 ánh xạ hợp lệ với cờ `PAGE_USER`.
        unsafe { zenith_enter_user_space_asm(entry_rip, user_rsp) }
    }

    pub fn return_to_kernel_from_exit(exit_code: i32) -> ! {
        // SAFETY:
        // Được gọi từ ngữ cảnh Syscall sau khi `swapgs` đã khôi phục `GS` của Kernel (`BSP_PER_CPU`).
        unsafe { zenith_exit_user_space_asm(exit_code) }
    }

    pub fn copy_to_buffer(dst_addr: u64, src: &[u8]) -> Result<(), OstdError> {
        if dst_addr == 0 {
            return Err(OstdError::InvalidUserPointer);
        }
        // SAFETY:
        // `dst_addr` đã được kiểm tra khác null và đã được ánh xạ trong không gian địa chỉ.
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
        // `src_addr` đã được xác thực khác null và trỏ tới bộ đệm đã ánh xạ.
        unsafe {
            core::ptr::copy_nonoverlapping(src_addr as *const u8, dst.as_mut_ptr(), dst.len());
        }
        Ok(())
    }
}

// ============================================================================
// MODULE 2: SAFE KERNEL CORE, VFS, INITRAMFS, EXECVE & LINUX ABI (PHASE 4)
// Toàn bộ tầng này cấm tuyệt đối `unsafe` và cấm `unwrap()` / `expect()`.
// ============================================================================
#[forbid(unsafe_code)]
pub mod linux_abi {
    use crate::ostd::{
        copy_from_buffer, copy_to_buffer, map_user_page, read_msr, read_tsc, write_msr,
        BitmapFrameAllocator, Msr, SerialConsole, TrapFrame, PAGE_SIZE,
    };

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum LinuxErrno {
        Eperm = 1,
        Enoent = 2,
        Esrch = 3,
        Ebadf = 9,
        Echild = 10,
        Eagain = 11,
        Enomem = 12,
        Eacces = 13,
        Efault = 14,
        Enotdir = 20,
        Eisdir = 21,
        Einval = 22,
        Emfile = 24,
        Enosys = 38,
        Erange = 34,
    }

    impl LinuxErrno {
        #[inline(always)]
        pub const fn as_neg_ret(self) -> u64 {
            (-(self as isize)) as u64
        }
    }

    // Bảng số hiệu Syscall chuẩn của kiến trúc Linux x86_64
    pub const SYS_READ: u64 = 0;
    pub const SYS_WRITE: u64 = 1;
    pub const SYS_OPEN: u64 = 2;
    pub const SYS_CLOSE: u64 = 3;
    pub const SYS_FSTAT: u64 = 5;
    pub const SYS_LSEEK: u64 = 8;
    pub const SYS_MMAP: u64 = 9;
    pub const SYS_MPROTECT: u64 = 10;
    pub const SYS_MUNMAP: u64 = 11;
    pub const SYS_BRK: u64 = 12;
    pub const SYS_IOCTL: u64 = 16;
    pub const SYS_WRITEV: u64 = 20;
    pub const SYS_SCHED_YIELD: u64 = 24;
    pub const SYS_DUP: u64 = 32;
    pub const SYS_DUP2: u64 = 33;
    pub const SYS_NANOSLEEP: u64 = 35;
    pub const SYS_GETPID: u64 = 39;
    pub const SYS_CLONE: u64 = 56;
    pub const SYS_EXECVE: u64 = 59;
    pub const SYS_EXIT: u64 = 60;
    pub const SYS_WAIT4: u64 = 61;
    pub const SYS_UNAME: u64 = 63;
    pub const SYS_GETCWD: u64 = 79;
    pub const SYS_CHDIR: u64 = 80;
    pub const SYS_GETPPID: u64 = 110;
    pub const SYS_ARCH_PRCTL: u64 = 158;
    pub const SYS_GETTID: u64 = 186;
    pub const SYS_FUTEX: u64 = 202;
    pub const SYS_GETDENTS64: u64 = 217;
    pub const SYS_SET_TID_ADDRESS: u64 = 218;
    pub const SYS_CLOCK_GETTIME: u64 = 228;
    pub const SYS_EXIT_GROUP: u64 = 231;
    pub const SYS_OPENAT: u64 = 257;
    pub const SYS_NEWFSTATAT: u64 = 262;
    pub const SYS_FACCESSAT: u64 = 269;
    pub const SYS_PIPE2: u64 = 293;

    pub const ARCH_SET_GS: u64 = 0x1001;
    pub const ARCH_SET_FS: u64 = 0x1002;
    pub const ARCH_GET_FS: u64 = 0x1003;
    pub const ARCH_GET_GS: u64 = 0x1004;

    pub const TIOCGWINSZ: u64 = 0x5413;
    pub const FUTEX_WAIT: u64 = 0;
    pub const FUTEX_WAKE: u64 = 1;
    pub const FUTEX_PRIVATE_FLAG: u64 = 128;

    // ------------------------------------------------------------------------
    // 2.1: VIRTUAL FILE SYSTEM (VFS), CAPABILITIES & PIPE IPC
    // ------------------------------------------------------------------------
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum VfsNodeKind {
        RootDir,
        OsRelease,
        ProcVersion,
        DevNull,
        DevZero,
        BinInit,
        BinApp,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum CapObject {
        SerialIn,
        SerialOut,
        SerialErr,
        PipeRead(usize),
        PipeWrite(usize),
        VfsFile { kind: VfsNodeKind, offset: usize },
        VfsDir { offset: usize },
    }

    #[derive(Debug, Clone, Copy)]
    pub struct CapabilityEntry {
        pub object: CapObject,
        pub can_read: bool,
        pub can_write: bool,
    }

    pub const MAX_FDS: usize = 16;
    pub const MAX_PIPES: usize = 4;
    pub const PIPE_BUF_SIZE: usize = 256;

    pub struct PipeRingBuffer {
        pub allocated: bool,
        pub data: [u8; PIPE_BUF_SIZE],
        pub read_pos: usize,
        pub write_pos: usize,
        pub count: usize,
    }

    impl Default for PipeRingBuffer {
        fn default() -> Self {
            Self::new()
        }
    }

    impl PipeRingBuffer {
        pub const fn new() -> Self {
            Self {
                allocated: false,
                data: [0; PIPE_BUF_SIZE],
                read_pos: 0,
                write_pos: 0,
                count: 0,
            }
        }

        pub fn write_slice(&mut self, src: &[u8]) -> usize {
            let mut written = 0;
            for &b in src {
                if self.count >= PIPE_BUF_SIZE {
                    break;
                }
                if let Some(slot) = self.data.get_mut(self.write_pos) {
                    *slot = b;
                    self.write_pos = (self.write_pos + 1) % PIPE_BUF_SIZE;
                    self.count += 1;
                    written += 1;
                }
            }
            written
        }

        pub fn read_slice(&mut self, dst: &mut [u8]) -> usize {
            let mut read_bytes = 0;
            for slot in dst.iter_mut() {
                if self.count == 0 {
                    break;
                }
                if let Some(&b) = self.data.get(self.read_pos) {
                    *slot = b;
                    self.read_pos = (self.read_pos + 1) % PIPE_BUF_SIZE;
                    self.count -= 1;
                    read_bytes += 1;
                }
            }
            read_bytes
        }
    }

    const OS_RELEASE_CONTENT: &[u8] =
        b"NAME=\"ZenithOS\"\nVERSION=\"0.4.0\"\nID=zenith\nPRETTY_NAME=\"ZenithOS v0.4.0\"\n";
    const PROC_VERSION_CONTENT: &[u8] =
        b"Linux version 6.8.0-zenith-framekernel (rust@zenith) #1 SMP PREEMPT_DYNAMIC\n";

    fn resolve_vfs_path(path: &[u8]) -> Result<VfsNodeKind, LinuxErrno> {
        match path {
            b"/" | b"." => Ok(VfsNodeKind::RootDir),
            b"/etc/os-release" | b"etc/os-release" => Ok(VfsNodeKind::OsRelease),
            b"/proc/version" | b"proc/version" => Ok(VfsNodeKind::ProcVersion),
            b"/dev/null" => Ok(VfsNodeKind::DevNull),
            b"/dev/zero" => Ok(VfsNodeKind::DevZero),
            b"/bin/init" => Ok(VfsNodeKind::BinInit),
            b"/bin/app" => Ok(VfsNodeKind::BinApp),
            _ => Err(LinuxErrno::Enoent),
        }
    }

    fn read_cstr_from_user(user_ptr: u64, buf: &mut [u8; 64]) -> Result<&[u8], LinuxErrno> {
        copy_from_buffer(user_ptr, buf).map_err(|_| LinuxErrno::Efault)?;
        let mut len = 0;
        while len < buf.len() {
            if buf.get(len) == Some(&0) {
                break;
            }
            len += 1;
        }
        buf.get(..len).ok_or(LinuxErrno::Einval)
    }

    // ------------------------------------------------------------------------
    // 2.2: QUẢN LÝ TIẾN TRÌNH (TASK TABLE), VMO MMAP & WAITPORT (FUTEX)
    // ------------------------------------------------------------------------
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum TaskState {
        Unused,
        Running,
        Ready,
        BlockedOnFutex(u64),
        Exited(i32),
    }

    #[derive(Clone, Copy)]
    pub struct TaskControlBlock {
        pub pid: u64,
        pub ppid: u64,
        pub state: TaskState,
        pub clear_child_tid: u64,
    }

    impl TaskControlBlock {
        pub const fn unused() -> Self {
            Self {
                pid: 0,
                ppid: 0,
                state: TaskState::Unused,
                clear_child_tid: 0,
            }
        }
    }

    pub const MAX_TASKS: usize = 8;

    pub struct KernelContext {
        pub pmm: BitmapFrameAllocator,
        pub current_pid: u64,
        pub next_pid: u64,
        pub program_break: u64,
        pub next_mmap_va: u64,
        pub last_exit_code: Option<i32>,
        pub cwd: [u8; 64],
        pub cwd_len: usize,
        pub fd_table: [Option<CapabilityEntry>; MAX_FDS],
        pub pipes: [PipeRingBuffer; MAX_PIPES],
        pub tasks: [TaskControlBlock; MAX_TASKS],
    }

    impl KernelContext {
        pub const fn const_new() -> Self {
            let mut fds: [Option<CapabilityEntry>; MAX_FDS] = [None; MAX_FDS];
            fds[0] = Some(CapabilityEntry {
                object: CapObject::SerialIn,
                can_read: true,
                can_write: false,
            });
            fds[1] = Some(CapabilityEntry {
                object: CapObject::SerialOut,
                can_read: false,
                can_write: true,
            });
            fds[2] = Some(CapabilityEntry {
                object: CapObject::SerialErr,
                can_read: false,
                can_write: true,
            });

            let mut tasks = [TaskControlBlock::unused(); MAX_TASKS];
            tasks[0] = TaskControlBlock {
                pid: 1,
                ppid: 0,
                state: TaskState::Running,
                clear_child_tid: 0,
            };

            let mut cwd = [0u8; 64];
            cwd[0] = b'/';

            Self {
                pmm: BitmapFrameAllocator::const_uninit(),
                current_pid: 1,
                next_pid: 2,
                program_break: 0x0000_0000_0040_3000,
                next_mmap_va: 0x0000_0000_1000_0000,
                last_exit_code: None,
                cwd,
                cwd_len: 1,
                fd_table: fds,
                pipes: [
                    PipeRingBuffer::new(),
                    PipeRingBuffer::new(),
                    PipeRingBuffer::new(),
                    PipeRingBuffer::new(),
                ],
                tasks,
            }
        }

        pub fn init_hardware_pmm(&mut self) {
            self.pmm = BitmapFrameAllocator::new();
        }

        pub fn alloc_fd(&mut self, entry: CapabilityEntry) -> Result<usize, LinuxErrno> {
            for (idx, slot) in self.fd_table.iter_mut().enumerate() {
                if slot.is_none() {
                    *slot = Some(entry);
                    return Ok(idx);
                }
            }
            Err(LinuxErrno::Emfile)
        }
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct LinuxUtsName {
        pub sysname: [u8; 65],
        pub nodename: [u8; 65],
        pub release: [u8; 65],
        pub version: [u8; 65],
        pub machine: [u8; 65],
        pub domainname: [u8; 65],
    }

    impl LinuxUtsName {
        pub fn zenith_default() -> Self {
            let mut uts = Self {
                sysname: [0; 65],
                nodename: [0; 65],
                release: [0; 65],
                version: [0; 65],
                machine: [0; 65],
                domainname: [0; 65],
            };
            Self::fill_field(&mut uts.sysname, b"Linux");
            Self::fill_field(&mut uts.nodename, b"zenith-os");
            Self::fill_field(&mut uts.release, b"6.8.0-zenith-framekernel");
            Self::fill_field(&mut uts.version, b"#1 SMP PREEMPT_DYNAMIC ZenithOS Rust");
            Self::fill_field(&mut uts.machine, b"x86_64");
            Self::fill_field(&mut uts.domainname, b"(none)");
            uts
        }

        fn fill_field(dst: &mut [u8; 65], src: &[u8]) {
            for (d, &s) in dst.iter_mut().take(64).zip(src.iter()) {
                *d = s;
            }
        }

        pub fn to_bytes(self) -> [u8; 390] {
            let mut out = [0u8; 390];
            Self::copy_slice(&mut out, 0, &self.sysname);
            Self::copy_slice(&mut out, 65, &self.nodename);
            Self::copy_slice(&mut out, 130, &self.release);
            Self::copy_slice(&mut out, 195, &self.version);
            Self::copy_slice(&mut out, 260, &self.machine);
            Self::copy_slice(&mut out, 325, &self.domainname);
            out
        }

        fn copy_slice(dst: &mut [u8; 390], offset: usize, src: &[u8; 65]) {
            if let Some(sub) = dst.get_mut(offset..offset + 65) {
                for (d, &s) in sub.iter_mut().zip(src.iter()) {
                    *d = s;
                }
            }
        }
    }

    // ------------------------------------------------------------------------
    // 2.3: BỘ ĐIỀU PHỐI SYSCALL LINUX X86_64 (PHASE 4: VFS & EXECVE)
    // ------------------------------------------------------------------------
    pub fn dispatch_linux_syscall(frame: &mut TrapFrame, ctx: &mut KernelContext) {
        let result = match frame.rax {
            SYS_READ => sys_read(ctx, frame.rdi, frame.rsi, frame.rdx),
            SYS_WRITE => sys_write(ctx, frame.rdi, frame.rsi, frame.rdx),
            SYS_OPEN => sys_openat(ctx, -100i64 as u64, frame.rdi, frame.rsi),
            SYS_CLOSE => sys_close(ctx, frame.rdi),
            SYS_FSTAT => sys_fstat(ctx, frame.rdi, frame.rsi),
            SYS_LSEEK => sys_lseek(ctx, frame.rdi, frame.rsi as i64, frame.rdx),
            SYS_MMAP => sys_mmap(ctx, frame.rdi, frame.rsi),
            SYS_MPROTECT | SYS_MUNMAP => Ok(0),
            SYS_BRK => sys_brk(ctx, frame.rdi),
            SYS_IOCTL => sys_ioctl(frame.rdi, frame.rsi, frame.rdx),
            SYS_WRITEV => sys_writev(ctx, frame.rdi, frame.rsi, frame.rdx),
            SYS_SCHED_YIELD | SYS_NANOSLEEP => Ok(0),
            SYS_DUP => sys_dup(ctx, frame.rdi),
            SYS_DUP2 => sys_dup2(ctx, frame.rdi, frame.rsi),
            SYS_GETPID | SYS_GETTID => Ok(ctx.current_pid),
            SYS_GETPPID => Ok(0),
            SYS_CLONE => sys_clone(ctx),
            SYS_EXECVE => sys_execve(frame, ctx),
            SYS_WAIT4 => sys_wait4(ctx, frame.rdi, frame.rsi),
            SYS_UNAME => sys_uname(frame.rdi),
            SYS_GETCWD => sys_getcwd(ctx, frame.rdi, frame.rsi),
            SYS_CHDIR => sys_chdir(ctx, frame.rdi),
            SYS_ARCH_PRCTL => sys_arch_prctl(frame.rdi, frame.rsi),
            SYS_FUTEX => sys_futex(ctx, frame.rdi, frame.rsi, frame.rdx as u32),
            SYS_GETDENTS64 => sys_getdents64(ctx, frame.rdi, frame.rsi, frame.rdx),
            SYS_SET_TID_ADDRESS => {
                if let Some(t) = ctx.tasks.get_mut(0) {
                    t.clear_child_tid = frame.rdi;
                }
                Ok(ctx.current_pid)
            }
            SYS_CLOCK_GETTIME => sys_clock_gettime(frame.rdi, frame.rsi),
            SYS_OPENAT => sys_openat(ctx, frame.rdi, frame.rsi, frame.rdx),
            SYS_NEWFSTATAT => sys_newfstatat(frame.rsi, frame.rdx),
            SYS_FACCESSAT => sys_faccessat(frame.rsi),
            SYS_PIPE2 => sys_pipe2(ctx, frame.rdi),
            SYS_EXIT | SYS_EXIT_GROUP => {
                ctx.last_exit_code = Some(frame.rdi as i32);
                Ok(0)
            }
            _ => Err(LinuxErrno::Enosys),
        };

        frame.rax = match result {
            Ok(val) => val,
            Err(errno) => errno.as_neg_ret(),
        };
    }

    fn sys_openat(
        ctx: &mut KernelContext,
        _dirfd: u64,
        path_ptr: u64,
        _flags: u64,
    ) -> Result<u64, LinuxErrno> {
        let mut path_buf = [0u8; 64];
        let path_slice = read_cstr_from_user(path_ptr, &mut path_buf)?;
        let kind = resolve_vfs_path(path_slice)?;

        let entry = match kind {
            VfsNodeKind::RootDir => CapabilityEntry {
                object: CapObject::VfsDir { offset: 0 },
                can_read: true,
                can_write: false,
            },
            VfsNodeKind::DevNull | VfsNodeKind::DevZero => CapabilityEntry {
                object: CapObject::VfsFile { kind, offset: 0 },
                can_read: true,
                can_write: true,
            },
            _ => CapabilityEntry {
                object: CapObject::VfsFile { kind, offset: 0 },
                can_read: true,
                can_write: false,
            },
        };

        let fd = ctx.alloc_fd(entry)?;
        Ok(fd as u64)
    }

    fn sys_read(
        ctx: &mut KernelContext,
        fd: u64,
        buf_ptr: u64,
        count: u64,
    ) -> Result<u64, LinuxErrno> {
        let cap = ctx
            .fd_table
            .get(fd as usize)
            .and_then(|c| *c)
            .ok_or(LinuxErrno::Ebadf)?;
        if !cap.can_read {
            return Err(LinuxErrno::Ebadf);
        }
        match cap.object {
            CapObject::SerialIn => Ok(0),
            CapObject::PipeRead(pipe_idx) => {
                let pipe = ctx.pipes.get_mut(pipe_idx).ok_or(LinuxErrno::Ebadf)?;
                let max_len = (count as usize).min(128);
                let mut tmp = [0u8; 128];
                let slice = tmp.get_mut(..max_len).ok_or(LinuxErrno::Efault)?;
                let n = pipe.read_slice(slice);
                if let Some(valid_out) = slice.get(..n) {
                    copy_to_buffer(buf_ptr, valid_out).map_err(|_| LinuxErrno::Efault)?;
                }
                Ok(n as u64)
            }
            CapObject::VfsFile { kind, offset } => {
                let content: &[u8] = match kind {
                    VfsNodeKind::OsRelease => OS_RELEASE_CONTENT,
                    VfsNodeKind::ProcVersion => PROC_VERSION_CONTENT,
                    VfsNodeKind::DevNull => &[],
                    VfsNodeKind::DevZero => &[0u8; 128],
                    _ => return Err(LinuxErrno::Einval),
                };
                if offset >= content.len() {
                    return Ok(0);
                }
                let avail = content.len() - offset;
                let to_read = (count as usize).min(avail);
                let slice = content
                    .get(offset..offset + to_read)
                    .ok_or(LinuxErrno::Efault)?;
                copy_to_buffer(buf_ptr, slice).map_err(|_| LinuxErrno::Efault)?;

                if let Some(Some(slot)) = ctx.fd_table.get_mut(fd as usize) {
                    slot.object = CapObject::VfsFile {
                        kind,
                        offset: offset + to_read,
                    };
                }
                Ok(to_read as u64)
            }
            CapObject::VfsDir { .. } => Err(LinuxErrno::Eisdir),
            _ => Err(LinuxErrno::Ebadf),
        }
    }

    fn sys_lseek(
        ctx: &mut KernelContext,
        fd: u64,
        offset_arg: i64,
        whence: u64,
    ) -> Result<u64, LinuxErrno> {
        let slot = ctx
            .fd_table
            .get_mut(fd as usize)
            .and_then(|c| c.as_mut())
            .ok_or(LinuxErrno::Ebadf)?;
        if let CapObject::VfsFile { kind, offset } = slot.object {
            let new_off = match whence {
                0 => offset_arg as usize,
                1 => (offset as i64 + offset_arg) as usize,
                _ => return Err(LinuxErrno::Einval),
            };
            slot.object = CapObject::VfsFile {
                kind,
                offset: new_off,
            };
            Ok(new_off as u64)
        } else {
            Err(LinuxErrno::Einval)
        }
    }

    fn sys_getdents64(
        ctx: &mut KernelContext,
        fd: u64,
        dirp_ptr: u64,
        count: u64,
    ) -> Result<u64, LinuxErrno> {
        let slot = ctx
            .fd_table
            .get_mut(fd as usize)
            .and_then(|c| c.as_mut())
            .ok_or(LinuxErrno::Ebadf)?;
        if let CapObject::VfsDir { offset } = slot.object {
            if offset > 0 {
                return Ok(0);
            }
            // Cấu trúc linux_dirent64 tối giản cho mục "bin" và "etc" (mỗi mục 24 bytes căn lề 8-byte)
            if count < 48 {
                return Err(LinuxErrno::Einval);
            }
            let mut dirents = [0u8; 48];
            // Mục 1: d_ino=1, d_off=24, d_reclen=24, d_type=4 (DT_DIR), d_name="bin\0"
            dirents[0..8].copy_from_slice(&1u64.to_le_bytes());
            dirents[8..16].copy_from_slice(&24u64.to_le_bytes());
            dirents[16..18].copy_from_slice(&24u16.to_le_bytes());
            dirents[18] = 4;
            dirents[19..23].copy_from_slice(b"bin\0");

            // Mục 2: d_ino=2, d_off=48, d_reclen=24, d_type=4 (DT_DIR), d_name="etc\0"
            dirents[24..32].copy_from_slice(&2u64.to_le_bytes());
            dirents[32..40].copy_from_slice(&48u64.to_le_bytes());
            dirents[40..42].copy_from_slice(&24u16.to_le_bytes());
            dirents[42] = 4;
            dirents[43..47].copy_from_slice(b"etc\0");

            copy_to_buffer(dirp_ptr, &dirents).map_err(|_| LinuxErrno::Efault)?;
            slot.object = CapObject::VfsDir { offset: 2 };
            Ok(48)
        } else {
            Err(LinuxErrno::Enotdir)
        }
    }

    fn sys_getcwd(ctx: &KernelContext, buf_ptr: u64, size: u64) -> Result<u64, LinuxErrno> {
        let needed = ctx.cwd_len + 1;
        if (size as usize) < needed {
            return Err(LinuxErrno::Erange);
        }
        let slice = ctx.cwd.get(..needed).ok_or(LinuxErrno::Efault)?;
        copy_to_buffer(buf_ptr, slice).map_err(|_| LinuxErrno::Efault)?;
        Ok(buf_ptr)
    }

    fn sys_chdir(ctx: &mut KernelContext, path_ptr: u64) -> Result<u64, LinuxErrno> {
        let mut path_buf = [0u8; 64];
        let path_slice = read_cstr_from_user(path_ptr, &mut path_buf)?;
        let kind = resolve_vfs_path(path_slice)?;
        if kind != VfsNodeKind::RootDir {
            return Err(LinuxErrno::Enotdir);
        }
        ctx.cwd = [0u8; 64];
        ctx.cwd[0] = b'/';
        ctx.cwd_len = 1;
        Ok(0)
    }

    fn sys_newfstatat(path_ptr: u64, statbuf_ptr: u64) -> Result<u64, LinuxErrno> {
        let mut path_buf = [0u8; 64];
        let path_slice = read_cstr_from_user(path_ptr, &mut path_buf)?;
        let _kind = resolve_vfs_path(path_slice)?;
        let zero_stat = [0u8; 144];
        copy_to_buffer(statbuf_ptr, &zero_stat).map_err(|_| LinuxErrno::Efault)?;
        Ok(0)
    }

    fn sys_faccessat(path_ptr: u64) -> Result<u64, LinuxErrno> {
        let mut path_buf = [0u8; 64];
        let path_slice = read_cstr_from_user(path_ptr, &mut path_buf)?;
        let _kind = resolve_vfs_path(path_slice)?;
        Ok(0)
    }

    /// Hiện thực `SYS_execve (59)`: Phân giải đường dẫn tệp thực thi trong VFS,
    /// nạp ảnh ELF64 mới vào không gian địa chỉ User-space, và thay thế `RIP` (`frame.rcx`)
    /// cùng `RSP` (`frame.rsp`) để `sysretq` nhảy thẳng vào chương trình mới ở Ring 3!
    fn sys_execve(frame: &mut TrapFrame, ctx: &mut KernelContext) -> Result<u64, LinuxErrno> {
        let mut path_buf = [0u8; 64];
        let path_slice = read_cstr_from_user(frame.rdi, &mut path_buf)?;
        let kind = resolve_vfs_path(path_slice)?;

        let elf_image = match kind {
            VfsNodeKind::BinApp => build_embedded_linux_app_elf64(),
            VfsNodeKind::BinInit => build_embedded_linux_init_elf64(),
            _ => return Err(LinuxErrno::Eacces),
        };

        let loaded = load_elf64_into_user_space(&elf_image, &mut ctx.pmm)
            .map_err(|_| LinuxErrno::Enomem)?;

        ctx.program_break = loaded.initial_brk;

        // Đặt lại toàn bộ thanh ghi của luồng User và trỏ RCX (User RIP) + RSP tới ảnh ELF64 mới!
        frame.rcx = loaded.entry_rip;
        frame.rsp = loaded.user_rsp;
        frame.rdi = 0;
        frame.rsi = 0;
        frame.rdx = 0;
        frame.r8 = 0;
        frame.r9 = 0;
        frame.r10 = 0;

        Ok(0)
    }

    fn sys_write(
        ctx: &mut KernelContext,
        fd: u64,
        buf_ptr: u64,
        count: u64,
    ) -> Result<u64, LinuxErrno> {
        let cap = ctx
            .fd_table
            .get(fd as usize)
            .and_then(|c| *c)
            .ok_or(LinuxErrno::Ebadf)?;
        if !cap.can_write {
            return Err(LinuxErrno::Ebadf);
        }
        if count == 0 {
            return Ok(0);
        }

        match cap.object {
            CapObject::SerialOut | CapObject::SerialErr => {
                let mut temp_buf = [0u8; 128];
                let mut written: u64 = 0;
                while written < count {
                    let remaining = (count - written) as usize;
                    let chunk_len = remaining.min(temp_buf.len());
                    if let Some(slice) = temp_buf.get_mut(..chunk_len) {
                        copy_from_buffer(buf_ptr.wrapping_add(written), slice)
                            .map_err(|_| LinuxErrno::Efault)?;
                        SerialConsole::write_bytes(slice);
                        written += chunk_len as u64;
                    } else {
                        return Err(LinuxErrno::Efault);
                    }
                }
                Ok(written)
            }
            CapObject::PipeWrite(pipe_idx) => {
                let max_len = (count as usize).min(128);
                let mut tmp = [0u8; 128];
                let slice = tmp.get_mut(..max_len).ok_or(LinuxErrno::Efault)?;
                copy_from_buffer(buf_ptr, slice).map_err(|_| LinuxErrno::Efault)?;
                let pipe = ctx.pipes.get_mut(pipe_idx).ok_or(LinuxErrno::Ebadf)?;
                let n = pipe.write_slice(slice);
                Ok(n as u64)
            }
            CapObject::VfsFile {
                kind: VfsNodeKind::DevNull | VfsNodeKind::DevZero,
                ..
            } => Ok(count),
            _ => Err(LinuxErrno::Ebadf),
        }
    }

    fn sys_close(ctx: &mut KernelContext, fd: u64) -> Result<u64, LinuxErrno> {
        let slot = ctx.fd_table.get_mut(fd as usize).ok_or(LinuxErrno::Ebadf)?;
        if slot.is_none() {
            return Err(LinuxErrno::Ebadf);
        }
        *slot = None;
        Ok(0)
    }

    fn sys_dup(ctx: &mut KernelContext, oldfd: u64) -> Result<u64, LinuxErrno> {
        let cap = ctx
            .fd_table
            .get(oldfd as usize)
            .and_then(|c| *c)
            .ok_or(LinuxErrno::Ebadf)?;
        let new_fd = ctx.alloc_fd(cap)?;
        Ok(new_fd as u64)
    }

    fn sys_dup2(ctx: &mut KernelContext, oldfd: u64, newfd: u64) -> Result<u64, LinuxErrno> {
        let cap = ctx
            .fd_table
            .get(oldfd as usize)
            .and_then(|c| *c)
            .ok_or(LinuxErrno::Ebadf)?;
        if oldfd == newfd {
            return Ok(newfd);
        }
        let target_slot = ctx
            .fd_table
            .get_mut(newfd as usize)
            .ok_or(LinuxErrno::Ebadf)?;
        *target_slot = Some(cap);
        Ok(newfd)
    }

    fn sys_pipe2(ctx: &mut KernelContext, pipefd_user_ptr: u64) -> Result<u64, LinuxErrno> {
        let mut pipe_idx = None;
        for (i, p) in ctx.pipes.iter_mut().enumerate() {
            if !p.allocated {
                p.allocated = true;
                p.read_pos = 0;
                p.write_pos = 0;
                p.count = 0;
                pipe_idx = Some(i);
                break;
            }
        }
        let idx = pipe_idx.ok_or(LinuxErrno::Emfile)?;

        let read_fd = ctx.alloc_fd(CapabilityEntry {
            object: CapObject::PipeRead(idx),
            can_read: true,
            can_write: false,
        })?;

        let write_fd = match ctx.alloc_fd(CapabilityEntry {
            object: CapObject::PipeWrite(idx),
            can_read: false,
            can_write: true,
        }) {
            Ok(fd) => fd,
            Err(e) => {
                if let Some(slot) = ctx.fd_table.get_mut(read_fd) {
                    *slot = None;
                }
                return Err(e);
            }
        };

        let mut fds_bytes = [0u8; 8];
        fds_bytes[0..4].copy_from_slice(&(read_fd as i32).to_le_bytes());
        fds_bytes[4..8].copy_from_slice(&(write_fd as i32).to_le_bytes());
        copy_to_buffer(pipefd_user_ptr, &fds_bytes).map_err(|_| LinuxErrno::Efault)?;
        Ok(0)
    }

    fn sys_fstat(ctx: &KernelContext, fd: u64, statbuf_ptr: u64) -> Result<u64, LinuxErrno> {
        let _cap = ctx
            .fd_table
            .get(fd as usize)
            .and_then(|c| *c)
            .ok_or(LinuxErrno::Ebadf)?;
        let zero_stat = [0u8; 144];
        copy_to_buffer(statbuf_ptr, &zero_stat).map_err(|_| LinuxErrno::Efault)?;
        Ok(0)
    }

    fn sys_writev(
        ctx: &mut KernelContext,
        fd: u64,
        iov_ptr: u64,
        iovcnt: u64,
    ) -> Result<u64, LinuxErrno> {
        if iovcnt > 1024 {
            return Err(LinuxErrno::Einval);
        }
        let mut total_written: u64 = 0;
        for idx in 0..iovcnt {
            let entry_addr = iov_ptr.wrapping_add(idx.wrapping_mul(16));
            let mut raw_iov = [0u8; 16];
            copy_from_buffer(entry_addr, &mut raw_iov).map_err(|_| LinuxErrno::Efault)?;

            let mut base_bytes = [0u8; 8];
            let mut len_bytes = [0u8; 8];
            base_bytes.copy_from_slice(&raw_iov[0..8]);
            len_bytes.copy_from_slice(&raw_iov[8..16]);

            let iov_base = u64::from_le_bytes(base_bytes);
            let iov_len = u64::from_le_bytes(len_bytes);

            if iov_len > 0 {
                let n = sys_write(ctx, fd, iov_base, iov_len)?;
                total_written = total_written.wrapping_add(n);
            }
        }
        Ok(total_written)
    }

    fn sys_ioctl(fd: u64, request: u64, argp: u64) -> Result<u64, LinuxErrno> {
        if fd <= 2 && request == TIOCGWINSZ {
            let winsize: [u8; 8] = [25, 0, 80, 0, 0, 0, 0, 0];
            copy_to_buffer(argp, &winsize).map_err(|_| LinuxErrno::Efault)?;
            Ok(0)
        } else {
            Err(LinuxErrno::Einval)
        }
    }

    fn sys_mmap(ctx: &mut KernelContext, addr_hint: u64, length: u64) -> Result<u64, LinuxErrno> {
        if length == 0 {
            return Err(LinuxErrno::Einval);
        }
        let page_sz = PAGE_SIZE as u64;
        let num_pages = length.wrapping_add(0xFFF) / page_sz;
        let base_va = if addr_hint != 0 && (addr_hint & 0xFFF) == 0 {
            addr_hint
        } else {
            ctx.next_mmap_va
        };

        for i in 0..num_pages {
            let page_va = base_va + i * page_sz;
            map_user_page(&mut ctx.pmm, page_va).map_err(|_| LinuxErrno::Enomem)?;
        }

        if base_va >= ctx.next_mmap_va {
            ctx.next_mmap_va = base_va + num_pages * page_sz;
        }
        Ok(base_va)
    }

    fn sys_brk(ctx: &mut KernelContext, new_brk: u64) -> Result<u64, LinuxErrno> {
        if new_brk != 0 && new_brk >= ctx.program_break {
            ctx.program_break = new_brk;
        }
        Ok(ctx.program_break)
    }

    fn sys_uname(uts_ptr: u64) -> Result<u64, LinuxErrno> {
        let uts = LinuxUtsName::zenith_default();
        let raw_bytes = uts.to_bytes();
        copy_to_buffer(uts_ptr, &raw_bytes).map_err(|_| LinuxErrno::Efault)?;
        Ok(0)
    }

    fn sys_arch_prctl(code: u64, addr: u64) -> Result<u64, LinuxErrno> {
        match code {
            ARCH_SET_FS => {
                write_msr(Msr::Ia32FsBase, addr);
                Ok(0)
            }
            ARCH_GET_FS => {
                let fs_val = read_msr(Msr::Ia32FsBase);
                copy_to_buffer(addr, &fs_val.to_ne_bytes()).map_err(|_| LinuxErrno::Efault)?;
                Ok(0)
            }
            _ => Err(LinuxErrno::Einval),
        }
    }

    fn sys_clock_gettime(_clock_id: u64, tp_user_ptr: u64) -> Result<u64, LinuxErrno> {
        let cycles = read_tsc();
        let total_ns = cycles / 2;
        let tv_sec = (total_ns / 1_000_000_000) as i64;
        let tv_nsec = (total_ns % 1_000_000_000) as i64;

        let mut ts_bytes = [0u8; 16];
        ts_bytes[0..8].copy_from_slice(&tv_sec.to_le_bytes());
        ts_bytes[8..16].copy_from_slice(&tv_nsec.to_le_bytes());
        copy_to_buffer(tp_user_ptr, &ts_bytes).map_err(|_| LinuxErrno::Efault)?;
        Ok(0)
    }

    fn sys_futex(
        ctx: &mut KernelContext,
        uaddr: u64,
        op: u64,
        val: u32,
    ) -> Result<u64, LinuxErrno> {
        let cmd = op & !FUTEX_PRIVATE_FLAG;
        match cmd {
            FUTEX_WAIT => {
                let mut cur_bytes = [0u8; 4];
                copy_from_buffer(uaddr, &mut cur_bytes).map_err(|_| LinuxErrno::Efault)?;
                let cur_val = u32::from_le_bytes(cur_bytes);
                if cur_val != val {
                    return Err(LinuxErrno::Eagain);
                }
                Ok(0)
            }
            FUTEX_WAKE => {
                let mut woken = 0u64;
                for task in ctx.tasks.iter_mut() {
                    if task.state == TaskState::BlockedOnFutex(uaddr) && woken < (val as u64) {
                        task.state = TaskState::Ready;
                        woken += 1;
                    }
                }
                Ok(woken)
            }
            _ => Err(LinuxErrno::Einval),
        }
    }

    fn sys_clone(ctx: &mut KernelContext) -> Result<u64, LinuxErrno> {
        let child_pid = ctx.next_pid;
        for task in ctx.tasks.iter_mut() {
            if task.state == TaskState::Unused {
                task.pid = child_pid;
                task.ppid = ctx.current_pid;
                task.state = TaskState::Exited(0);
                ctx.next_pid += 1;
                return Ok(child_pid);
            }
        }
        Err(LinuxErrno::Eagain)
    }

    fn sys_wait4(
        ctx: &mut KernelContext,
        target_pid: u64,
        wstatus_ptr: u64,
    ) -> Result<u64, LinuxErrno> {
        for task in ctx.tasks.iter_mut() {
            if let TaskState::Exited(code) = task.state {
                if target_pid == u64::MAX || task.pid == target_pid {
                    let reaped_pid = task.pid;
                    task.state = TaskState::Unused;
                    if wstatus_ptr != 0 {
                        let status_word: i32 = (code & 0xFF) << 8;
                        copy_to_buffer(wstatus_ptr, &status_word.to_le_bytes())
                            .map_err(|_| LinuxErrno::Efault)?;
                    }
                    return Ok(reaped_pid);
                }
            }
        }
        Err(LinuxErrno::Echild)
    }

    // ------------------------------------------------------------------------
    // 2.4: TRÌNH PHÂN TÍCH & NẠP LINUX ELF64 (100% SAFE RUST)
    // ------------------------------------------------------------------------
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

        // Tách biệt địa chỉ Stack cho từng ảnh thực thi (0x0080_0000 cho /bin/init, 0x0090_0000 cho /bin/app)
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
        copy_to_buffer(arg1_va, b"--ci-verify\0").map_err(|_| ElfLoadError::MappingFailed)?;
        copy_to_buffer(env0_va, b"OS=ZenithOS\0").map_err(|_| ElfLoadError::MappingFailed)?;

        let table_words: [u64; 16] = [
            2,         // argc
            arg0_va,   // argv[0] = "/bin/init"
            arg1_va,   // argv[1] = "--ci-verify"
            0,         // argv[2] = NULL
            env0_va,   // envp[0] = "OS=ZenithOS"
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

    /// Ảnh thực thi số 1 (`/bin/init` tại `0x0040_1000`):
    /// Kiểm thử VFS (`SYS_getcwd`, `SYS_openat("/etc/os-release")`, `SYS_read`, `SYS_getdents64("/")`),
    /// sau đó gọi `SYS_execve("/bin/app", NULL, NULL)` để chuyển quyền thực thi sang `/bin/app`!
    pub fn build_embedded_linux_init_elf64() -> [u8; 1024] {
        let mut elf = [0u8; 1024];

        // 1. ELF64 Header
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

        // 2. Program Header #0: PT_LOAD (.text tại 0x0040_1000)
        let ph0 = 64;
        elf[ph0..ph0 + 4].copy_from_slice(&PT_LOAD.to_le_bytes());
        elf[ph0 + 4..ph0 + 8].copy_from_slice(&5u32.to_le_bytes());
        elf[ph0 + 8..ph0 + 16].copy_from_slice(&256u64.to_le_bytes());
        elf[ph0 + 16..ph0 + 24].copy_from_slice(&0x0040_1000u64.to_le_bytes());
        elf[ph0 + 24..ph0 + 32].copy_from_slice(&0x0040_1000u64.to_le_bytes());
        elf[ph0 + 32..ph0 + 40].copy_from_slice(&256u64.to_le_bytes());
        elf[ph0 + 40..ph0 + 48].copy_from_slice(&256u64.to_le_bytes());
        elf[ph0 + 48..ph0 + 56].copy_from_slice(&0x1000u64.to_le_bytes());

        // 3. Program Header #1: PT_LOAD (.data tại 0x0040_2000)
        let ph1 = 120;
        elf[ph1..ph1 + 4].copy_from_slice(&PT_LOAD.to_le_bytes());
        elf[ph1 + 4..ph1 + 8].copy_from_slice(&6u32.to_le_bytes());
        elf[ph1 + 8..ph1 + 16].copy_from_slice(&512u64.to_le_bytes());
        elf[ph1 + 16..ph1 + 24].copy_from_slice(&0x0040_2000u64.to_le_bytes());
        elf[ph1 + 24..ph1 + 32].copy_from_slice(&0x0040_2000u64.to_le_bytes());
        elf[ph1 + 32..ph1 + 40].copy_from_slice(&384u64.to_le_bytes());
        elf[ph1 + 40..ph1 + 48].copy_from_slice(&384u64.to_le_bytes());
        elf[ph1 + 48..ph1 + 56].copy_from_slice(&0x1000u64.to_le_bytes());

        // 4. Mã máy Ring 3 của `/bin/init`:
        let code: &[u8] = &[
            // (A) SYS_mmap(NULL, 4096, PROT_RW, MAP_PRIVATE|MAP_ANON, -1, 0) -> r12 = 0x1000_0000
            0xb8, 0x09, 0x00, 0x00, 0x00, // mov eax, 9 (SYS_mmap)
            0x31, 0xff, // xor edi, edi
            0xbe, 0x00, 0x10, 0x00, 0x00, // mov esi, 4096
            0xba, 0x03, 0x00, 0x00, 0x00, // mov edx, 3
            0x41, 0xba, 0x22, 0x00, 0x00, 0x00, // mov r10d, 0x22
            0x49, 0xc7, 0xc0, 0xff, 0xff, 0xff, 0xff, // mov r8, -1
            0x45, 0x31, 0xc9, // xor r9d, r9d
            0x0f, 0x05, // syscall
            0x49, 0x89, 0xc4, // mov r12, rax
            // (B) SYS_getcwd(r12, 64) -> lưu "/\0" vào 0x1000_0000
            0x4c, 0x89, 0xe7, // mov rdi, r12
            0xbe, 0x40, 0x00, 0x00, 0x00, // mov esi, 64
            0xb8, 0x4f, 0x00, 0x00, 0x00, // mov eax, 79 (SYS_getcwd)
            0x0f, 0x05, // syscall
            // (C) SYS_openat(AT_FDCWD=-100, "/etc/os-release", O_RDONLY=0, 0) -> r13 = fd
            0x48, 0xc7, 0xc7, 0x9c, 0xff, 0xff, 0xff, // mov rdi, -100
            0xbe, 0x50, 0x20, 0x40, 0x00, // mov esi, 0x00402050 ("/etc/os-release")
            0x31, 0xd2, // xor edx, edx
            0x45, 0x31, 0xd2, // xor r10d, r10d
            0xb8, 0x01, 0x01, 0x00, 0x00, // mov eax, 257 (SYS_openat)
            0x0f, 0x05, // syscall
            0x49, 0x89, 0xc5, // mov r13, rax
            // (D) SYS_read(r13, r12 + 0x500, 15) -> đọc "NAME=\"ZenithOS\"" vào 0x1000_0500
            0x4c, 0x89, 0xef, // mov rdi, r13
            0x49, 0x8d, 0xb4, 0x24, 0x00, 0x05, 0x00, 0x00, // lea rsi, [r12 + 0x500]
            0xba, 0x0f, 0x00, 0x00, 0x00, // mov edx, 15
            0x31, 0xc0, // xor eax, eax (SYS_read = 0)
            0x0f, 0x05, // syscall
            // (E) SYS_close(r13)
            0x4c, 0x89, 0xef, // mov rdi, r13
            0xb8, 0x03, 0x00, 0x00, 0x00, // mov eax, 3 (SYS_close)
            0x0f, 0x05, // syscall
            // (F) SYS_openat(AT_FDCWD=-100, "/", 0, 0) + SYS_getdents64(fd, r12 + 0x700, 128) + SYS_close
            0x48, 0xc7, 0xc7, 0x9c, 0xff, 0xff, 0xff, // mov rdi, -100
            0xbe, 0x70, 0x20, 0x40, 0x00, // mov esi, 0x00402070 ("/")
            0x31, 0xd2, // xor edx, edx
            0x45, 0x31, 0xd2, // xor r10d, r10d
            0xb8, 0x01, 0x01, 0x00, 0x00, // mov eax, 257 (SYS_openat)
            0x0f, 0x05, // syscall
            0x49, 0x89, 0xc5, // mov r13, rax
            0x4c, 0x89, 0xef, // mov rdi, r13
            0x49, 0x8d, 0xb4, 0x24, 0x00, 0x07, 0x00, 0x00, // lea rsi, [r12 + 0x700]
            0xba, 0x80, 0x00, 0x00, 0x00, // mov edx, 128
            0xb8, 0xd9, 0x00, 0x00, 0x00, // mov eax, 217 (SYS_getdents64)
            0x0f, 0x05, // syscall
            0x4c, 0x89, 0xef, // mov rdi, r13
            0xb8, 0x03, 0x00, 0x00, 0x00, // mov eax, 3 (SYS_close)
            0x0f, 0x05, // syscall
            // (G) SYS_write(1, 0x00402000, 70)
            0xb8, 0x01, 0x00, 0x00, 0x00, // mov eax, 1 (SYS_write)
            0xbf, 0x01, 0x00, 0x00, 0x00, // mov edi, 1
            0xbe, 0x00, 0x20, 0x40, 0x00, // mov esi, 0x00402000
            0xba, 0x46, 0x00, 0x00, 0x00, // mov edx, 70
            0x0f, 0x05, // syscall
            // (H) SYS_execve("/bin/app", NULL, NULL) -> nhảy sang ảnh ELF64 thứ hai!
            0xbf, 0x80, 0x20, 0x40, 0x00, // mov edi, 0x00402080 ("/bin/app")
            0x31, 0xf6, // xor esi, esi
            0x31, 0xd2, // xor edx, edx
            0xb8, 0x3b, 0x00, 0x00, 0x00, // mov eax, 59 (SYS_execve)
            0x0f, 0x05, // syscall
        ];
        elf[256..256 + code.len()].copy_from_slice(code);

        // 5. Dữ liệu .data của `/bin/init` tại offset 512 (ánh xạ vào 0x0040_2000):
        let msg = b"[RING-3 /bin/init] VFS openat('/etc/os-release') & getdents64 tested!\n";
        elf[512..512 + 70].copy_from_slice(msg);
        elf[512 + 0x50..512 + 0x50 + 16].copy_from_slice(b"/etc/os-release\0");
        elf[512 + 0x70..512 + 0x70 + 2].copy_from_slice(b"/\0");
        elf[512 + 0x80..512 + 0x80 + 9].copy_from_slice(b"/bin/app\0");

        elf
    }

    /// Ảnh thực thi số 2 (`/bin/app` tại `0x0050_1000`):
    /// Được nạp từ VFS khi `/bin/init` gọi `SYS_execve("/bin/app")`.
    pub fn build_embedded_linux_app_elf64() -> [u8; 1024] {
        let mut elf = [0u8; 1024];

        // 1. ELF64 Header (e_entry = 0x0050_1000)
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

        // 2. Program Header #0: PT_LOAD (.text tại 0x0050_1000)
        let ph0 = 64;
        elf[ph0..ph0 + 4].copy_from_slice(&PT_LOAD.to_le_bytes());
        elf[ph0 + 4..ph0 + 8].copy_from_slice(&5u32.to_le_bytes());
        elf[ph0 + 8..ph0 + 16].copy_from_slice(&256u64.to_le_bytes());
        elf[ph0 + 16..ph0 + 24].copy_from_slice(&0x0050_1000u64.to_le_bytes());
        elf[ph0 + 24..ph0 + 32].copy_from_slice(&0x0050_1000u64.to_le_bytes());
        elf[ph0 + 32..ph0 + 40].copy_from_slice(&256u64.to_le_bytes());
        elf[ph0 + 40..ph0 + 48].copy_from_slice(&256u64.to_le_bytes());
        elf[ph0 + 48..ph0 + 56].copy_from_slice(&0x1000u64.to_le_bytes());

        // 3. Program Header #1: PT_LOAD (.data tại 0x0050_2000)
        let ph1 = 120;
        elf[ph1..ph1 + 4].copy_from_slice(&PT_LOAD.to_le_bytes());
        elf[ph1 + 4..ph1 + 8].copy_from_slice(&6u32.to_le_bytes());
        elf[ph1 + 8..ph1 + 16].copy_from_slice(&512u64.to_le_bytes());
        elf[ph1 + 16..ph1 + 24].copy_from_slice(&0x0050_2000u64.to_le_bytes());
        elf[ph1 + 24..ph1 + 32].copy_from_slice(&0x0050_2000u64.to_le_bytes());
        elf[ph1 + 32..ph1 + 40].copy_from_slice(&256u64.to_le_bytes());
        elf[ph1 + 40..ph1 + 48].copy_from_slice(&256u64.to_le_bytes());
        elf[ph1 + 48..ph1 + 56].copy_from_slice(&0x1000u64.to_le_bytes());

        // 4. Mã máy Ring 3 của `/bin/app`:
        let code: &[u8] = &[
            // Ghi chữ ký xác thực "ZEN4" vào 0x1000_0600
            0xbf, 0x00, 0x06, 0x00, 0x10, // mov edi, 0x10000600
            0xc7, 0x07, 0x5a, 0x45, 0x4e, 0x34, // mov dword ptr [rdi], "ZEN4"
            // In thông điệp từ `/bin/app` qua SYS_write(1, 0x00502000, 72)
            0xb8, 0x01, 0x00, 0x00, 0x00, // mov eax, 1 (SYS_write)
            0xbf, 0x01, 0x00, 0x00, 0x00, // mov edi, 1
            0xbe, 0x00, 0x20, 0x50, 0x00, // mov esi, 0x00502000
            0xba, 0x48, 0x00, 0x00, 0x00, // mov edx, 72
            0x0f, 0x05, // syscall
            // Thoát với mã xác thực 0x44
            0xbf, 0x44, 0x00, 0x00, 0x00, // mov edi, 0x44
            0xb8, 0xe7, 0x00, 0x00, 0x00, // mov eax, 231 (SYS_exit_group)
            0x0f, 0x05, // syscall
        ];
        elf[256..256 + code.len()].copy_from_slice(code);

        // 5. Dữ liệu .data của `/bin/app` tại offset 512 (ánh xạ vào 0x0050_2000):
        let app_msg = b"[RING-3 /bin/app] Executed new ELF64 image from VFS via SYS_execve(59)!\n";
        elf[512..512 + 72].copy_from_slice(app_msg);

        elf
    }
}

// ============================================================================
// TRẠNG THÁI NHÂN BỀN VỮNG & ĐIỂM NỐI SYSCALL TỪ RING 3
// ============================================================================
struct SyncContextCell(UnsafeCell<linux_abi::KernelContext>);

// SAFETY:
// Truy cập tuần tự trên BSP trong quá trình thực thi Syscall.
unsafe impl Sync for SyncContextCell {}

impl SyncContextCell {
    const fn new() -> Self {
        Self(UnsafeCell::new(linux_abi::KernelContext::const_new()))
    }
}

static GLOBAL_KERNEL_CTX: SyncContextCell = SyncContextCell::new();

#[no_mangle]
extern "C" fn zenith_syscall_entry_from_asm(frame: &mut ostd::TrapFrame) {
    // SAFETY:
    // Thực thi đơn luồng trên BSP khi xử lý lệnh `syscall` từ Ring 3.
    let ctx = unsafe { &mut *GLOBAL_KERNEL_CTX.0.get() };
    linux_abi::dispatch_linux_syscall(frame, ctx);

    if let Some(exit_code) = ctx.last_exit_code {
        ostd::return_to_kernel_from_exit(exit_code);
    }
}

macro_rules! kprintln {
    ($($arg:tt)*) => {
        $crate::ostd::print_fmt(core::format_args!($($arg)*));
        $crate::ostd::print_fmt(core::format_args!("\n"));
    };
}

/// Điểm vào chính của Kernel ZenithOS (Phase 4: VFS, Initramfs, Directory I/O & SYS_execve).
#[no_mangle]
pub extern "C" fn _start() -> ! {
    ostd::SerialConsole::init();
    kprintln!("================================================================");
    kprintln!("  ZenithOS v0.4.0 - 100% Rust Framekernel (Linux ABI Phase 4)   ");
    kprintln!("================================================================");

    // 1. Khởi tạo GDT, TSS, IDT và MSR cho SYSCALL/SYSRETQ
    ostd::init_gdt_and_tss();
    ostd::init_idt();
    ostd::trigger_breakpoint_self_test();
    if ostd::BREAKPOINT_HIT_COUNT.load(Ordering::SeqCst) != 1 {
        kprintln!("[ZENITH-TEST][FAIL] IDT Breakpoint self-test failed!");
        ostd::exit_qemu(ostd::QemuExitCode::Failed);
    }
    ostd::init_syscall_msrs();
    ostd::disable_smap_for_loader();
    kprintln!("[ZENITH-OSTD] GDT/TSS, IDT (#BP), and SYSCALL/SYSRETQ MSRs ready.");

    // 2. Khởi tạo Ngữ cảnh Nhân Bền vững & VFS
    // SAFETY:
    // Khởi tạo duy nhất 1 lần lúc boot trên BSP trước khi nhảy vào Ring 3.
    let ctx = unsafe { &mut *GLOBAL_KERNEL_CTX.0.get() };
    ctx.init_hardware_pmm();
    kprintln!(
        "[ZENITH-VFS] Mounted In-Memory VFS ('/', '/etc/os-release', '/proc/version', '/bin/init', '/bin/app')."
    );

    // 3. Xây dựng và nạp ảnh thực thi Linux ELF64 (`/bin/init`) vào vùng nhớ User-space
    let elf_image = linux_abi::build_embedded_linux_init_elf64();
    let loaded = match linux_abi::load_elf64_into_user_space(&elf_image, &mut ctx.pmm) {
        Ok(info) => info,
        Err(err) => {
            kprintln!("[ZENITH-ELF][FAIL] ELF64 Loader error: {:?}", err);
            ostd::exit_qemu(ostd::QemuExitCode::Failed);
        }
    };
    ctx.program_break = loaded.initial_brk;
    kprintln!(
        "[ZENITH-ELF] Loaded initial ELF64 '/bin/init': entry=0x{:x}, user_rsp=0x{:x}",
        loaded.entry_rip,
        loaded.user_rsp
    );

    // 4. Nhảy xuống Ring 3 chạy `/bin/init` -> đọc VFS -> gọi `SYS_execve("/bin/app")`!
    kprintln!("[ZENITH-CORE] Entering Ring 3 ('/bin/init' -> VFS -> SYS_execve -> '/bin/app')...");
    let exit_code = ostd::enter_user_space(loaded.entry_rip, loaded.user_rsp);
    kprintln!(
        "[ZENITH-CORE] Ring 3 process '/bin/app' exited cleanly back to Kernel with token=0x{:x}.",
        exit_code
    );

    // 5. Xác thực toàn bộ chuỗi thực thi VFS & SYS_execve:
    if exit_code != 0x44 {
        kprintln!(
            "[ZENITH-TEST][FAIL] Unexpected Ring 3 exit token: 0x{:x} (expected 0x44)",
            exit_code
        );
        ostd::exit_qemu(ostd::QemuExitCode::Failed);
    }

    // - Kiểm tra nội dung `/etc/os-release` mà `/bin/init` đã đọc từ VFS vào `0x1000_0500`
    let mut os_rel_verify = [0u8; 15];
    if ostd::copy_from_buffer(0x1000_0500, &mut os_rel_verify).is_err()
        || &os_rel_verify != b"NAME=\"ZenithOS\""
    {
        kprintln!("[ZENITH-TEST][FAIL] VFS /etc/os-release read content mismatch!");
        ostd::exit_qemu(ostd::QemuExitCode::Failed);
    }
    kprintln!("[ZENITH-VFS] Verified Ring 3 SYS_openat & SYS_read ('NAME=\"ZenithOS\"').");

    // - Kiểm tra chữ ký `"ZEN4"` mà `/bin/app` đã ghi tại `0x1000_0600` sau khi được `SYS_execve`
    let mut exec_sig = [0u8; 4];
    if ostd::copy_from_buffer(0x1000_0600, &mut exec_sig).is_err() || &exec_sig != b"ZEN4" {
        kprintln!("[ZENITH-TEST][FAIL] SYS_execve '/bin/app' memory signature mismatch!");
        ostd::exit_qemu(ostd::QemuExitCode::Failed);
    }
    kprintln!("[ZENITH-VFS] Verified Ring 3 SYS_getcwd, SYS_getdents64 & SYS_execve('/bin/app').");

    kprintln!("================================================================");
    kprintln!("[ZENITH-CI] ALL PHASE 4 VFS, INITRAMFS & EXECVE TESTS PASSED!   ");
    kprintln!("================================================================");

    ostd::exit_qemu(ostd::QemuExitCode::Success);
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    kprintln!("\n[ZENITH-PANIC] Kernel panicked: {}", info);
    ostd::exit_qemu(ostd::QemuExitCode::Failed);
}
