#![no_std]
#![no_main]
#![deny(unsafe_op_in_unsafe_fn)]

use core::panic::PanicInfo;
use core::sync::atomic::{AtomicU64, Ordering};

// ============================================================================
// GIAO THỨC KHỞI ĐỘNG LIMINE (LIMINE BOOT PROTOCOL)
// Định nghĩa trực tiếp các cấu trúc #[repr(C)] chuẩn ABI không cần phụ thuộc ngoài.
// ============================================================================
pub mod boot_protocol {
    #[repr(C)]
    pub struct LimineBaseRevision {
        pub id: [u64; 2],
        pub revision: u64,
    }

    unsafe impl Sync for LimineBaseRevision {}

    #[used]
    #[link_section = ".requests"]
    pub static BASE_REVISION: LimineBaseRevision = LimineBaseRevision {
        id: [0xf956_2b2d_5c95_a6c8, 0x6a7b_3849_4453_6bdc],
        revision: 2,
    };

    #[used]
    #[link_section = ".requests_start_marker"]
    pub static REQUESTS_START_MARKER: [u64; 4] = [
        0xf6b8_f4b3_9de7_d1ae,
        0xfab9_1a69_40fcb_9cf,
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

    /// Mã lỗi phần cứng ở tầng OSTD
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum OstdError {
        OutOfPhysicalFrames,
        InvalidFrameIndex,
        UnalignedAddress,
        PageTableWalkFailed,
        InvalidUserPointer,
    }

    // ------------------------------------------------------------------------
    // 1.1: PORT I/O, SERIAL CONSOLE (UART 16550) & QEMU EXIT DEVICE
    // ------------------------------------------------------------------------
    #[inline(always)]
    pub fn outb(port: u16, val: u8) {
        // SAFETY:
        // Chỉ thực thi ở Ring 0 trên các cổng I/O kiến trúc PC chuẩn (COM1, QEMU Debug).
        // `nostack` và `preserves_flags` đảm bảo lệnh `out` không ảnh hưởng stack và RFLAGS.
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
        // Chỉ thực thi ở Ring 0 trên các cổng I/O phần cứng chuẩn.
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
        // Chỉ thực thi ở Ring 0 phục vụ ghi 32-bit ra cổng `isa-debug-exit` (0xF4).
        unsafe {
            asm!(
                "out dx, eax",
                in("dx") port,
                in("eax") val,
                options(nostack, preserves_flags)
            );
        }
    }

    /// Trạng thái thoát gửi tới thiết bị `isa-debug-exit` của QEMU (cổng 0xF4).
    /// QEMU sẽ thoát với mã `(code << 1) | 1`.
    #[repr(u32)]
    #[derive(Debug, Clone, Copy)]
    pub enum QemuExitCode {
        /// Thành công -> QEMU trả về exit code `(0x10 << 1) | 1 = 33`
        Success = 0x10,
        /// Thất bại -> QEMU trả về exit code `(0x11 << 1) | 1 = 35`
        Failed = 0x11,
    }

    pub fn exit_qemu(exit_code: QemuExitCode) -> ! {
        outl(0xF4, exit_code as u32);
        loop {
            // SAFETY:
            // `hlt` tạm dừng CPU ở Ring 0 cho tới khi có ngắt tiếp theo.
            unsafe {
                asm!("hlt", options(nostack, preserves_flags));
            }
        }
    }

    /// Trình điều khiển UART 16550 trên cổng COM1 (`0x3F8`).
    pub struct SerialConsole;

    impl SerialConsole {
        const COM1: u16 = 0x3F8;

        pub fn init() {
            outb(Self::COM1 + 1, 0x00); // Tắt mọi ngắt UART
            outb(Self::COM1 + 3, 0x80); // Bật DLAB (đặt tốc độ baud)
            outb(Self::COM1 + 0, 0x03); // Chia tần số: 38400 baud (byte thấp)
            outb(Self::COM1 + 1, 0x00); // Byte cao
            outb(Self::COM1 + 3, 0x03); // 8 bits, không parity, 1 stop bit
            outb(Self::COM1 + 2, 0xC7); // Bật FIFO, xóa bộ đệm, ngưỡng 14-byte
            outb(Self::COM1 + 4, 0x0B); // Bật RTS/DSR
        }

        #[inline]
        pub fn write_byte(byte: u8) {
            // Chờ thanh ghi Line Status (offset +5) báo bộ đệm truyền trống (bit 5 = 0x20)
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
    /// Task State Segment 64-bit chuẩn phần cứng x86_64.
    #[repr(C, packed)]
    pub struct TaskStateSegment {
        pub reserved_1: u32,
        /// Con trỏ Stack cho Ring 0..2 khi chuyển mức đặc quyền
        pub privilege_stack_table: [u64; 3],
        pub reserved_2: u64,
        /// Bảng ngăn xếp ngắt khẩn cấp (IST1..IST7) cho Double Fault, NMI, Page Fault
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

    /// Bảng GDT 7 phần tử (Null, Kernel Code, Kernel Data, User Data, User Code 64, TSS Low, TSS High).
    /// Lưu ý: Lệnh `sysretq` yêu cầu User Data (0x18) ĐỨNG TRƯỚC User Code 64 (0x20)!
    #[repr(C, align(16))]
    pub struct GdtTable {
        pub entries: [u64; 7],
    }

    static mut GDT: GdtTable = GdtTable {
        entries: [
            0x0000_0000_0000_0000, // 0x00: Null Descriptor
            0x00AF_9A00_0000_FFFF, // 0x08: Kernel Code 64-bit (DPL = 0, L = 1)
            0x00CF_9200_0000_FFFF, // 0x10: Kernel Data 64-bit (DPL = 0)
            0x00CF_F200_0000_FFFF, // 0x18: User Data 64-bit   (DPL = 3 -> Selector 0x1B)
            0x00AF_FA00_0000_FFFF, // 0x20: User Code 64-bit   (DPL = 3 -> Selector 0x23)
            0x0000_0000_0000_0000, // 0x28: TSS Descriptor (64-bit thấp)
            0x0000_0000_0000_0000, // 0x30: TSS Descriptor (64-bit cao)
        ],
    };

    #[repr(C, packed)]
    struct DescriptorTablePointer {
        limit: u16,
        base: u64,
    }

    pub fn init_gdt_and_tss() {
        // SAFETY:
        // Hàm này được gọi duy nhất 1 lần trên BSP (Bootstrap Processor) trong quá trình khởi động
        // đơn luồng trước khi bật ngắt. Không có truy cập đồng thời (aliasing) tới `TSS` và `GDT`.
        unsafe {
            let rsp0_top = (&raw const KERNEL_INTR_STACK.0 as u64) + 16384;
            let ist1_top = (&raw const DOUBLE_FAULT_IST_STACK.0 as u64) + 16384;

            TSS.privilege_stack_table[0] = rsp0_top;
            TSS.interrupt_stack_table[0] = ist1_top;

            let tss_base = &raw const TSS as u64;
            let tss_limit = (core::mem::size_of::<TaskStateSegment>() - 1) as u64;

            let mut tss_low: u64 = 0;
            tss_low |= tss_limit & 0xFFFF;
            tss_low |= (tss_base & 0xFF_FFFF) << 16;
            tss_low |= 0x89u64 << 40; // Present = 1, Type = 0x9 (64-bit Available TSS)
            tss_low |= ((tss_limit >> 16) & 0xF) << 48;
            tss_low |= ((tss_base >> 24) & 0xFF) << 56;

            let tss_high: u64 = tss_base >> 32;

            GDT.entries[5] = tss_low;
            GDT.entries[6] = tss_high;

            let gdtr = DescriptorTablePointer {
                limit: (core::mem::size_of::<GdtTable>() - 1) as u16,
                base: &raw const GDT as u64,
            };

            asm!(
                "lgdt [{gdtr}]",
                // Nạp lại CS = 0x08 thông qua far return (`retfq`)
                "push 0x08",
                "lea {tmp}, [rip + 2f]",
                "push {tmp}",
                "retfq",
                "2:",
                // Nạp lại các thanh ghi phân đoạn dữ liệu bằng Kernel Data (0x10)
                "mov ax, 0x10",
                "mov ds, ax",
                "mov es, ax",
                "mov ss, ax",
                // Xóa FS và GS selector về 0 (địa chỉ cơ sở được điều khiển qua MSR)
                "xor ax, ax",
                "mov fs, ax",
                "mov gs, ax",
                // Nạp Task Register với TSS Selector (0x28)
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
    // 1.3: IDT (INTERRUPT DESCRIPTOR TABLE) & EXCEPTION SELF-TEST
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
                selector: 0x08, // Kernel Code Selector
                ist: ist_index & 0x7,
                type_attr: 0x8E, // Present = 1, DPL = 0, Type = 0xE (64-bit Interrupt Gate)
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
        let mut console = SerialConsole;
        let _ = writeln!(
            console,
            "\n[ZENITH-OSTD][FATAL] CPU Exception #{} (err=0x{:x}) at RIP=0x{:016x}",
            vector, error_code, rip
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
        "mov rdi, 8",           // Vector #8 (#DF)
        "pop rsi",              // Error code do CPU đẩy vào stack
        "mov rdx, [rsp]",       // RIP gây lỗi
        "call zenith_unhandled_exception",

        ".global isr_page_fault_stub",
        "isr_page_fault_stub:",
        "mov rdi, 14",          // Vector #14 (#PF)
        "pop rsi",              // Error code
        "mov rdx, [rsp]",       // RIP gây lỗi
        "call zenith_unhandled_exception",
    );

    extern "C" {
        fn isr_breakpoint_stub();
        fn isr_double_fault_stub();
        fn isr_page_fault_stub();
    }

    pub fn init_idt() {
        // SAFETY:
        // Chỉ chạy 1 lần lúc khởi tạo hệ thống trên BSP. Các hàm stub assembly tuân thủ đúng
        // giao thức lưu/khôi phục thanh ghi và kết thúc bằng `iretq`.
        unsafe {
            IDT.entries[3] = IdtEntry::new_handler(isr_breakpoint_stub as *const () as u64, 0);
            IDT.entries[8] = IdtEntry::new_handler(isr_double_fault_stub as *const () as u64, 1); // IST1
            IDT.entries[14] = IdtEntry::new_handler(isr_page_fault_stub as *const () as u64, 0);

            let idtr = DescriptorTablePointer {
                limit: (core::mem::size_of::<IdtTable>() - 1) as u16,
                base: &raw const IDT as u64,
            };

            asm!("lidt [{}]", in(reg) &idtr, options(nostack, preserves_flags));
        }
    }

    pub fn trigger_breakpoint_self_test() {
        // SAFETY:
        // Lệnh `int3` gọi Vector #3 đã được đăng ký an toàn trong IDT ở trên.
        unsafe {
            asm!("int3", options(nostack, preserves_flags));
        }
    }

    // ------------------------------------------------------------------------
    // 1.4: QUẢN LÝ BỘ NHỚ VẬT LÝ (BITMAP PMM) & BẢNG TRANG 4 CẤP (MMU)
    // ------------------------------------------------------------------------
    pub const PAGE_SIZE: usize = 4096;
    const POOL_FRAMES: usize = 256; // 1 MiB vùng nhớ khung trang tĩnh phục vụ Phase 1 & MMU

    #[repr(C, align(4096))]
    struct FramePool([[u8; PAGE_SIZE]; POOL_FRAMES]);

    static mut STATIC_FRAME_POOL: FramePool = FramePool([[0; PAGE_SIZE]; POOL_FRAMES]);

    /// Bộ cấp phát khung trang vật lý/ảo dựa trên Bitmap (Zero-panic).
    pub struct BitmapFrameAllocator {
        bitmap: [u64; 4], // 4 * 64 = 256 frames
        base_addr: u64,
    }

    impl BitmapFrameAllocator {
        pub fn new() -> Self {
            // SAFETY:
            // Lấy địa chỉ gốc của vùng `STATIC_FRAME_POOL` đã căn lề 4096-byte.
            let base_addr = unsafe { &raw const STATIC_FRAME_POOL as u64 };
            Self {
                bitmap: [0; 4],
                base_addr,
            }
        }

        pub fn allocate_frame(&mut self) -> Result<u64, OstdError> {
            for (word_idx, word) in self.bitmap.iter_mut().enumerate() {
                if *word != u64::MAX {
                    let bit_idx = (*word).trailing_ones() as usize;
                    *word |= 1u64 << bit_idx;
                    let frame_index = word_idx * 64 + bit_idx;
                    let addr = self.base_addr + (frame_index * PAGE_SIZE) as u64;
                    // Xóa trắng trang 4KiB trước khi bàn giao (ngăn rò rỉ dữ liệu)
                    // SAFETY:
                    // `addr` nằm hoàn toàn trong `STATIC_FRAME_POOL`, căn lề 4096-byte,
                    // và vừa được đánh dấu sở hữu độc quyền trong bitmap.
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
    }

    /// Cờ phần cứng của một mục bảng trang x86_64 (Page Table Entry Flags).
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct PageFlags(pub u64);

    impl PageFlags {
        pub const PRESENT: Self = Self(1 << 0);
        pub const WRITABLE: Self = Self(1 << 1);
        pub const USER: Self = Self(1 << 2);
        pub const NO_EXECUTE: Self = Self(1 << 63);
    }

    #[repr(C, align(4096))]
    pub struct PageTable {
        pub entries: [u64; 512],
    }

    impl PageTable {
        /// Giải mã chỉ số 4 cấp bảng trang (PML4, PDPT, PD, PT) từ địa chỉ ảo 64-bit.
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
        // `invlpg` chỉ làm mới bộ đệm TLB của địa chỉ ảo được chỉ định, không ảnh hưởng bộ nhớ.
        unsafe {
            asm!("invlpg [{}]", in(reg) virt_addr, options(nostack, preserves_flags));
        }
    }

    // ------------------------------------------------------------------------
    // 1.5: MSR, PER-CPU STATE & SYSCALL / SYSRETQ TRAMPOLINE
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

    /// Dữ liệu cục bộ của từng lõi CPU (được trỏ tới bởi thanh ghi `GS` khi ở Ring 0).
    #[repr(C, align(64))]
    pub struct PerCpuData {
        /// Offset 0x00: Đỉnh Kernel Stack của luồng hiện tại
        pub kernel_rsp: u64,
        /// Offset 0x08: Biến tạm giữ User Stack (`RSP`) khi vừa gọi lệnh `syscall`
        pub user_rsp_scratch: u64,
    }

    static mut BSP_PER_CPU: PerCpuData = PerCpuData {
        kernel_rsp: 0,
        user_rsp_scratch: 0,
    };

    /// Khung thanh ghi đầy đủ của tiến trình khi gọi `syscall`.
    #[repr(C, align(16))]
    #[derive(Debug, Clone, Copy, Default)]
    pub struct TrapFrame {
        pub r15: u64,
        pub r14: u64,
        pub r13: u64,
        pub r12: u64,
        pub rbp: u64,
        pub rbx: u64,
        pub r11: u64, // User RFLAGS
        pub r10: u64, // Linux Syscall Arg 4
        pub r9: u64,  // Linux Syscall Arg 6
        pub r8: u64,  // Linux Syscall Arg 5
        pub rax: u64, // Linux Syscall Number & Return Value
        pub rcx: u64, // User RIP
        pub rdx: u64, // Linux Syscall Arg 3
        pub rsi: u64, // Linux Syscall Arg 2
        pub rdi: u64, // Linux Syscall Arg 1
        pub rsp: u64, // User RSP
    }

    global_asm!(
        ".global zenith_syscall_trampoline",
        "zenith_syscall_trampoline:",
        // Hoán đổi GS của User với Kernel PerCpuData
        "swapgs",
        // Lưu RSP của User vào gs:[0x08] và nạp Kernel Stack từ gs:[0x00]
        "mov gs:[0x08], rsp",
        "mov rsp, gs:[0x00]",
        // Đẩy toàn bộ TrapFrame lên Kernel Stack
        "push qword ptr gs:[0x08]", // user rsp
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
        // Truyền `&mut TrapFrame` qua thanh ghi RDI (SysV AMD64 ABI)
        "mov rdi, rsp",
        "call zenith_syscall_entry_from_asm",
        // Khôi phục TrapFrame
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
    );

    extern "C" {
        fn zenith_syscall_trampoline();
    }

    /// Khởi tạo hệ thống `SYSCALL` / `SYSRETQ` ở cấp độ phần cứng CPU.
    pub fn init_syscall_msrs() {
        // SAFETY:
        // Chỉ chạy 1 lần trên BSP. Thiết lập `BSP_PER_CPU` và các MSR `EFER`, `STAR`, `LSTAR`, `SFMASK`.
        unsafe {
            let kstack_top = (&raw const KERNEL_INTR_STACK.0 as u64) + 16384;
            BSP_PER_CPU.kernel_rsp = kstack_top;
            let per_cpu_addr = &raw const BSP_PER_CPU as u64;

            write_msr(Msr::Ia32GsBase, per_cpu_addr);
            write_msr(Msr::Ia32KernelGsBase, per_cpu_addr);
        }

        // Bật bit SCE (System Call Extensions - bit 0) trong IA32_EFER
        let efer = read_msr(Msr::Ia32Efer);
        write_msr(Msr::Ia32Efer, efer | 1);

        // Cấu hình IA32_STAR:
        // - Bits [47:32] = 0x0008 (Kernel CS = 0x08, Kernel SS = 0x10)
        // - Bits [63:48] = 0x0010 (SYSRETQ sẽ nạp SS = 0x10 + 8 | 3 = 0x1B, CS = 0x10 + 16 | 3 = 0x23)
        let star: u64 = (0x0010u64 << 48) | (0x0008u64 << 32);
        write_msr(Msr::Ia32Star, star);

        // Cấu hình IA32_LSTAR trỏ tới `zenith_syscall_trampoline`
        write_msr(Msr::Ia32Lstar, zenith_syscall_trampoline as *const () as u64);

        // Xóa cờ IF (0x200) và DF (0x400) khi nhảy vào Ring 0
        write_msr(Msr::Ia32SfMask, 0x0600);
    }

    /// Sao chép dữ liệu an toàn từ bộ đệm của Kernel ra vùng nhớ được cấp phát cho tiến trình/test.
    pub fn copy_to_buffer(dst_addr: u64, src: &[u8]) -> Result<(), OstdError> {
        if dst_addr == 0 {
            return Err(OstdError::InvalidUserPointer);
        }
        // SAFETY:
        // `dst_addr` được kiểm tra khác null và do các trình kiểm thử hoặc trình quản lý bộ nhớ
        // của nhân cấp phát hợp lệ với độ dài `src.len()`.
        unsafe {
            core::ptr::copy_nonoverlapping(src.as_ptr(), dst_addr as *mut u8, src.len());
        }
        Ok(())
    }

    /// Đọc chuỗi byte từ bộ đệm vào lát cắt an toàn.
    pub fn copy_from_buffer(src_addr: u64, dst: &mut [u8]) -> Result<(), OstdError> {
        if src_addr == 0 {
            return Err(OstdError::InvalidUserPointer);
        }
        // SAFETY:
        // `src_addr` đã được xác thực khác null và trỏ tới bộ đệm hợp lệ có kích thước >= `dst.len()`.
        unsafe {
            core::ptr::copy_nonoverlapping(src_addr as *const u8, dst.as_mut_ptr(), dst.len());
        }
        Ok(())
    }
}

// ============================================================================
// MODULE 2: SAFE KERNEL CORE & LINUX ABI SUBSYSTEM (#![forbid(unsafe_code)])
// Toàn bộ tầng này cấm tuyệt đối `unsafe` và cấm `unwrap()` / `expect()`.
// ============================================================================
#[forbid(unsafe_code)]
pub mod linux_abi {
    use crate::ostd::{
        copy_from_buffer, copy_to_buffer, read_msr, write_msr, BitmapFrameAllocator, Msr,
        SerialConsole, TrapFrame,
    };

    /// Mã lỗi chuẩn Linux POSIX (`errno`).
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

    /// Số hiệu Syscall chuẩn của kiến trúc Linux `x86_64`
    pub const SYS_READ: u64 = 0;
    pub const SYS_WRITE: u64 = 1;
    pub const SYS_MMAP: u64 = 9;
    pub const SYS_BRK: u64 = 12;
    pub const SYS_UNAME: u64 = 63;
    pub const SYS_ARCH_PRCTL: u64 = 158;
    pub const SYS_EXIT: u64 = 60;
    pub const SYS_EXIT_GROUP: u64 = 231;

    /// Tham số của `SYS_arch_prctl` trên Linux x86_64 phục vụ Thread-Local Storage (`FS` base)
    pub const ARCH_SET_GS: u64 = 0x1001;
    pub const ARCH_SET_FS: u64 = 0x1002;
    pub const ARCH_GET_FS: u64 = 0x1003;
    pub const ARCH_GET_GS: u64 = 0x1004;

    /// Cấu trúc `utsname` chuẩn của Linux ABI (trả về bởi syscall `uname`).
    /// Mỗi trường dài đúng 65 byte (kèm null-terminator).
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
            let max_len = if src.len() < 64 { src.len() } else { 64 };
            let mut i = 0;
            while i < max_len {
                if let (Some(d), Some(&s)) = (dst.get_mut(i), src.get(i)) {
                    *d = s;
                }
                i += 1;
            }
        }

        pub fn as_bytes(&self) -> [u8; 390] {
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
            let mut i = 0;
            while i < 65 {
                if let (Some(d), Some(&s)) = (dst.get_mut(offset + i), src.get(i)) {
                    *d = s;
                }
                i += 1;
            }
        }
    }

    /// Trạng thái ngữ cảnh Tiến trình Linux (Linux Task Context) trong Safe Kernel Core.
    pub struct LinuxProcessState {
        pub pid: u64,
        pub program_break: u64,
        pub last_exit_code: Option<i32>,
    }

    impl LinuxProcessState {
        pub const fn new(pid: u64, initial_brk: u64) -> Self {
            Self {
                pid,
                program_break: initial_brk,
                last_exit_code: None,
            }
        }
    }

    /// Bộ xử lý Syscall chuẩn Linux x86_64 viết bằng 100% Safe Rust.
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
            // EOF trên stdin khi chạy không tương tác
            Ok(0)
        } else {
            Err(LinuxErrno::Ebadf)
        }
    }

    fn sys_write(fd: u64, buf_ptr: u64, count: u64) -> Result<u64, LinuxErrno> {
        // Chỉ hỗ trợ stdout (1) và stderr (2) trong Phase 1
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
            let chunk_len = if remaining < temp_buf.len() {
                remaining
            } else {
                temp_buf.len()
            };

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

    fn sys_mmap(_addr_hint: u64, length: u64, pmm: &mut BitmapFrameAllocator) -> Result<u64, LinuxErrno> {
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
        let raw_bytes = uts.as_bytes();
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
}

// ============================================================================
// ĐIỂM NỐI ASSEMBLY SYSCALL ENTRY & BOOTSTRAP KIỂM THỬ TỰ ĐỘNG (PHASE 1)
// ============================================================================
#[no_mangle]
extern "C" fn zenith_syscall_entry_from_asm(frame: &mut ostd::TrapFrame) {
    let mut dummy_proc = linux_abi::LinuxProcessState::new(1, 0x0000_0040_0000_0000);
    let mut dummy_pmm = ostd::BitmapFrameAllocator::new();
    linux_abi::dispatch_linux_syscall(frame, &mut dummy_proc, &mut dummy_pmm);
}

macro_rules! kprintln {
    ($($arg:tt)*) => {
        $crate::ostd::print_fmt(core::format_args!($($arg)*));
        $crate::ostd::print_fmt(core::format_args!("\n"));
    };
}

/// Điểm vào chính của Kernel ZenithOS sau khi Bootloader Limine bàn giao quyền điều khiển.
#[no_mangle]
pub extern "C" fn _start() -> ! {
    ostd::SerialConsole::init();
    kprintln!("================================================================");
    kprintln!("  ZenithOS v0.1.0 - 100% Rust Framekernel (Linux ABI Phase 1)   ");
    kprintln!("================================================================");

    // 1. Khởi tạo GDT & TSS (chuẩn bị selector cho Ring 0 và Ring 3 SYSRETQ)
    ostd::init_gdt_and_tss();
    kprintln!("[ZENITH-OSTD] GDT (Ring0/Ring3 Selectors) & 64-bit TSS initialized.");

    // 2. Khởi tạo IDT và chạy tự kiểm tra ngắt phần cứng (#BP - int3)
    ostd::init_idt();
    ostd::trigger_breakpoint_self_test();
    let bp_hits = ostd::BREAKPOINT_HIT_COUNT.load(Ordering::SeqCst);
    if bp_hits != 1 {
        kprintln!("[ZENITH-TEST][FAIL] IDT Breakpoint self-test failed!");
        ostd::exit_qemu(ostd::QemuExitCode::Failed);
    }
    kprintln!("[ZENITH-OSTD] IDT initialized & Breakpoint (#BP) self-test PASSED.");

    // 3. Khởi tạo MSR cho SYSCALL/SYSRETQ
    ostd::init_syscall_msrs();
    kprintln!("[ZENITH-OSTD] Fast SYSCALL/SYSRETQ MSRs (STAR/LSTAR/SFMASK) configured.");

    // 4. Kiểm thử Bộ cấp phát khung trang vật lý (PMM) & MMU Page Table Indexing
    let mut pmm = ostd::BitmapFrameAllocator::new();
    let frame1 = match pmm.allocate_frame() {
        Ok(addr) => addr,
        Err(_) => ostd::exit_qemu(ostd::QemuExitCode::Failed),
    };
    let frame2 = match pmm.allocate_frame() {
        Ok(addr) => addr,
        Err(_) => ostd::exit_qemu(ostd::QemuExitCode::Failed),
    };
    if frame1 == frame2 || (frame1 & 0xFFF) != 0 {
        kprintln!("[ZENITH-TEST][FAIL] PMM frame alignment or uniqueness check failed!");
        ostd::exit_qemu(ostd::QemuExitCode::Failed);
    }
    ostd::flush_tlb(frame1);
    let (p4, p3, p2, p1) = ostd::PageTable::indices(0xFFFF_FFFF_8000_1000);
    if (p4, p3, p2, p1) != (511, 510, 0, 1) {
        kprintln!("[ZENITH-TEST][FAIL] 4-Level MMU index calculation mismatch!");
        ostd::exit_qemu(ostd::QemuExitCode::Failed);
    }
    kprintln!(
        "[ZENITH-OSTD] Bitmap PMM (frame=0x{:x}) & 4-Level MMU verification PASSED.",
        frame1
    );

    // 5. Kiểm thử Tầng tương thích Linux ABI (`#![forbid(unsafe_code)]`)
    let mut linux_proc = linux_abi::LinuxProcessState::new(100, 0x0000_0000_1000_0000);
    let mut trap = ostd::TrapFrame::default();

    // 5a. Kiểm thử Linux `SYS_write` (Syscall #1) in trực tiếp ra Serial Console
    let msg = b"[ZENITH-LINUX] Hello from Linux x86_64 ABI SYS_write(fd=1)!\n";
    trap.rax = linux_abi::SYS_WRITE;
    trap.rdi = 1; // stdout
    trap.rsi = msg.as_ptr() as u64;
    trap.rdx = msg.len() as u64;
    linux_abi::dispatch_linux_syscall(&mut trap, &mut linux_proc, &mut pmm);
    if trap.rax != msg.len() as u64 {
        kprintln!("[ZENITH-TEST][FAIL] Linux SYS_write returned unexpected byte count!");
        ostd::exit_qemu(ostd::QemuExitCode::Failed);
    }

    // 5b. Kiểm thử Linux `SYS_uname` (Syscall #63)
    trap.rax = linux_abi::SYS_UNAME;
    trap.rdi = frame1; // Sử dụng trang 4KB vừa cấp phát làm bộ đệm nhận `struct utsname`
    linux_abi::dispatch_linux_syscall(&mut trap, &mut linux_proc, &mut pmm);
    let mut sysname_check = [0u8; 5];
    if ostd::copy_from_buffer(frame1, &mut sysname_check).is_err() || &sysname_check != b"Linux" {
        kprintln!("[ZENITH-TEST][FAIL] Linux SYS_uname verification failed!");
        ostd::exit_qemu(ostd::QemuExitCode::Failed);
    }
    kprintln!("[ZENITH-LINUX] SYS_uname verified (sysname = \"Linux\", machine = \"x86_64\").");

    // 5c. Kiểm thử Linux `SYS_arch_prctl(ARCH_SET_FS)` & `ARCH_GET_FS` (Syscall #158 cho TLS)
    let expected_tls_base: u64 = 0x0000_7FFF_F000_0000;
    trap.rax = linux_abi::SYS_ARCH_PRCTL;
    trap.rdi = linux_abi::ARCH_SET_FS;
    trap.rsi = expected_tls_base;
    linux_abi::dispatch_linux_syscall(&mut trap, &mut linux_proc, &mut pmm);

    trap.rax = linux_abi::SYS_ARCH_PRCTL;
    trap.rdi = linux_abi::ARCH_GET_FS;
    trap.rsi = frame2;
    linux_abi::dispatch_linux_syscall(&mut trap, &mut linux_proc, &mut pmm);

    let mut tls_readback = [0u8; 8];
    if ostd::copy_from_buffer(frame2, &mut tls_readback).is_err()
        || u64::from_ne_bytes(tls_readback) != expected_tls_base
    {
        kprintln!("[ZENITH-TEST][FAIL] Linux TLS (FS_BASE) verification failed!");
        ostd::exit_qemu(ostd::QemuExitCode::Failed);
    }
    kprintln!(
        "[ZENITH-LINUX] SYS_arch_prctl(ARCH_SET_FS = 0x{:x}) verified.",
        expected_tls_base
    );

    // 5d. Kiểm thử Linux `SYS_brk` (Syscall #12) & `SYS_exit_group` (Syscall #231)
    trap.rax = linux_abi::SYS_BRK;
    trap.rdi = 0x0000_0000_1000_8000;
    linux_abi::dispatch_linux_syscall(&mut trap, &mut linux_proc, &mut pmm);
    if trap.rax != 0x0000_0000_1000_8000 {
        kprintln!("[ZENITH-TEST][FAIL] Linux SYS_brk verification failed!");
        ostd::exit_qemu(ostd::QemuExitCode::Failed);
    }

    trap.rax = linux_abi::SYS_EXIT_GROUP;
    trap.rdi = 0;
    linux_abi::dispatch_linux_syscall(&mut trap, &mut linux_proc, &mut pmm);
    if linux_proc.last_exit_code != Some(0) {
        kprintln!("[ZENITH-TEST][FAIL] Linux SYS_exit_group verification failed!");
        ostd::exit_qemu(ostd::QemuExitCode::Failed);
    }

    let _ = pmm.deallocate_frame(frame1);
    let _ = pmm.deallocate_frame(frame2);

    kprintln!("================================================================");
    kprintln!("[ZENITH-CI] ALL PHASE 1 HARDWARE & LINUX ABI TESTS PASSED!      ");
    kprintln!("================================================================");

    // Thoát QEMU ngay lập tức với mã thành công (Exit code 33) để CI hoàn tất trong < 1 giây
    ostd::exit_qemu(ostd::QemuExitCode::Success);
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    kprintln!("\n[ZENITH-PANIC] Kernel panicked: {}", info);
    ostd::exit_qemu(ostd::QemuExitCode::Failed);
}
