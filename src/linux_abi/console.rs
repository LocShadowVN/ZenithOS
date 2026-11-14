#![forbid(unsafe_code)]

use crate::ostd::{fb_fill_rect, FramebufferInfo, PciDeviceInfo, Rect};

#[derive(Clone, Copy)]
pub struct GlyphStyle {
    pub x: u32,
    pub y: u32,
    pub scale: u32,
    pub color: u32,
}

pub const fn ascii_to_glyph_8x8(ch: u8) -> [u8; 8] {
    match ch {
        b'A' => [0x3C, 0x66, 0x66, 0x7E, 0x66, 0x66, 0x66, 0x00],
        b'B' => [0x7C, 0x66, 0x66, 0x7C, 0x66, 0x66, 0x7C, 0x00],
        b'C' => [0x3C, 0x66, 0x60, 0x60, 0x60, 0x66, 0x3C, 0x00],
        b'D' => [0x78, 0x6C, 0x66, 0x66, 0x66, 0x6C, 0x78, 0x00],
        b'E' => [0x7E, 0x60, 0x60, 0x7C, 0x60, 0x60, 0x7E, 0x00],
        b'F' => [0x7E, 0x60, 0x60, 0x7C, 0x60, 0x60, 0x60, 0x00],
        b'G' => [0x3C, 0x66, 0x60, 0x6E, 0x66, 0x66, 0x3C, 0x00],
        b'H' => [0x66, 0x66, 0x66, 0x7E, 0x66, 0x66, 0x66, 0x00],
        b'I' => [0x3C, 0x18, 0x18, 0x18, 0x18, 0x18, 0x3C, 0x00],
        b'J' => [0x1E, 0x0C, 0x0C, 0x0C, 0x6C, 0x6C, 0x38, 0x00],
        b'K' => [0x66, 0x6C, 0x78, 0x70, 0x78, 0x6C, 0x66, 0x00],
        b'L' => [0x60, 0x60, 0x60, 0x60, 0x60, 0x60, 0x7E, 0x00],
        b'M' => [0x63, 0x77, 0x7F, 0x6B, 0x63, 0x63, 0x63, 0x00],
        b'N' => [0x66, 0x76, 0x7E, 0x7E, 0x6E, 0x66, 0x66, 0x00],
        b'O' => [0x3C, 0x66, 0x66, 0x66, 0x66, 0x66, 0x3C, 0x00],
        b'P' => [0x7C, 0x66, 0x66, 0x7C, 0x60, 0x60, 0x60, 0x00],
        b'Q' => [0x3C, 0x66, 0x66, 0x66, 0x6A, 0x6C, 0x36, 0x00],
        b'R' => [0x7C, 0x66, 0x66, 0x7C, 0x6C, 0x66, 0x66, 0x00],
        b'S' => [0x3C, 0x66, 0x60, 0x3C, 0x06, 0x66, 0x3C, 0x00],
        b'T' => [0x7E, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x00],
        b'U' => [0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x3C, 0x00],
        b'V' => [0x66, 0x66, 0x66, 0x66, 0x66, 0x3C, 0x18, 0x00],
        b'W' => [0x63, 0x63, 0x63, 0x6B, 0x7F, 0x77, 0x63, 0x00],
        b'X' => [0x66, 0x66, 0x3C, 0x18, 0x3C, 0x66, 0x66, 0x00],
        b'Y' => [0x66, 0x66, 0x66, 0x3C, 0x18, 0x18, 0x18, 0x00],
        b'Z' => [0x7E, 0x06, 0x0C, 0x18, 0x30, 0x60, 0x7E, 0x00],
        b'a' => [0x00, 0x00, 0x3C, 0x06, 0x3E, 0x66, 0x3E, 0x00],
        b'b' => [0x60, 0x60, 0x7C, 0x66, 0x66, 0x66, 0x7C, 0x00],
        b'c' => [0x00, 0x00, 0x3C, 0x60, 0x60, 0x60, 0x3C, 0x00],
        b'd' => [0x06, 0x06, 0x3E, 0x66, 0x66, 0x66, 0x3E, 0x00],
        b'e' => [0x00, 0x00, 0x3C, 0x66, 0x7E, 0x60, 0x3C, 0x00],
        b'f' => [0x1C, 0x30, 0x7C, 0x30, 0x30, 0x30, 0x30, 0x00],
        b'g' => [0x00, 0x00, 0x3E, 0x66, 0x66, 0x3E, 0x06, 0x3C],
        b'h' => [0x60, 0x60, 0x7C, 0x66, 0x66, 0x66, 0x66, 0x00],
        b'i' => [0x18, 0x00, 0x38, 0x18, 0x18, 0x18, 0x3C, 0x00],
        b'j' => [0x06, 0x00, 0x0E, 0x06, 0x06, 0x66, 0x66, 0x3C],
        b'k' => [0x60, 0x60, 0x66, 0x6C, 0x78, 0x6C, 0x66, 0x00],
        b'l' => [0x38, 0x18, 0x18, 0x18, 0x18, 0x18, 0x3C, 0x00],
        b'm' => [0x00, 0x00, 0x66, 0x7F, 0x6B, 0x6B, 0x63, 0x00],
        b'n' => [0x00, 0x00, 0x7C, 0x66, 0x66, 0x66, 0x66, 0x00],
        b'o' => [0x00, 0x00, 0x3C, 0x66, 0x66, 0x66, 0x3C, 0x00],
        b'p' => [0x00, 0x00, 0x7C, 0x66, 0x66, 0x7C, 0x60, 0x60],
        b'q' => [0x00, 0x00, 0x3E, 0x66, 0x66, 0x3E, 0x06, 0x06],
        b'r' => [0x00, 0x00, 0x7C, 0x66, 0x60, 0x60, 0x60, 0x00],
        b's' => [0x00, 0x00, 0x3E, 0x60, 0x3C, 0x06, 0x7C, 0x00],
        b't' => [0x30, 0x30, 0x7C, 0x30, 0x30, 0x30, 0x1C, 0x00],
        b'u' => [0x00, 0x00, 0x66, 0x66, 0x66, 0x66, 0x3E, 0x00],
        b'v' => [0x00, 0x00, 0x66, 0x66, 0x66, 0x3C, 0x18, 0x00],
        b'w' => [0x00, 0x00, 0x63, 0x6B, 0x6B, 0x7F, 0x36, 0x00],
        b'x' => [0x00, 0x00, 0x66, 0x3C, 0x18, 0x3C, 0x66, 0x00],
        b'y' => [0x00, 0x00, 0x66, 0x66, 0x66, 0x3E, 0x06, 0x3C],
        b'z' => [0x00, 0x00, 0x7E, 0x0C, 0x18, 0x30, 0x7E, 0x00],
        b'0' => [0x3C, 0x66, 0x6E, 0x76, 0x66, 0x66, 0x3C, 0x00],
        b'1' => [0x18, 0x38, 0x18, 0x18, 0x18, 0x18, 0x7E, 0x00],
        b'2' => [0x3C, 0x66, 0x06, 0x0C, 0x18, 0x30, 0x7E, 0x00],
        b'3' => [0x3C, 0x66, 0x06, 0x1C, 0x06, 0x66, 0x3C, 0x00],
        b'4' => [0x0C, 0x1C, 0x3C, 0x6C, 0x7E, 0x0C, 0x0C, 0x00],
        b'5' => [0x7E, 0x60, 0x7C, 0x06, 0x06, 0x66, 0x3C, 0x00],
        b'6' => [0x3C, 0x60, 0x60, 0x7C, 0x66, 0x66, 0x3C, 0x00],
        b'7' => [0x7E, 0x06, 0x0C, 0x18, 0x30, 0x30, 0x30, 0x00],
        b'8' => [0x3C, 0x66, 0x66, 0x3C, 0x66, 0x66, 0x3C, 0x00],
        b'9' => [0x3C, 0x66, 0x66, 0x3E, 0x06, 0x06, 0x3C, 0x00],
        b'&' => [0x38, 0x6C, 0x38, 0x76, 0xDC, 0xCC, 0x76, 0x00],
        b'=' => [0x00, 0x00, 0x7E, 0x00, 0x7E, 0x00, 0x00, 0x00],
        b'+' => [0x00, 0x18, 0x18, 0x7E, 0x18, 0x18, 0x00, 0x00],
        b'"' => [0x66, 0x66, 0x24, 0x00, 0x00, 0x00, 0x00, 0x00],
        b'\'' => [0x18, 0x18, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00],
        b'[' => [0x3C, 0x30, 0x30, 0x30, 0x30, 0x30, 0x3C, 0x00],
        b']' => [0x3C, 0x0C, 0x0C, 0x0C, 0x0C, 0x0C, 0x3C, 0x00],
        b'(' => [0x0C, 0x18, 0x30, 0x30, 0x30, 0x18, 0x0C, 0x00],
        b')' => [0x30, 0x18, 0x0C, 0x0C, 0x0C, 0x18, 0x30, 0x00],
        b'/' => [0x06, 0x0C, 0x18, 0x30, 0x60, 0x40, 0x00, 0x00],
        b':' => [0x00, 0x18, 0x18, 0x00, 0x18, 0x18, 0x00, 0x00],
        b'-' => [0x00, 0x00, 0x00, 0x7E, 0x00, 0x00, 0x00, 0x00],
        b'_' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x7E, 0x00],
        b'!' => [0x18, 0x18, 0x18, 0x18, 0x18, 0x00, 0x18, 0x00],
        b'.' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x18, 0x18, 0x00],
        b',' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x18, 0x18, 0x30],
        b'>' => [0x60, 0x30, 0x18, 0x0C, 0x18, 0x30, 0x60, 0x00],
        b'<' => [0x0C, 0x18, 0x30, 0x60, 0x30, 0x18, 0x0C, 0x00],
        b'@' => [0x3C, 0x66, 0x6E, 0x6E, 0x60, 0x62, 0x3C, 0x00],
        b'#' => [0x36, 0x36, 0x7F, 0x36, 0x7F, 0x36, 0x36, 0x00],
        b'~' => [0x00, 0x32, 0x4C, 0x00, 0x00, 0x00, 0x00, 0x00],
        b' ' | b'\n' | b'\r' => [0x00; 8],
        _ => [0x3C, 0x42, 0x81, 0x81, 0x81, 0x42, 0x3C, 0x00],
    }
}

pub fn draw_glyph_8x8(fb: &FramebufferInfo, glyph: [u8; 8], style: GlyphStyle) {
    for (row_idx, &row_bits) in glyph.iter().enumerate() {
        for col_idx in 0..8u32 {
            if (row_bits & (1 << (7 - col_idx))) != 0 {
                fb_fill_rect(
                    fb,
                    Rect {
                        x: style.x + col_idx * style.scale,
                        y: style.y + (row_idx as u32) * style.scale,
                        w: style.scale,
                        h: style.scale,
                    },
                    style.color,
                );
            }
        }
    }
}

pub fn draw_text_line(fb: &FramebufferInfo, text: &[u8], x0: u32, y0: u32, color: u32) {
    for (i, &ch) in text.iter().enumerate() {
        draw_glyph_8x8(
            fb,
            ascii_to_glyph_8x8(ch),
            GlyphStyle {
                x: x0 + (i as u32) * 8,
                y: y0,
                scale: 1,
                color,
            },
        );
    }
}

/// Renders the stylized geometric ZenithOS "Z" emblem (3-layer precision vector-bitmap).
/// - Top horizontal blade: Fluent Ice Cyan (`#60CDFF`)
/// - Diagonal precision slash: Zenith Cobalt (`#0078D4`)
/// - Bottom horizontal blade: Electric Blue (`#3B82F6`)
pub fn draw_stylized_zenith_logo(fb: &FramebufferInfo, x0: u32, y0: u32, scale: u32) {
    let top_blade: [u16; 3] = [0b1111_1111_1111_1100, 0b0111_1111_1111_1110, 0b0011_1111_1111_1100];
    let diag_slash: [u16; 10] = [
        0b0000_0000_0111_1000,
        0b0000_0000_1111_0000,
        0b0000_0001_1110_0000,
        0b0000_0011_1100_0000,
        0b0000_0111_1000_0000,
        0b0000_1111_0000_0000,
        0b0001_1110_0000_0000,
        0b0011_1100_0000_0000,
        0b0111_1000_0000_0000,
        0b1111_0000_0000_0000,
    ];
    let bot_blade: [u16; 3] = [0b0011_1111_1111_1100, 0b0111_1111_1111_1110, 0b0011_1111_1111_1111];

    for (r, &bits) in top_blade.iter().enumerate() {
        for c in 0..16u32 {
            if (bits & (1 << (15 - c))) != 0 {
                fb_fill_rect(
                    fb,
                    Rect {
                        x: x0 + c * scale,
                        y: y0 + (r as u32) * scale,
                        w: scale,
                        h: scale,
                    },
                    0x0060_CDFF,
                );
            }
        }
    }

    for (r, &bits) in diag_slash.iter().enumerate() {
        for c in 0..16u32 {
            if (bits & (1 << (15 - c))) != 0 {
                fb_fill_rect(
                    fb,
                    Rect {
                        x: x0 + c * scale,
                        y: y0 + ((r + 3) as u32) * scale,
                        w: scale,
                        h: scale,
                    },
                    0x0000_78D4,
                );
            }
        }
    }

    for (r, &bits) in bot_blade.iter().enumerate() {
        for c in 0..16u32 {
            if (bits & (1 << (15 - c))) != 0 {
                fb_fill_rect(
                    fb,
                    Rect {
                        x: x0 + c * scale,
                        y: y0 + ((r + 13) as u32) * scale,
                        w: scale,
                        h: scale,
                    },
                    0x003B_82F6,
                );
            }
        }
    }
}

fn draw_win11_window(
    fb: &FramebufferInfo,
    rect: Rect,
    title: &[u8],
    client_bg: u32,
    active_accent: u32,
) {
    fb_fill_rect(
        fb,
        Rect {
            x: rect.x + 6,
            y: rect.y + 6,
            w: rect.w,
            h: rect.h,
        },
        0x0006_0911,
    );
    fb_fill_rect(fb, rect, 0x0038_3C45);
    fb_fill_rect(
        fb,
        Rect {
            x: rect.x + 1,
            y: rect.y + 1,
            w: rect.w.saturating_sub(2),
            h: 34,
        },
        0x001F_2128,
    );
    fb_fill_rect(
        fb,
        Rect {
            x: rect.x + 1,
            y: rect.y + 35,
            w: rect.w.saturating_sub(2),
            h: rect.h.saturating_sub(36),
        },
        client_bg,
    );

    let tab_w = ((title.len() as u32) * 8 + 28).min(rect.w.saturating_sub(120));
    fb_fill_rect(
        fb,
        Rect {
            x: rect.x + 10,
            y: rect.y + 6,
            w: tab_w,
            h: 29,
        },
        client_bg,
    );
    fb_fill_rect(
        fb,
        Rect {
            x: rect.x + 10,
            y: rect.y + 6,
            w: tab_w,
            h: 2,
        },
        active_accent,
    );

    draw_text_line(fb, title, rect.x + 22, rect.y + 16, 0x00F0_F4F8);

    let right_edge = rect.x + rect.w;
    if right_edge > 110 {
        fb_fill_rect(
            fb,
            Rect {
                x: right_edge - 96,
                y: rect.y + 17,
                w: 10,
                h: 1,
            },
            0x009C_A3AF,
        );
        fb_fill_rect(
            fb,
            Rect {
                x: right_edge - 64,
                y: rect.y + 13,
                w: 10,
                h: 9,
            },
            0x009C_A3AF,
        );
        fb_fill_rect(
            fb,
            Rect {
                x: right_edge - 63,
                y: rect.y + 14,
                w: 8,
                h: 7,
            },
            0x001F_2128,
        );
        draw_text_line(fb, b"X", right_edge - 30, rect.y + 14, 0x00E5_E7EB);
    }
}

const fn hex_nibble(n: u8) -> u8 {
    let v = n & 0x0F;
    if v < 10 {
        b'0' + v
    } else {
        b'a' + (v - 10)
    }
}

pub fn init_fbcon(fb: &FramebufferInfo, pci_devs: &[PciDeviceInfo; 8], pci_count: usize) {
    let taskbar_h = 44u32;
    let desktop_h = fb.height.saturating_sub(taskbar_h);

    // 1. Windows 11 Dark Bloom Layered Background
    let band1 = desktop_h / 3;
    let band2 = (desktop_h * 2) / 3;
    fb_fill_rect(
        fb,
        Rect {
            x: 0,
            y: 0,
            w: fb.width,
            h: band1,
        },
        0x000B_1528,
    );
    fb_fill_rect(
        fb,
        Rect {
            x: 0,
            y: band1,
            w: fb.width,
            h: band2 - band1,
        },
        0x0010_213E,
    );
    fb_fill_rect(
        fb,
        Rect {
            x: 0,
            y: band2,
            w: fb.width,
            h: desktop_h - band2,
        },
        0x0009_1224,
    );

    // Stylized "Z" Brand Watermark in center of desktop
    if fb.width >= 640 && fb.height >= 360 {
        draw_stylized_zenith_logo(fb, fb.width / 2 - 48, desktop_h / 2 - 48, 6);
    }

    // 2. Window 1 (Left): Zenith Terminal (Debian 13 Trixie amd64 Userland)
    let win1_w = ((fb.width * 58) / 100).saturating_sub(32).max(320);
    let win_h = desktop_h.saturating_sub(48).max(220);
    draw_win11_window(
        fb,
        Rect {
            x: 24,
            y: 24,
            w: win1_w,
            h: win_h,
        },
        b"root@zenith-os: ~ (Debian 13 Trixie amd64)",
        0x000C_0E14,
        0x0000_78D4,
    );

    // 3. Window 2 (Right): ZenithOS 1.0 Brand, ZenithFS & Hardware PCIe Device Manager
    let win2_x = 24 + win1_w + 16;
    let win2_w = fb.width.saturating_sub(win2_x + 24);
    if win2_w >= 200 {
        draw_win11_window(
            fb,
            Rect {
                x: win2_x,
                y: 24,
                w: win2_w,
                h: win_h,
            },
            b"ZenithOS 1.0 - Hardware & ZenithFS",
            0x0014_161F,
            0x0060_CDFF,
        );

        let cx = win2_x + 16;
        let cy = 72;

        // Stylized Z Brand Emblem + OS Identity Header inside Window 2
        draw_stylized_zenith_logo(fb, cx, cy, 2);
        draw_text_line(fb, b"ZenithOS 1.0 Framekernel", cx + 42, cy + 4, 0x00FFFFFF);
        draw_text_line(
            fb,
            b"Debian 13 (Trixie) amd64 ABI",
            cx + 42,
            cy + 20,
            0x0060_CDFF,
        );

        draw_text_line(
            fb,
            b"Local Disk (Z:) - ZENITH-TRIXIE (ZFS1)",
            cx,
            cy + 44,
            0x00F3_F4F6,
        );
        let bar_w = win2_w.saturating_sub(32).min(360);
        fb_fill_rect(
            fb,
            Rect {
                x: cx,
                y: cy + 60,
                w: bar_w,
                h: 8,
            },
            0x002B_2F3A,
        );
        fb_fill_rect(
            fb,
            Rect {
                x: cx,
                y: cy + 60,
                w: (bar_w * 38) / 100,
                h: 8,
            },
            0x0000_78D4,
        );

        draw_text_line(
            fb,
            b"PCIe Hardware Bus Controllers:",
            cx,
            cy + 80,
            0x0060_CDFF,
        );
        let max_show = pci_count.min(4);
        for (i, dev) in pci_devs.iter().take(max_show).enumerate() {
            let mut line = [b' '; 42];
            line[0..4].copy_from_slice(b"00:0");
            line[3] = hex_nibble(dev.slot);
            line[4] = b'.';
            line[5] = hex_nibble(dev.func);
            line[6] = b' ';
            line[7] = b'[';
            line[8] = hex_nibble((dev.vendor_id >> 12) as u8);
            line[9] = hex_nibble((dev.vendor_id >> 8) as u8);
            line[10] = hex_nibble((dev.vendor_id >> 4) as u8);
            line[11] = hex_nibble(dev.vendor_id as u8);
            line[12] = b':';
            line[13] = hex_nibble((dev.device_id >> 12) as u8);
            line[14] = hex_nibble((dev.device_id >> 8) as u8);
            line[15] = hex_nibble((dev.device_id >> 4) as u8);
            line[16] = hex_nibble(dev.device_id as u8);
            line[17] = b']';
            line[18] = b' ';
            let cname = dev.class_name();
            let clen = cname.len().min(23);
            line[19..19 + clen].copy_from_slice(&cname[..clen]);
            draw_text_line(fb, &line, cx, cy + 98 + (i as u32) * 16, 0x00D1_D5DB);
        }

        draw_text_line(
            fb,
            b"Installed Debian 13 (.deb) Packages:",
            cx,
            cy + 172,
            0x0060_CDFF,
        );
        draw_text_line(
            fb,
            b" [x] base-files (13.0+trixie1 amd64)",
            cx,
            cy + 190,
            0x009E_CE6A,
        );
        draw_text_line(
            fb,
            b" [x] hello-trixie (1.0 GCC ELF64 .deb)",
            cx,
            cy + 206,
            0x009E_CE6A,
        );
    }

    // 4. Windows 11 Bottom Taskbar with Stylized "Z" Start Button
    let tb_y = desktop_h;
    fb_fill_rect(
        fb,
        Rect {
            x: 0,
            y: tb_y,
            w: fb.width,
            h: taskbar_h,
        },
        0x0018_1A20,
    );
    fb_fill_rect(
        fb,
        Rect {
            x: 0,
            y: tb_y,
            w: fb.width,
            h: 1,
        },
        0x0032_3642,
    );

    let center_x = (fb.width / 2).saturating_sub(110);

    // Start Button Pill with Stylized Zenith "Z" Logo
    fb_fill_rect(
        fb,
        Rect {
            x: center_x - 4,
            y: tb_y + 8,
            w: 28,
            h: 28,
        },
        0x0025_2832,
    );
    draw_stylized_zenith_logo(fb, center_x + 2, tb_y + 14, 1);

    // Search Pill
    fb_fill_rect(
        fb,
        Rect {
            x: center_x + 32,
            y: tb_y + 9,
            w: 96,
            h: 26,
        },
        0x0025_2832,
    );
    draw_text_line(fb, b"Search", center_x + 48, tb_y + 18, 0x009C_A3AF);

    // Terminal App Icon
    let term_icon_x = center_x + 140;
    fb_fill_rect(
        fb,
        Rect {
            x: term_icon_x,
            y: tb_y + 9,
            w: 28,
            h: 24,
        },
        0x002B_2F3B,
    );
    draw_text_line(fb, b">_", term_icon_x + 6, tb_y + 17, 0x0060_CDFF);
    fb_fill_rect(
        fb,
        Rect {
            x: term_icon_x + 8,
            y: tb_y + 37,
            w: 12,
            h: 3,
        },
        0x0060_CDFF,
    );

    // Explorer Folder Icon
    let exp_icon_x = center_x + 180;
    fb_fill_rect(
        fb,
        Rect {
            x: exp_icon_x,
            y: tb_y + 11,
            w: 24,
            h: 18,
        },
        0x00F5_B942,
    );
    fb_fill_rect(
        fb,
        Rect {
            x: exp_icon_x + 6,
            y: tb_y + 37,
            w: 12,
            h: 3,
        },
        0x0060_CDFF,
    );

    if fb.width > 240 {
        draw_text_line(
            fb,
            b"PCIe  ZFS1  DEB13",
            fb.width - 156,
            tb_y + 18,
            0x00E5_E7EB,
        );
    }
}
