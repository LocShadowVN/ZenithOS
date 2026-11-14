use core::arch::asm;

use crate::ostd::{inb, inl, outb, outl};

#[derive(Debug, Clone, Copy)]
pub struct RtcDateTime {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

impl RtcDateTime {
    pub const fn const_default() -> Self {
        Self {
            year: 2026,
            month: 11,
            day: 14,
            hour: 8,
            minute: 0,
            second: 0,
        }
    }

    #[inline]
    fn read_cmos_reg(reg: u8) -> u8 {
        outb(0x70, reg);
        inb(0x71)
    }

    #[inline]
    const fn bcd_to_bin(val: u8) -> u8 {
        (val & 0x0F) + ((val / 16) * 10)
    }

    pub fn read_from_cmos() -> Self {
        let mut spins = 0;
        while (Self::read_cmos_reg(0x0A) & 0x80) != 0 && spins < 10_000 {
            core::hint::spin_loop();
            spins += 1;
        }

        let mut second = Self::read_cmos_reg(0x00);
        let mut minute = Self::read_cmos_reg(0x02);
        let mut hour = Self::read_cmos_reg(0x04);
        let mut day = Self::read_cmos_reg(0x07);
        let mut month = Self::read_cmos_reg(0x08);
        let mut year = Self::read_cmos_reg(0x09);
        let status_b = Self::read_cmos_reg(0x0B);

        if (status_b & 0x04) == 0 {
            second = Self::bcd_to_bin(second);
            minute = Self::bcd_to_bin(minute);
            hour = Self::bcd_to_bin(hour & 0x7F);
            day = Self::bcd_to_bin(day);
            month = Self::bcd_to_bin(month);
            year = Self::bcd_to_bin(year);
        }

        let full_year = 2000u16 + (year as u16);
        Self {
            year: if full_year < 2024 { 2026 } else { full_year },
            month: month.clamp(1, 12),
            day: day.clamp(1, 31),
            hour: hour % 24,
            minute: minute % 60,
            second: second % 60,
        }
    }

    /// Formats `HH:MM:SS` (8 bytes) for live clock updates.
    pub const fn format_time(&self) -> [u8; 8] {
        [
            b'0' + (self.hour / 10),
            b'0' + (self.hour % 10),
            b':',
            b'0' + (self.minute / 10),
            b'0' + (self.minute % 10),
            b':',
            b'0' + (self.second / 10),
            b'0' + (self.second % 10),
        ]
    }

    /// Formats `DD/MM/YYYY` (10 bytes).
    pub const fn format_date(&self) -> [u8; 10] {
        let y = self.year;
        [
            b'0' + (self.day / 10),
            b'0' + (self.day % 10),
            b'/',
            b'0' + (self.month / 10),
            b'0' + (self.month % 10),
            b'/',
            b'0' + ((y / 1000) % 10) as u8,
            b'0' + ((y / 100) % 10) as u8,
            b'0' + ((y / 10) % 10) as u8,
            b'0' + (y % 10) as u8,
        ]
    }
}

#[inline]
fn cpuid_leaf(leaf: u32) -> (u32, u32, u32, u32) {
    let eax: u32;
    let ebx: u32;
    let ecx: u32;
    let edx: u32;
    // SAFETY:
    // Executes architectural `cpuid` instruction while preserving `rbx`.
    unsafe {
        asm!(
            "xchg rbx, {tmp}",
            "cpuid",
            "mov {ebx_out:e}, ebx",
            "xchg rbx, {tmp}",
            inout("eax") leaf => eax,
            ebx_out = out(reg) ebx,
            out("ecx") ecx,
            out("edx") edx,
            tmp = inout(reg) 0u64 => _,
            options(nostack, preserves_flags)
        );
    }
    (eax, ebx, ecx, edx)
}

pub fn read_cpu_brand() -> [u8; 48] {
    let mut brand = [b' '; 48];
    let (max_ext, _, _, _) = cpuid_leaf(0x8000_0000);
    if max_ext < 0x8000_0004 {
        let fallback = b"x86_64 64-bit Processor (2013+ ISA)";
        brand[..fallback.len()].copy_from_slice(fallback);
        return brand;
    }

    let leaves = [0x8000_0002u32, 0x8000_0003, 0x8000_0004];
    for (idx, &leaf) in leaves.iter().enumerate() {
        let (a, b, c, d) = cpuid_leaf(leaf);
        let base = idx * 16;
        brand[base..base + 4].copy_from_slice(&a.to_le_bytes());
        brand[base + 4..base + 8].copy_from_slice(&b.to_le_bytes());
        brand[base + 8..base + 12].copy_from_slice(&c.to_le_bytes());
        brand[base + 12..base + 16].copy_from_slice(&d.to_le_bytes());
    }

    for byte in brand.iter_mut() {
        if *byte == 0 || *byte < 0x20 || *byte > 0x7E {
            *byte = b' ';
        }
    }
    brand
}

#[derive(Debug, Clone, Copy)]
pub struct PciDeviceInfo {
    pub bus: u8,
    pub slot: u8,
    pub func: u8,
    pub vendor_id: u16,
    pub device_id: u16,
    pub class_code: u8,
    pub subclass: u8,
    pub prog_if: u8,
    pub bar0: u32,
}

impl PciDeviceInfo {
    pub const fn empty() -> Self {
        Self {
            bus: 0,
            slot: 0,
            func: 0,
            vendor_id: 0xFFFF,
            device_id: 0xFFFF,
            class_code: 0,
            subclass: 0,
            prog_if: 0,
            bar0: 0,
        }
    }

    pub const fn driver_description(&self) -> &'static [u8] {
        match (self.class_code, self.subclass, self.prog_if) {
            (0x01, 0x06, _) => b"ahci_sata  (SATA 3.0 AHCI Storage)",
            (0x01, 0x08, _) => b"nvme_pcie  (NVMe M.2 Solid State)",
            (0x01, _, _) => b"virtio_blk (Block Storage Driver)",
            (0x02, 0x00, _) => b"e1000e_net (Gigabit Ethernet NIC)",
            (0x03, 0x00, _) => b"uefi_gop   (Linear Framebuffer GPU)",
            (0x04, 0x03, _) => b"snd_hda    (High Definition Audio)",
            (0x06, 0x00, _) => b"pcie_root  (Host PCIe Root Complex)",
            (0x06, 0x01, _) => b"lpc_ich9   (LPC Bridge & CMOS RTC)",
            (0x0C, 0x03, 0x30) => b"xhci_usb3  (USB 3.x xHCI Host)",
            (0x0C, 0x03, _) => b"usb_ehci   (USB 2.0 Host Controller)",
            (0x0C, 0x05, _) => b"i801_smbus (System Management Bus)",
            _ => b"pci_generic (PCIe System Device)",
        }
    }
}

#[inline]
pub fn pci_config_read_u32(bus: u8, slot: u8, func: u8, offset: u8) -> u32 {
    let addr: u32 = 0x8000_0000
        | ((bus as u32) << 16)
        | ((slot as u32 & 0x1F) << 11)
        | ((func as u32 & 0x07) << 8)
        | (offset as u32 & 0xFC);
    outl(0xCF8, addr);
    inl(0xCFC)
}

pub fn scan_pci_bus(out_devices: &mut [PciDeviceInfo; 8]) -> usize {
    let mut count = 0;
    for slot in 0..32u8 {
        for func in 0..8u8 {
            let id_reg = pci_config_read_u32(0, slot, func, 0x00);
            let vendor_id = (id_reg & 0xFFFF) as u16;
            if vendor_id == 0xFFFF {
                if func == 0 {
                    break;
                }
                continue;
            }
            let device_id = (id_reg >> 16) as u16;
            let class_reg = pci_config_read_u32(0, slot, func, 0x08);
            let class_code = ((class_reg >> 24) & 0xFF) as u8;
            let subclass = ((class_reg >> 16) & 0xFF) as u8;
            let prog_if = ((class_reg >> 8) & 0xFF) as u8;
            let bar0 = pci_config_read_u32(0, slot, func, 0x10);

            if let Some(dev_slot) = out_devices.get_mut(count) {
                *dev_slot = PciDeviceInfo {
                    bus: 0,
                    slot,
                    func,
                    vendor_id,
                    device_id,
                    class_code,
                    subclass,
                    prog_if,
                    bar0,
                };
                count += 1;
            }
            if count >= out_devices.len() {
                return count;
            }
            if func == 0 {
                let hdr_reg = pci_config_read_u32(0, slot, 0, 0x0C);
                let header_type = ((hdr_reg >> 16) & 0xFF) as u8;
                if (header_type & 0x80) == 0 {
                    break;
                }
            }
        }
    }
    count
}
