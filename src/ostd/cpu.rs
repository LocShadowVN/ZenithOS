use core::arch::{asm, global_asm};
use core::fmt::Write;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::ostd::{exit_qemu, QemuExitCode, SerialConsole};

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

pub static mut KERNEL_INTR_STACK: AlignedStack = AlignedStack([0; 16384]);
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
        0x0000_0000_0000_0000, // 0x00: Null descriptor
        0x00AF_9A00_0000_FFFF, // 0x08: Kernel code 64-bit (DPL=0)
        0x00CF_9200_0000_FFFF, // 0x10: Kernel data 64-bit (DPL=0)
        0x00CF_F200_0000_FFFF, // 0x18: User data 64-bit   (DPL=3 -> 0x1B)
        0x00AF_FA00_0000_FFFF, // 0x20: User code 64-bit   (DPL=3 -> 0x23)
        0x0000_0000_0000_0000, // 0x28: TSS descriptor low
        0x0000_0000_0000_0000, // 0x30: TSS descriptor high
    ],
};

#[repr(C, packed)]
struct DescriptorTablePointer {
    limit: u16,
    base: u64,
}

pub fn init_gdt_and_tss() {
    // SAFETY:
    // Executed once on the BSP during single-threaded boot initialization.
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
    // Reads CR2 register to report the faulting linear address on Page Fault (#PF).
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
    // Registers architectural exception gates in the IDT on the BSP.
    unsafe {
        let idt_ptr = &raw mut IDT;
        (*idt_ptr).entries[3] = IdtEntry::new_handler(isr_breakpoint_stub as *const () as u64, 0);
        (*idt_ptr).entries[8] = IdtEntry::new_handler(isr_double_fault_stub as *const () as u64, 1);
        (*idt_ptr).entries[13] = IdtEntry::new_handler(isr_gp_fault_stub as *const () as u64, 0);
        (*idt_ptr).entries[14] = IdtEntry::new_handler(isr_page_fault_stub as *const () as u64, 0);

        let idtr = DescriptorTablePointer {
            limit: (core::mem::size_of::<IdtTable>() - 1) as u16,
            base: idt_ptr as u64,
        };

        asm!("lidt [{}]", in(reg) &idtr, options(nostack, preserves_flags));
    }
}

pub fn trigger_breakpoint_self_test() {
    // SAFETY:
    // Invokes software breakpoint vector #3 registered in `init_idt`.
    unsafe {
        asm!("int3", options(nostack, preserves_flags));
    }
}
