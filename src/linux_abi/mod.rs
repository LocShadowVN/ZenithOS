#![forbid(unsafe_code)]

pub mod console;
pub mod elf;
pub mod vfs;
pub mod zenithfs;

pub use console::{
    ascii_to_glyph_8x8, draw_glyph_8x8, draw_stylized_zenith_logo, draw_text_line, init_fbcon,
    DesktopState, GlyphStyle,
};
pub use elf::{
    build_embedded_linux_app_elf64, build_embedded_linux_init_elf64, load_elf64_into_user_space,
    ElfLoadError, LoadedElfProcess,
};
pub use vfs::{
    read_cstr_from_user, resolve_vfs_path, CapObject, CapabilityEntry, LinuxInputEvent,
    PipeRingBuffer, VfsNodeKind, ZCapRights, MAX_FDS, MAX_PIPES, OS_RELEASE_CONTENT, PIPE_BUF_SIZE,
    PROC_VERSION_CONTENT,
};
pub use zenithfs::{
    ZenithFsVolume, APT_SOURCES_DATA, DEBIAN_VERSION_DATA, DPKG_STATUS_DATA, PROC_FILESYSTEMS_DATA,
    ZENITHFS_SUPER_MAGIC, ZFS_BLOCK_SIZE,
};

use crate::ostd::{
    copy_from_buffer, copy_to_buffer, fb_draw_pixel, map_user_page, poll_ps2_scancode,
    read_cpu_brand, read_msr, read_tsc, scan_pci_bus, write_msr, BitmapFrameAllocator,
    FramebufferInfo, Msr, PciDeviceInfo, RtcDateTime, SerialConsole, TrapFrame, PAGE_SIZE,
};
