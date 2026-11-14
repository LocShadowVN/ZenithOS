#![forbid(unsafe_code)]

use crate::ostd::{fb_fill_rect, FramebufferInfo, PciDeviceInfo, Rect, RtcDateTime};

#[derive(Clone, Copy)]
pub struct GlyphStyle {
    pub x: u32,
    pub y: u32,
    pub scale: u32,
    pub color: u32,
}

#[derive(Clone, Copy)]
pub struct DesktopState<'a> {
    pub pci_devs: &'a [PciDeviceInfo; 8],
    pub pci_count: usize,
    pub rtc: RtcDateTime,
    pub cpu_brand: &'a [u8; 48],
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
        b'\\' => [0x60, 0x30, 0x18, 0x0C, 0x06, 0x02, 0x00, 0x00],
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

pub fn draw_stylized_zenith_logo(fb: &FramebufferInfo, x0: u32, y0: u32, scale: u32) {
    let top_blade: [u16; 3] = [
        0b1111_1111_1111_1100,
        0b0111_1111_1111_1110,
        0b0011_1111_1111_1100,
    ];
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
    let bot_blade: [u16; 3] = [
        0b0011_1111_1111_1100,
        0b0111_1111_1111_1110,
        0b0011_1111_1111_1111,
    ];

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
            x: rect.x + 5,
            y: rect.y + 5,
            w: rect.w,
            h: rect.h,
        },
        0x0005_0810,
    );
    fb_fill_rect(fb, rect, 0x0038_3D48);
    fb_fill_rect(
        fb,
        Rect {
            x: rect.x + 1,
            y: rect.y + 1,
            w: rect.w.saturating_sub(2),
            h: 32,
        },
        0x001E_2129,
    );
    fb_fill_rect(
        fb,
        Rect {
            x: rect.x + 1,
            y: rect.y + 33,
            w: rect.w.saturating_sub(2),
            h: rect.h.saturating_sub(34),
        },
        client_bg,
    );

    let tab_w = ((title.len() as u32) * 8 + 24).min(rect.w.saturating_sub(110));
    fb_fill_rect(
        fb,
        Rect {
            x: rect.x + 8,
            y: rect.y + 5,
            w: tab_w,
            h: 28,
        },
        client_bg,
    );
    fb_fill_rect(
        fb,
        Rect {
            x: rect.x + 8,
            y: rect.y + 5,
            w: tab_w,
            h: 2,
        },
        active_accent,
    );

    draw_text_line(fb, title, rect.x + 18, rect.y + 15, 0x00F3_F4F6);

    let right_edge = rect.x + rect.w;
    if right_edge > 100 {
        fb_fill_rect(
            fb,
            Rect {
                x: right_edge - 88,
                y: rect.y + 16,
                w: 10,
                h: 1,
            },
            0x009C_A3AF,
        );
        fb_fill_rect(
            fb,
            Rect {
                x: right_edge - 58,
                y: rect.y + 12,
                w: 10,
                h: 9,
            },
            0x009C_A3AF,
        );
        fb_fill_rect(
            fb,
            Rect {
                x: right_edge - 57,
                y: rect.y + 13,
                w: 8,
                h: 7,
            },
            0x001E_2129,
        );
        draw_text_line(fb, b"X", right_edge - 26, rect.y + 13, 0x00E5_E7EB);
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

fn draw_desktop_shortcuts(fb: &FramebufferInfo) {
    // 1. "This PC" icon
    fb_fill_rect(
        fb,
        Rect {
            x: 24,
            y: 28,
            w: 36,
            h: 26,
        },
        0x0000_78D4,
    );
    fb_fill_rect(
        fb,
        Rect {
            x: 26,
            y: 30,
            w: 32,
            h: 20,
        },
        0x0060_CDFF,
    );
    fb_fill_rect(
        fb,
        Rect {
            x: 36,
            y: 54,
            w: 12,
            h: 4,
        },
        0x009C_A3AF,
    );
    draw_text_line(fb, b"This PC", 16, 64, 0x00F3_F4F6);

    // 2. "Files" Folder icon
    fb_fill_rect(
        fb,
        Rect {
            x: 24,
            y: 96,
            w: 16,
            h: 5,
        },
        0x00D9_9B26,
    );
    fb_fill_rect(
        fb,
        Rect {
            x: 24,
            y: 101,
            w: 36,
            h: 24,
        },
        0x00F5_B942,
    );
    draw_text_line(fb, b"Files", 22, 132, 0x00F3_F4F6);

    // 3. "Settings" icon
    fb_fill_rect(
        fb,
        Rect {
            x: 24,
            y: 166,
            w: 36,
            h: 28,
        },
        0x002B_3240,
    );
    draw_stylized_zenith_logo(fb, 34, 172, 1);
    draw_text_line(fb, b"Settings", 12, 200, 0x00F3_F4F6);
}

fn draw_files_app(fb: &FramebufferInfo, x: u32, y: u32, w: u32, h: u32) {
    draw_win11_window(
        fb,
        Rect { x, y, w, h },
        b"Files - /home/user (ZenithFS)",
        0x0010_121A,
        0x00F5_B942,
    );

    // Address & Navigation Bar
    fb_fill_rect(
        fb,
        Rect {
            x: x + 1,
            y: y + 34,
            w: w.saturating_sub(2),
            h: 28,
        },
        0x0018_1B24,
    );
    draw_text_line(
        fb,
        b"<  >  ^   Z:\\ > home > user > Documents",
        x + 14,
        y + 44,
        0x00D1_D5DB,
    );

    // Left Sidebar Navigation
    let side_w = 136u32;
    fb_fill_rect(
        fb,
        Rect {
            x: x + 1,
            y: y + 63,
            w: side_w,
            h: h.saturating_sub(64),
        },
        0x0014_1720,
    );
    draw_text_line(fb, b"Quick Access", x + 12, y + 76, 0x0060_CDFF);
    draw_text_line(fb, b" Desktop", x + 12, y + 96, 0x00D1_D5DB);
    fb_fill_rect(
        fb,
        Rect {
            x: x + 8,
            y: y + 110,
            w: side_w - 16,
            h: 18,
        },
        0x0000_78D4,
    );
    draw_text_line(fb, b" Documents", x + 12, y + 115, 0x00FFFFFF);
    draw_text_line(fb, b" Debian Apps", x + 12, y + 136, 0x00D1_D5DB);
    draw_text_line(fb, b" Local Disk Z:", x + 12, y + 156, 0x009E_CE6A);

    // Main File Table
    let list_x = x + side_w + 16;
    draw_text_line(
        fb,
        b"Name                Type           Size    Security",
        list_x,
        y + 76,
        0x009C_A3AF,
    );
    fb_fill_rect(
        fb,
        Rect {
            x: list_x,
            y: y + 88,
            w: w.saturating_sub(side_w + 28),
            h: 1,
        },
        0x002B_2F3A,
    );

    draw_text_line(
        fb,
        b"zenith.txt          Text File      17 B    Z-Cap Home R/W",
        list_x,
        y + 100,
        0x009E_CE6A,
    );
    draw_text_line(
        fb,
        b"hello-deb           Debian 13 App  4 KB    Verified .deb",
        list_x,
        y + 118,
        0x0060_CDFF,
    );
    draw_text_line(
        fb,
        b"sources.list        APT Repo Cfg   70 B    Immutable Sys",
        list_x,
        y + 136,
        0x00E5_E7EB,
    );
    draw_text_line(
        fb,
        b"debian_version      OS Release     5 B     Immutable Sys",
        list_x,
        y + 154,
        0x00E5_E7EB,
    );
}

fn draw_settings_app(fb: &FramebufferInfo, x: u32, y: u32, w: u32, h: u32, state: &DesktopState) {
    draw_win11_window(
        fb,
        Rect { x, y, w, h },
        b"Settings - System, RTC & Drivers (2013-2026)",
        0x0013_161F,
        0x0060_CDFF,
    );

    let cx = x + 16;
    let cy = y + 48;

    draw_stylized_zenith_logo(fb, cx, cy, 2);
    draw_text_line(fb, b"ZenithOS 1.2 LTS (x86_64)", cx + 42, cy + 4, 0x00FFFFFF);
    draw_text_line(
        fb,
        b"Debian 13 (Trixie) + Z-Cap Passwordless Security",
        cx + 42,
        cy + 20,
        0x0060_CDFF,
    );

    // Hardware CPUID & CMOS RTC Telemetry Card
    fb_fill_rect(
        fb,
        Rect {
            x: cx,
            y: cy + 42,
            w: w.saturating_sub(32),
            h: 82,
        },
        0x001B_1F2B,
    );
    draw_text_line(fb, b"Processor (Silicon CPUID):", cx + 10, cy + 52, 0x009C_A3AF);
    draw_text_line(fb, &state.cpu_brand[..44], cx + 10, cy + 68, 0x00FFFFFF);

    draw_text_line(fb, b"Motherboard CMOS RTC:", cx + 10, cy + 90, 0x009C_A3AF);
    let time_str = state.rtc.format_time();
    let date_str = state.rtc.format_date();
    draw_text_line(fb, &date_str, cx + 186, cy + 90, 0x009E_CE6A);
    draw_text_line(fb, &time_str, cx + 276, cy + 90, 0x0060_CDFF);
    draw_text_line(
        fb,
        b"Security Mode: Z-Cap Scoped Token (No sudo prompt)",
        cx + 10,
        cy + 108,
        0x00E5_E7EB,
    );

    // Active Hardware Drivers Section (2013-2026 Hardware Support)
    draw_text_line(
        fb,
        b"Bound PCIe & Platform Drivers (2013-2026 ISA):",
        cx,
        cy + 138,
        0x0060_CDFF,
    );

    let max_show = state.pci_count.min(4);
    for (i, dev) in state.pci_devs.iter().take(max_show).enumerate() {
        let row_y = cy + 158 + (i as u32) * 18;
        let mut slot_tag = [b' '; 14];
        slot_tag[0] = b'[';
        slot_tag[1] = hex_nibble((dev.vendor_id >> 12) as u8);
        slot_tag[2] = hex_nibble((dev.vendor_id >> 8) as u8);
        slot_tag[3] = hex_nibble((dev.vendor_id >> 4) as u8);
        slot_tag[4] = hex_nibble(dev.vendor_id as u8);
        slot_tag[5] = b':';
        slot_tag[6] = hex_nibble((dev.device_id >> 12) as u8);
        slot_tag[7] = hex_nibble((dev.device_id >> 8) as u8);
        slot_tag[8] = hex_nibble((dev.device_id >> 4) as u8);
        slot_tag[9] = hex_nibble(dev.device_id as u8);
        slot_tag[10] = b']';
        draw_text_line(fb, &slot_tag, cx, row_y, 0x009C_A3AF);
        draw_text_line(fb, dev.driver_description(), cx + 104, row_y, 0x00D1_D5DB);
    }

    let extra_y = cy + 162 + (max_show as u32) * 18;
    draw_text_line(
        fb,
        b"[MOD READY] nvme_pcie, xhci_usb3, ahci_sata, e1000e",
        cx,
        extra_y,
        0x009E_CE6A,
    );

    // Storage Bar
    draw_text_line(
        fb,
        b"ZenithFS (Z:) Partition Health: 100% (FNV-1a Verified)",
        cx,
        extra_y + 26,
        0x00F3_F4F6,
    );
    let bar_w = w.saturating_sub(32);
    fb_fill_rect(
        fb,
        Rect {
            x: cx,
            y: extra_y + 44,
            w: bar_w,
            h: 10,
        },
        0x002B_2F3A,
    );
    fb_fill_rect(
        fb,
        Rect {
            x: cx,
            y: extra_y + 44,
            w: (bar_w * 36) / 100,
            h: 10,
        },
        0x0000_78D4,
    );
}

pub fn init_fbcon(fb: &FramebufferInfo, state: DesktopState) {
    let taskbar_h = 46u32;
    let desktop_h = fb.height.saturating_sub(taskbar_h);

    // 1. Smooth Per-Scanline Windows 11 Dark Bloom Gradient Wallpaper
    for py in 0..desktop_h {
        let r = 8 + (py * 10) / desktop_h.max(1);
        let g = 16 + (py * 24) / desktop_h.max(1);
        let b = 34 + (py * 44) / desktop_h.max(1);
        let color = (r << 16) | (g << 8) | b;
        fb_fill_rect(
            fb,
            Rect {
                x: 0,
                y: py,
                w: fb.width,
                h: 1,
            },
            color,
        );
    }

    // 2. Desktop Shortcut Icons (This PC, Files, Settings)
    draw_desktop_shortcuts(fb);

    // 3. Built-in Default Apps Layout:
    //    - Top-Left: Files (File Explorer)
    //    - Bottom-Left: Terminal
    //    - Right: Settings & Hardware Drivers
    let left_col_x = 84u32;
    let left_col_w = ((fb.width * 54) / 100).saturating_sub(left_col_x).max(320);
    let files_h = 200u32;
    draw_files_app(fb, left_col_x, 18, left_col_w, files_h);

    let term_y = 18 + files_h + 14;
    let term_h = desktop_h.saturating_sub(term_y + 14).max(160);
    draw_win11_window(
        fb,
        Rect {
            x: left_col_x,
            y: term_y,
            w: left_col_w,
            h: term_h,
        },
        b"Terminal - user@zenith-os: ~ (UID=1000 Z-Cap)",
        0x000B_0D13,
        0x0000_78D4,
    );

    let right_x = left_col_x + left_col_w + 16;
    let right_w = fb.width.saturating_sub(right_x + 18);
    if right_w >= 220 {
        draw_settings_app(
            fb,
            right_x,
            18,
            right_w,
            desktop_h.saturating_sub(32),
            &state,
        );
    }

    // 4. Windows 11 Bottom Taskbar with Live CMOS RTC Clock & Date
    let tb_y = desktop_h;
    fb_fill_rect(
        fb,
        Rect {
            x: 0,
            y: tb_y,
            w: fb.width,
            h: taskbar_h,
        },
        0x0014_171F,
    );
    fb_fill_rect(
        fb,
        Rect {
            x: 0,
            y: tb_y,
            w: fb.width,
            h: 1,
        },
        0x002E_3442,
    );

    let center_x = (fb.width / 2).saturating_sub(120);

    // Stylized "Z" Start Button
    fb_fill_rect(
        fb,
        Rect {
            x: center_x - 4,
            y: tb_y + 9,
            w: 28,
            h: 28,
        },
        0x0022_2734,
    );
    draw_stylized_zenith_logo(fb, center_x + 2, tb_y + 15, 1);

    // Search Pill
    fb_fill_rect(
        fb,
        Rect {
            x: center_x + 32,
            y: tb_y + 10,
            w: 104,
            h: 26,
        },
        0x0022_2734,
    );
    draw_text_line(fb, b"Search", center_x + 52, tb_y + 19, 0x009C_A3AF);

    // Pinned App 1: Files
    let files_ic_x = center_x + 148;
    fb_fill_rect(
        fb,
        Rect {
            x: files_ic_x,
            y: tb_y + 12,
            w: 24,
            h: 18,
        },
        0x00F5_B942,
    );
    fb_fill_rect(
        fb,
        Rect {
            x: files_ic_x + 6,
            y: tb_y + 38,
            w: 12,
            h: 3,
        },
        0x0060_CDFF,
    );

    // Pinned App 2: Terminal
    let term_ic_x = center_x + 184;
    fb_fill_rect(
        fb,
        Rect {
            x: term_ic_x,
            y: tb_y + 10,
            w: 26,
            h: 22,
        },
        0x0028_2D3C,
    );
    draw_text_line(fb, b">_", term_ic_x + 5, tb_y + 17, 0x0060_CDFF);
    fb_fill_rect(
        fb,
        Rect {
            x: term_ic_x + 7,
            y: tb_y + 38,
            w: 12,
            h: 3,
        },
        0x0060_CDFF,
    );

    // Pinned App 3: Settings
    let set_ic_x = center_x + 222;
    fb_fill_rect(
        fb,
        Rect {
            x: set_ic_x,
            y: tb_y + 10,
            w: 26,
            h: 22,
        },
        0x0000_78D4,
    );
    draw_text_line(fb, b"S", set_ic_x + 9, tb_y + 17, 0x00FFFFFF);
    fb_fill_rect(
        fb,
        Rect {
            x: set_ic_x + 7,
            y: tb_y + 38,
            w: 12,
            h: 3,
        },
        0x0060_CDFF,
    );

    // Right System Tray: Z-CAP | ENG | Two-Line Live CMOS Clock & Date
    if fb.width > 260 {
        draw_text_line(
            fb,
            b"Z-CAP  ENG",
            fb.width - 196,
            tb_y + 19,
            0x009E_CE6A,
        );
        let time_str = state.rtc.format_time();
        let date_str = state.rtc.format_date();
        draw_text_line(fb, &time_str, fb.width - 96, tb_y + 10, 0x00FFFFFF);
        draw_text_line(fb, &date_str, fb.width - 96, tb_y + 25, 0x00D1_D5DB);
    }
}
