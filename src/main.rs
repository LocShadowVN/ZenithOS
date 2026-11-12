#![no_std]
#![no_main]
#![deny(unsafe_op_in_unsafe_fn)]
#![allow(clippy::fn_to_numeric_cast)]

use core::panic::PanicInfo;
use core::sync::atomic::Ordering;

// ============================================================================
// GIAO THỨC KHỞI ĐỘNG LIMINE (HHDM & KERNEL PHYSICAL ADDRESS)
// ============================================================================
pub mod boot_protocol {
    #[repr(C)]
    pub struct LimineBaseRevision {
        pub id: [u64; 2],
        pub revision: u64,
    }

    unsafe impl Sync for LimineBaseRevision {}

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

    unsafe impl Sync for LimineHhdmRequest {}

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

    unsafe impl Sync for LimineKernelAddressRequest {}

    #[used]
    #[link_section = ".requests"]
    pub static BASE_REVISION: LimineBaseRevision = LimineBaseRevision {
        id: [0xf956_2b2d_5c95_a6c8, 0x6a7b_3849_4453_6bdc],
        revision: 2,
    };

    #[used]
    #[link_section = ".requests"]
    pub static HHDM_REQUEST: LimineHhdmRequest = LimineHhdmRequest {
        id: [
            0xc7b1_dd30_df4c_8b88,
            0x0a82_e883_a194_f07b,
            0x48dc_f1cb_8ad2_b852,
            0x6398_4e95_9a98_244b,
        ],
        revision: 0,
        response: core::ptr::null(),
    };

    #[used]
    #[link_section = ".requests"]
    pub static KERNEL_ADDRESS_REQUEST: LimineKernelAddressRequest = LimineKernelAddressRequest {
        id: [
            0xc7b1_dd30_df4c_8b88,
            0x0a82_e883_a194_f07b,
            0x71ba_7686_3cc5_5f63,
            0xb264_4a48_c516_a487,
        ],
        revision: 0,
        response: core::ptr::null(),
    };

    #[used]
    #[link_section = ".requests_start_marker"]
    pub static REQUESTS_START_MARKER: [u64; 4] = [
        0xf6b8_f4b3_9de7_d1ae,
        0xfab9_1a69_40fc_b9cf,
        0x785c_6ed0_15d3_e316,
        0x181e_920a_7852_b9d9,
    ];

    #[used]
    #[link_section = ".requests_end_marker"]
    pub static REQUESTS_END_MARKER: [u64; 2] = [0xadc0_e053_1bb1_0d03, 0x9572_709f_3176_4c62];
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
    // 1.1: PORT I/O, SERIAL CONSOLE (UART 16550) & QEMU EXIT DEVICE
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
        hhdm_offset: u64,
        kernel_phys_base: u64,
        kernel_virt_base: u64,
    }

    impl Default for BitmapFrameAllocator {
        fn default() -> Self {
            Self::new()
        }
    }

    impl BitmapFrameAllocator {
        pub fn new() -> Self {
            let base_addr = &raw const STATIC_FRAME_POOL as u64;
            let mut hhdm_offset = 0xFFFF_8000_0000_0000;
            let mut kernel_phys_base = 0;
            let mut kernel_virt_base = 0xFFFF_FFFF_8000_0000;

            // SAFETY:
            // Đọc thông tin HHDM và Kernel Address do Limine điền trước khi nhảy vào `_start`.
            unsafe {
                let hhdm_resp = crate::boot_protocol::HHDM_REQUEST.response;
                if !hhdm_resp.is_null() {
                    hhdm_offset = (*hhdm_resp).offset;
                }
                let ka_resp = crate::boot_protocol::KERNEL_ADDRESS_REQUEST.response;
                if !ka_resp.is_null() {
                    kernel_phys_base = (*ka_resp).physical_base;
                    kernel_virt_base = (*ka_resp).virtual_base;
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

        /// Chuyển đổi địa chỉ ảo trong vùng Kernel `.bss` sang địa chỉ vật lý phần cứng.
        #[inline]
        pub const fn kernel_virt_to_phys(&self, virt_addr: u64) -> u64 {
            (virt_addr - self.kernel_virt_base) + self.kernel_phys_base
        }

        /// Chuyển đổi địa chỉ vật lý phần cứng sang địa chỉ ảo HHDM để CPU đọc/ghi bảng trang.
        #[inline]
        pub const fn phys_to_hhdm_virt(&self, phys_addr: u64) -> u64 {
            phys_addr + self.hhdm_offset
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct PageFlags(pub u64);

    impl PageFlags {
        pub const PRESENT: u64 = 1 << 0;
        pub const WRITABLE: u64 = 1 << 1;
        pub const USER: u64 = 1 << 2;
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

    /// Tắt SMAP/SMEP tạm thời trên CR4 nếu CPU bật sẵn để Kernel nạp dữ liệu vào trang User-space.
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
        let pml4_virt = pmm.phys_to_hhdm_virt(pml4_phys) as *mut PageTable;

        let pdpt_phys = ensure_next_table(pmm, pml4_virt, p4_idx)?;
        let pdpt_virt = pmm.phys_to_hhdm_virt(pdpt_phys) as *mut PageTable;

        let pd_phys = ensure_next_table(pmm, pdpt_virt, p3_idx)?;
        let pd_virt = pmm.phys_to_hhdm_virt(pd_phys) as *mut PageTable;

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
        table_virt: *mut PageTable,
        idx: usize,
    ) -> Result<u64, OstdError> {
        // SAFETY:
        // `table_virt` là địa chỉ ảo HHDM của bảng trang hiện tại, `idx` nằm trong khoảng `0..512`.
        unsafe {
            let entry = (*table_virt).entries[idx];
            if (entry & PageFlags::PRESENT) != 0 {
                // Đảm bảo mục trung gian cho phép truy cập USER, WRITABLE và không chặn thực thi (clear NX bit 63)
                let updated = (entry | PageFlags::USER_RWX) & !(1u64 << 63);
                (*table_virt).entries[idx] = updated;
                Ok(updated & 0x000F_FFFF_FFFF_F000)
            } else {
                let new_table_kvirt = pmm.allocate_frame()?;
                let new_table_phys = pmm.kernel_virt_to_phys(new_table_kvirt);
                (*table_virt).entries[idx] =
                    (new_table_phys & 0x000F_FFFF_FFFF_F000) | PageFlags::USER_RWX;
                Ok(new_table_phys)
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
        /// Offset 0x00: Đỉnh Kernel Stack dùng cho trình xử lý `syscall`
        pub kernel_rsp: u64,
        /// Offset 0x08: Biến tạm giữ User Stack (`RSP`) khi vừa gọi lệnh `syscall`
        pub user_rsp_scratch: u64,
        /// Offset 0x10: Kernel Stack (`RSP`) đã lưu của trình gọi `enter_user_space`
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

    /// Chuyển CPU từ Ring 0 xuống Ring 3 tại `entry_rip` với ngăn xếp `user_rsp` bằng `sysretq`.
    /// Hàm sẽ trả về mã thoát `exit_code` khi tiến trình Ring 3 gọi `SYS_exit_group`.
    pub fn enter_user_space(entry_rip: u64, user_rsp: u64) -> i32 {
        // SAFETY:
        // `entry_rip` và `user_rsp` đã được trình nạp ELF64 ánh xạ hợp lệ với cờ `PAGE_USER`.
        unsafe { zenith_enter_user_space_asm(entry_rip, user_rsp) }
    }

    /// Khôi phục ngăn xếp Kernel và quay về điểm gọi `enter_user_space` khi tiến trình Ring 3 thoát.
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
// MODULE 2: SAFE KERNEL CORE, ELF64 LOADER & LINUX ABI (#![forbid(unsafe_code)])
// Toàn bộ tầng này cấm tuyệt đối `unsafe` và cấm `unwrap()` / `expect()`.
// ============================================================================
#[forbid(unsafe_code)]
pub mod linux_abi {
    use crate::ostd::{
        copy_from_buffer, copy_to_buffer, map_user_page, read_msr, write_msr,
        BitmapFrameAllocator, Msr, SerialConsole, TrapFrame, PAGE_SIZE,
    };

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum LinuxErrno {
        Eperm = 1,
        Enoent = 2,
        Ebadf = 9,
        Enomem = 12,
        Eacces = 13,
        Efault = 14,
        Einval = 22,
        Enosys = 38,
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
    pub const SYS_MMAP: u64 = 9;
    pub const SYS_BRK: u64 = 12;
    pub const SYS_IOCTL: u64 = 16;
    pub const SYS_WRITEV: u64 = 20;
    pub const SYS_GETPID: u64 = 39;
    pub const SYS_EXIT: u64 = 60;
    pub const SYS_UNAME: u64 = 63;
    pub const SYS_ARCH_PRCTL: u64 = 158;
    pub const SYS_GETTID: u64 = 186;
    pub const SYS_SET_TID_ADDRESS: u64 = 218;
    pub const SYS_EXIT_GROUP: u64 = 231;

    pub const ARCH_SET_GS: u64 = 0x1001;
    pub const ARCH_SET_FS: u64 = 0x1002;
    pub const ARCH_GET_FS: u64 = 0x1003;
    pub const ARCH_GET_GS: u64 = 0x1004;

    pub const TIOCGWINSZ: u64 = 0x5413;

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

    pub struct LinuxProcessState {
        pub pid: u64,
        pub tid: u64,
        pub clear_child_tid: u64,
        pub program_break: u64,
        pub last_exit_code: Option<i32>,
    }

    impl LinuxProcessState {
        pub const fn new(pid: u64, initial_brk: u64) -> Self {
            Self {
                pid,
                tid: pid,
                clear_child_tid: 0,
                program_break: initial_brk,
                last_exit_code: None,
            }
        }
    }

    pub fn dispatch_linux_syscall(
        frame: &mut TrapFrame,
        proc_state: &mut LinuxProcessState,
        pmm: &mut BitmapFrameAllocator,
    ) {
        let result = match frame.rax {
            SYS_READ => sys_read(frame.rdi, frame.rsi, frame.rdx),
            SYS_WRITE => sys_write(frame.rdi, frame.rsi, frame.rdx),
            SYS_MMAP => sys_mmap(frame.rdi, frame.rsi, pmm),
            SYS_BRK => sys_brk(frame.rdi, proc_state),
            SYS_IOCTL => sys_ioctl(frame.rdi, frame.rsi, frame.rdx),
            SYS_WRITEV => sys_writev(frame.rdi, frame.rsi, frame.rdx),
            SYS_GETPID => Ok(proc_state.pid),
            SYS_GETTID => Ok(proc_state.tid),
            SYS_SET_TID_ADDRESS => {
                proc_state.clear_child_tid = frame.rdi;
                Ok(proc_state.tid)
            }
            SYS_UNAME => sys_uname(frame.rdi),
            SYS_ARCH_PRCTL => sys_arch_prctl(frame.rdi, frame.rsi),
            SYS_EXIT | SYS_EXIT_GROUP => {
                proc_state.last_exit_code = Some(frame.rdi as i32);
                Ok(0)
            }
            _ => Err(LinuxErrno::Enosys),
        };

        frame.rax = match result {
            Ok(val) => val,
            Err(errno) => errno.as_neg_ret(),
        };
    }

    fn sys_read(fd: u64, _buf_ptr: u64, _count: u64) -> Result<u64, LinuxErrno> {
        if fd == 0 {
            Ok(0)
        } else {
            Err(LinuxErrno::Ebadf)
        }
    }

    fn sys_write(fd: u64, buf_ptr: u64, count: u64) -> Result<u64, LinuxErrno> {
        if fd != 1 && fd != 2 {
            return Err(LinuxErrno::Ebadf);
        }
        if count == 0 {
            return Ok(0);
        }
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

    /// Hiện thực `SYS_writev` (vectored I/O) phục vụ `musl libc` (`struct iovec { base: u64, len: u64 }`).
    fn sys_writev(fd: u64, iov_ptr: u64, iovcnt: u64) -> Result<u64, LinuxErrno> {
        if fd != 1 && fd != 2 {
            return Err(LinuxErrno::Ebadf);
        }
        if iovcnt > 1024 {
            return Err(LinuxErrno::Einval);
        }
        let mut total_written: u64 = 0;
        let mut idx: u64 = 0;
        while idx < iovcnt {
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
                let n = sys_write(fd, iov_base, iov_len)?;
                total_written = total_written.wrapping_add(n);
            }
            idx += 1;
        }
        Ok(total_written)
    }

    fn sys_ioctl(fd: u64, request: u64, argp: u64) -> Result<u64, LinuxErrno> {
        if fd <= 2 && request == TIOCGWINSZ {
            // struct winsize { ws_row: 25, ws_col: 80, ws_xpixel: 0, ws_ypixel: 0 }
            let winsize: [u8; 8] = [25, 0, 80, 0, 0, 0, 0, 0];
            copy_to_buffer(argp, &winsize).map_err(|_| LinuxErrno::Efault)?;
            Ok(0)
        } else {
            Err(LinuxErrno::Einval)
        }
    }

    fn sys_mmap(
        _addr_hint: u64,
        length: u64,
        pmm: &mut BitmapFrameAllocator,
    ) -> Result<u64, LinuxErrno> {
        if length == 0 || length > 4096 {
            return Err(LinuxErrno::Einval);
        }
        pmm.allocate_frame().map_err(|_| LinuxErrno::Enomem)
    }

    fn sys_brk(new_brk: u64, proc_state: &mut LinuxProcessState) -> Result<u64, LinuxErrno> {
        if new_brk != 0 && new_brk >= proc_state.program_break {
            proc_state.program_break = new_brk;
        }
        Ok(proc_state.program_break)
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

    // ------------------------------------------------------------------------
    // 2.2: TRÌNH PHÂN TÍCH & NẠP LINUX ELF64 (100% SAFE RUST)
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

    /// Phân tích ảnh nhị phân ELF64, ánh xạ các phân đoạn `PT_LOAD` vào không gian địa chỉ User-space,
    /// và thiết lập ngăn xếp khởi tạo chuẩn System V AMD64 (`argc`, `argv`, `envp`, `auxv`).
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
        // EI_CLASS == 2 (64-bit), EI_DATA == 1 (Little-Endian)
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
            let seg_end_page = (seg_end.wrapping_add(0xFFF)) & !0xFFF;

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

        // Cấp phát ngăn xếp User-space (8 KiB tại 0x0080_0000 .. 0x0080_2000)
        let stack_bottom: u64 = 0x0000_0000_0080_0000;
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

    /// Khởi tạo ngăn xếp chuẩn System V AMD64 ABI cho tiến trình Linux User-space:
    /// `[rsp]` -> `argc = 2`, `argv[0]`, `argv[1]`, `NULL`, `envp[0]`, `NULL`, `auxv[...]`.
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

        // Bảng gồm 16 phần tử u64 (128 bytes, căn lề 16-byte hoàn hảo):
        // [0]: argc = 2
        // [1]: argv[0], [2]: argv[1], [3]: NULL
        // [4]: envp[0], [5]: NULL
        // [6..15]: Auxiliary Vector (AT_PAGESZ, AT_ENTRY, AT_PHENT, AT_PHNUM, AT_NULL)
        let table_words: [u64; 16] = [
            2,          // argc
            arg0_va,    // argv[0] = "/bin/init"
            arg1_va,    // argv[1] = "--ci-verify"
            0,          // argv[2] = NULL
            env0_va,    // envp[0] = "OS=ZenithOS"
            0,          // envp[1] = NULL
            AT_PAGESZ,  // auxv[0].a_type
            4096,       // auxv[0].a_val
            AT_ENTRY,   // auxv[1].a_type
            entry_rip,  // auxv[1].a_val
            AT_PHENT,   // auxv[2].a_type
            56,         // auxv[2].a_val
            AT_PHNUM,   // auxv[3].a_type
            phnum,      // auxv[3].a_val
            AT_NULL,    // auxv[4].a_type
            0,          // auxv[4].a_val
        ];

        let rsp = str_area - 128;
        for (i, &word) in table_words.iter().enumerate() {
            let addr = rsp + (i as u64) * 8;
            copy_to_buffer(addr, &word.to_le_bytes()).map_err(|_| ElfLoadError::MappingFailed)?;
        }

        Ok(rsp)
    }

    /// Đóng gói một ảnh thực thi Linux `x86_64` ELF64 hoàn chỉnh (`/bin/init`) chứa mã máy Ring 3 thật.
    /// Tiến trình này sẽ chạy ở `CPL = 3` (`0x0040_1000`), đọc `argc` từ `[rsp]`, và gọi 6 syscall:
    /// `SYS_getpid`, `SYS_set_tid_address`, `SYS_write`, `SYS_writev`, `SYS_brk`, `SYS_exit_group`.
    pub fn build_embedded_linux_init_elf64() -> [u8; 768] {
        let mut elf = [0u8; 768];

        // 1. ELF64 Header (64 bytes tại offset 0x00)
        elf[0..4].copy_from_slice(b"\x7FELF");
        elf[4] = 2; // ELFCLASS64
        elf[5] = 1; // ELFDATA2LSB
        elf[6] = 1; // EV_CURRENT
        elf[16..18].copy_from_slice(&2u16.to_le_bytes()); // ET_EXEC
        elf[18..20].copy_from_slice(&62u16.to_le_bytes()); // EM_X86_64
        elf[20..24].copy_from_slice(&1u32.to_le_bytes()); // EV_CURRENT
        elf[24..32].copy_from_slice(&0x0040_1000u64.to_le_bytes()); // e_entry = 0x401000
        elf[32..40].copy_from_slice(&64u64.to_le_bytes()); // e_phoff = 64
        elf[52..54].copy_from_slice(&64u16.to_le_bytes()); // e_ehsize = 64
        elf[54..56].copy_from_slice(&56u16.to_le_bytes()); // e_phentsize = 56
        elf[56..58].copy_from_slice(&2u16.to_le_bytes()); // e_phnum = 2

        // 2. Program Header #0: PT_LOAD (.text tại 0x0040_1000, file offset 256, size 128)
        let ph0 = 64;
        elf[ph0..ph0 + 4].copy_from_slice(&PT_LOAD.to_le_bytes());
        elf[ph0 + 4..ph0 + 8].copy_from_slice(&5u32.to_le_bytes()); // PF_R | PF_X
        elf[ph0 + 8..ph0 + 16].copy_from_slice(&256u64.to_le_bytes()); // p_offset
        elf[ph0 + 16..ph0 + 24].copy_from_slice(&0x0040_1000u64.to_le_bytes()); // p_vaddr
        elf[ph0 + 24..ph0 + 32].copy_from_slice(&0x0040_1000u64.to_le_bytes()); // p_paddr
        elf[ph0 + 32..ph0 + 40].copy_from_slice(&128u64.to_le_bytes()); // p_filesz
        elf[ph0 + 40..ph0 + 48].copy_from_slice(&128u64.to_le_bytes()); // p_memsz
        elf[ph0 + 48..ph0 + 56].copy_from_slice(&0x1000u64.to_le_bytes()); // p_align

        // 3. Program Header #1: PT_LOAD (.data tại 0x0040_2000, file offset 384, size 256)
        let ph1 = 120;
        elf[ph1..ph1 + 4].copy_from_slice(&PT_LOAD.to_le_bytes());
        elf[ph1 + 4..ph1 + 8].copy_from_slice(&6u32.to_le_bytes()); // PF_R | PF_W
        elf[ph1 + 8..ph1 + 16].copy_from_slice(&384u64.to_le_bytes()); // p_offset
        elf[ph1 + 16..ph1 + 24].copy_from_slice(&0x0040_2000u64.to_le_bytes()); // p_vaddr
        elf[ph1 + 24..ph1 + 32].copy_from_slice(&0x0040_2000u64.to_le_bytes()); // p_paddr
        elf[ph1 + 32..ph1 + 40].copy_from_slice(&256u64.to_le_bytes()); // p_filesz
        elf[ph1 + 40..ph1 + 48].copy_from_slice(&256u64.to_le_bytes()); // p_memsz
        elf[ph1 + 48..ph1 + 56].copy_from_slice(&0x1000u64.to_le_bytes()); // p_align

        // 4. Mã máy Ring 3 x86_64 tại file offset 256 (ánh xạ vào 0x0040_1000):
        let code: [u8; 98] = [
            0x4c, 0x8b, 0x24, 0x24, // mov r12, qword ptr [rsp] (đọc argc = 2)
            0xb8, 0x27, 0x00, 0x00, 0x00, // mov eax, 39 (SYS_getpid)
            0x0f, 0x05, // syscall
            0x49, 0x89, 0xc5, // mov r13, rax (lưu PID = 1)
            0xb8, 0xda, 0x00, 0x00, 0x00, // mov eax, 218 (SYS_set_tid_address)
            0xbf, 0x00, 0x21, 0x40, 0x00, // mov edi, 0x00402100
            0x0f, 0x05, // syscall
            0x49, 0x89, 0xc6, // mov r14, rax (lưu TID = 1)
            0xb8, 0x01, 0x00, 0x00, 0x00, // mov eax, 1 (SYS_write)
            0xbf, 0x01, 0x00, 0x00, 0x00, // mov edi, 1 (stdout)
            0xbe, 0x00, 0x20, 0x40, 0x00, // mov esi, 0x00402000 (msg1)
            0xba, 0x48, 0x00, 0x00, 0x00, // mov edx, 72
            0x0f, 0x05, // syscall
            0xb8, 0x14, 0x00, 0x00, 0x00, // mov eax, 20 (SYS_writev)
            0xbf, 0x01, 0x00, 0x00, 0x00, // mov edi, 1 (stdout)
            0xbe, 0x90, 0x20, 0x40, 0x00, // mov esi, 0x00402090 (iovec[2])
            0xba, 0x02, 0x00, 0x00, 0x00, // mov edx, 2 (iovcnt = 2)
            0x0f, 0x05, // syscall
            0xb8, 0x0c, 0x00, 0x00, 0x00, // mov eax, 12 (SYS_brk)
            0xbf, 0x00, 0x80, 0x40, 0x00, // mov edi, 0x00408000
            0x0f, 0x05, // syscall
            // Kết hợp mã xác thực: rdi = (argc << 4) | (pid << 2) | tid = (2 << 4) | (1 << 2) | 1 = 0x25 (37)
            0x4c, 0x89, 0xe7, // mov rdi, r12
            0x48, 0xc1, 0xe7, 0x04, // shl rdi, 4
            0x49, 0xc1, 0xe5, 0x02, // shl r13, 2
            0x4c, 0x09, 0xef, // or rdi, r13
            0x4c, 0x09, 0xf7, // or rdi, r14
            0xb8, 0xe7, 0x00, 0x00, 0x00, // mov eax, 231 (SYS_exit_group)
            0x0f, 0x05, // syscall
        ];
        elf[256..256 + code.len()].copy_from_slice(&code);

        // 5. Dữ liệu .data tại file offset 384 (ánh xạ vào 0x0040_2000):
        let msg1 = b"[RING-3 ELF64] Hello from Linux User-Space (CPL=3) via SYSCALL/SYSRETQ!\n";
        elf[384..384 + 72].copy_from_slice(msg1);

        let iov_part1 = b"[RING-3 ELF64] musl-style "; // 26 bytes tại 0x0040_2048 (file offset 456)
        elf[456..456 + 26].copy_from_slice(iov_part1);

        let iov_part2 = b"SYS_writev(iovcnt=2) verified in Ring 3!\n"; // 41 bytes tại 0x0040_2068 (file offset 488)
        elf[488..488 + 41].copy_from_slice(iov_part2);

        // Mảng `struct iovec[2]` tại 0x0040_2090 (file offset 528):
        // iov[0] = { base: 0x0040_2048, len: 26 }
        // iov[1] = { base: 0x0040_2068, len: 41 }
        elf[528..536].copy_from_slice(&0x0040_2048u64.to_le_bytes());
        elf[536..544].copy_from_slice(&26u64.to_le_bytes());
        elf[544..552].copy_from_slice(&0x0040_2068u64.to_le_bytes());
        elf[552..560].copy_from_slice(&41u64.to_le_bytes());

        elf
    }
}

// ============================================================================
// ĐIỂM NỐI SYSCALL TỪ RING 3 & QUẢN LÝ TRẠNG THÁI TIẾN TRÌNH HIỆN HÀNH
// ============================================================================
#[no_mangle]
extern "C" fn zenith_syscall_entry_from_asm(frame: &mut ostd::TrapFrame) {
    let mut active_proc = linux_abi::LinuxProcessState::new(1, 0x0000_0000_0040_3000);
    let mut pmm = ostd::BitmapFrameAllocator::new();
    linux_abi::dispatch_linux_syscall(frame, &mut active_proc, &mut pmm);

    // Nếu tiến trình Ring 3 vừa gọi SYS_exit hoặc SYS_exit_group, thoát khỏi ngữ cảnh User-space
    // và trả mã thoát về cho hàm `ostd::enter_user_space` trong Kernel!
    if let Some(exit_code) = active_proc.last_exit_code {
        ostd::return_to_kernel_from_exit(exit_code);
    }
}

macro_rules! kprintln {
    ($($arg:tt)*) => {
        $crate::ostd::print_fmt(core::format_args!($($arg)*));
        $crate::ostd::print_fmt(core::format_args!("\n"));
    };
}

/// Điểm vào chính của Kernel ZenithOS (Phase 1 + Phase 2: ELF64 & Ring 3 User-Space).
#[no_mangle]
pub extern "C" fn _start() -> ! {
    ostd::SerialConsole::init();
    kprintln!("================================================================");
    kprintln!("  ZenithOS v0.2.0 - 100% Rust Framekernel (Linux ABI Phase 2)   ");
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

    // 2. Khởi tạo PMM (kèm HHDM & Kernel Address từ Limine)
    let mut pmm = ostd::BitmapFrameAllocator::new();

    // 3. Xây dựng và nạp ảnh thực thi Linux ELF64 (`/bin/init`) vào vùng nhớ User-space
    let elf_image = linux_abi::build_embedded_linux_init_elf64();
    let loaded = match linux_abi::load_elf64_into_user_space(&elf_image, &mut pmm) {
        Ok(info) => info,
        Err(err) => {
            kprintln!("[ZENITH-ELF][FAIL] ELF64 Loader error: {:?}", err);
            ostd::exit_qemu(ostd::QemuExitCode::Failed);
        }
    };
    kprintln!(
        "[ZENITH-ELF] Loaded Linux x86_64 ELF64 '/bin/init': entry=0x{:x}, user_rsp=0x{:x}, brk=0x{:x}",
        loaded.entry_rip,
        loaded.user_rsp,
        loaded.initial_brk
    );

    // 4. Nhảy xuống Ring 3 (CPL = 3) qua `sysretq` để thực thi tiến trình Linux `/bin/init`!
    kprintln!("[ZENITH-CORE] Transitioning to Ring 3 User-Space via SYSRETQ...");
    let exit_code = ostd::enter_user_space(loaded.entry_rip, loaded.user_rsp);
    kprintln!(
        "[ZENITH-CORE] Ring 3 process '/bin/init' exited cleanly back to Kernel with token=0x{:x}.",
        exit_code
    );

    // Kiểm tra mã xác thực trả về từ Ring 3:
    // (argc=2 << 4) | (pid=1 << 2) | (tid=1) = 0x25 (37)
    if exit_code != 0x25 {
        kprintln!(
            "[ZENITH-TEST][FAIL] Unexpected Ring 3 exit token: 0x{:x} (expected 0x25)",
            exit_code
        );
        ostd::exit_qemu(ostd::QemuExitCode::Failed);
    }

    kprintln!("================================================================");
    kprintln!("[ZENITH-CI] ALL PHASE 2 ELF64 & RING 3 USER-SPACE TESTS PASSED! ");
    kprintln!("================================================================");

    ostd::exit_qemu(ostd::QemuExitCode::Success);
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    kprintln!("\n[ZENITH-PANIC] Kernel panicked: {}", info);
    ostd::exit_qemu(ostd::QemuExitCode::Failed);
}
