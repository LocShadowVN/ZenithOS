use crate::ostd::{inl, outl};

#[derive(Debug, Clone, Copy)]
pub struct PciDeviceInfo {
    pub bus: u8,
    pub slot: u8,
    pub func: u8,
    pub vendor_id: u16,
    pub device_id: u16,
    pub class_code: u8,
    pub subclass: u8,
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
        }
    }

    pub const fn class_name(&self) -> &'static [u8] {
        match (self.class_code, self.subclass) {
            (0x01, 0x06) => b"SATA AHCI Controller",
            (0x01, 0x08) => b"NVMe SSD Controller",
            (0x01, _) => b"Mass Storage Controller",
            (0x02, 0x00) => b"Ethernet Network Adapter",
            (0x03, 0x00) => b"VGA Display Controller",
            (0x06, 0x00) => b"Host PCIe Bridge",
            (0x06, 0x01) => b"LPC ISA Bridge",
            (0x0C, 0x03) => b"USB xHCI Controller",
            (0x0C, 0x05) => b"SMBus Controller",
            _ => b"PCIe System Device",
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

/// Scans PCI Bus 0 and populates discovered hardware controllers into `out_devices`.
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

            if let Some(dev_slot) = out_devices.get_mut(count) {
                *dev_slot = PciDeviceInfo {
                    bus: 0,
                    slot,
                    func,
                    vendor_id,
                    device_id,
                    class_code,
                    subclass,
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
