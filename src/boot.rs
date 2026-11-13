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
pub struct LimineFramebuffer {
    pub address: *mut u8,
    pub width: u64,
    pub height: u64,
    pub pitch: u64,
    pub bpp: u16,
    pub memory_model: u8,
    pub red_mask_size: u8,
    pub red_mask_shift: u8,
    pub green_mask_size: u8,
    pub green_mask_shift: u8,
    pub blue_mask_size: u8,
    pub blue_mask_shift: u8,
    pub unused: [u8; 7],
    pub edid_size: u64,
    pub edid: *const u8,
    pub mode_count: u64,
    pub modes: *const *const (),
}

#[repr(C)]
pub struct LimineFramebufferResponse {
    pub revision: u64,
    pub framebuffer_count: u64,
    pub framebuffers: *const *const LimineFramebuffer,
}

#[repr(C)]
pub struct LimineFramebufferRequest {
    pub id: [u64; 4],
    pub revision: u64,
    pub response: *const LimineFramebufferResponse,
}

#[repr(C)]
pub struct LimineBootRequests {
    pub start_marker: [u64; 4],
    pub base_revision: [u64; 3],
    pub hhdm: LimineHhdmRequest,
    pub kernel_addr: LimineKernelAddressRequest,
    pub framebuffer: LimineFramebufferRequest,
    pub end_marker: [u64; 2],
}

#[repr(transparent)]
pub struct BootRequestsCell(pub UnsafeCell<LimineBootRequests>);

// SAFETY:
// Populated by the Limine bootloader prior to kernel entry and read-only after initialization.
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
        framebuffer: LimineFramebufferRequest {
            id: [
                0xc7b1_dd30_df4c_8b88,
                0x0a82_e883_a194_f07b,
                0x9d58_27dc_d881_dd75,
                0xa314_8604_f6fa_b11b,
            ],
            revision: 0,
            response: core::ptr::null(),
        },
        end_marker: [0xadc0_e053_1bb1_0d03, 0x9572_709f_3176_4c62],
    }));
