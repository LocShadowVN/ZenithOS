const FALLBACK_FB_W: usize = 640;
const FALLBACK_FB_H: usize = 360;
static mut FALLBACK_FB_CANVAS: [u32; FALLBACK_FB_W * FALLBACK_FB_H] =
    [0; FALLBACK_FB_W * FALLBACK_FB_H];

#[derive(Debug, Clone, Copy)]
pub struct FramebufferInfo {
    pub addr: u64,
    pub width: u32,
    pub height: u32,
    pub pitch: u32,
    pub bpp: u16,
    pub is_hardware: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl FramebufferInfo {
    pub const fn const_uninit() -> Self {
        Self {
            addr: 0,
            width: FALLBACK_FB_W as u32,
            height: FALLBACK_FB_H as u32,
            pitch: (FALLBACK_FB_W * 4) as u32,
            bpp: 32,
            is_hardware: false,
        }
    }

    pub fn detect_from_limine() -> Self {
        // SAFETY:
        // Reads the Limine framebuffer response pointer using volatile memory semantics.
        unsafe {
            let req_ptr = crate::boot::LIMINE_REQUESTS.0.get();
            let fb_resp = core::ptr::read_volatile(&(*req_ptr).framebuffer.response);
            if !fb_resp.is_null() {
                let count = core::ptr::read_volatile(&(*fb_resp).framebuffer_count);
                let fbs_ptr = core::ptr::read_volatile(&(*fb_resp).framebuffers);
                if count > 0 && !fbs_ptr.is_null() {
                    let fb0 = core::ptr::read_volatile(fbs_ptr);
                    if !fb0.is_null() {
                        let addr = core::ptr::read_volatile(&(*fb0).address) as u64;
                        let width = core::ptr::read_volatile(&(*fb0).width) as u32;
                        let height = core::ptr::read_volatile(&(*fb0).height) as u32;
                        let pitch = core::ptr::read_volatile(&(*fb0).pitch) as u32;
                        let bpp = core::ptr::read_volatile(&(*fb0).bpp);
                        if addr != 0 && width > 0 && height > 0 && bpp == 32 {
                            return Self {
                                addr,
                                width,
                                height,
                                pitch,
                                bpp,
                                is_hardware: true,
                            };
                        }
                    }
                }
            }
        }

        Self {
            addr: &raw const FALLBACK_FB_CANVAS as u64,
            width: FALLBACK_FB_W as u32,
            height: FALLBACK_FB_H as u32,
            pitch: (FALLBACK_FB_W * 4) as u32,
            bpp: 32,
            is_hardware: false,
        }
    }
}

#[inline]
pub fn fb_draw_pixel(fb: &FramebufferInfo, x: u32, y: u32, color: u32) {
    if fb.addr == 0 || x >= fb.width || y >= fb.height {
        return;
    }
    let byte_offset = (y as u64) * (fb.pitch as u64) + (x as u64) * 4;
    let pixel_ptr = (fb.addr + byte_offset) as *mut u32;
    // SAFETY:
    // Coordinates `(x, y)` are bounds-checked against `(fb.width, fb.height)`.
    unsafe {
        core::ptr::write_volatile(pixel_ptr, color);
    }
}

pub fn fb_read_pixel(fb: &FramebufferInfo, x: u32, y: u32) -> u32 {
    if fb.addr == 0 || x >= fb.width || y >= fb.height {
        return 0;
    }
    let byte_offset = (y as u64) * (fb.pitch as u64) + (x as u64) * 4;
    let pixel_ptr = (fb.addr + byte_offset) as *const u32;
    // SAFETY:
    // Coordinates `(x, y)` are bounds-checked against `(fb.width, fb.height)`.
    unsafe { core::ptr::read_volatile(pixel_ptr) }
}

pub fn fb_fill_rect(fb: &FramebufferInfo, rect: Rect, color: u32) {
    let max_x = (rect.x + rect.w).min(fb.width);
    let max_y = (rect.y + rect.h).min(fb.height);
    let mut py = rect.y;
    while py < max_y {
        let mut px = rect.x;
        while px < max_x {
            fb_draw_pixel(fb, px, py, color);
            px += 1;
        }
        py += 1;
    }
}
