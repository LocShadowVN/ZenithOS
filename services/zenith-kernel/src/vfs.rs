// services/zenith-kernel/src/vfs.rs
//! Pre-allocated Virtual File System (VFS), File Descriptor table, and in-memory `RamFs`.

use zenith_frame::memory::{VmReader, VmWriter};
use zenith_frame::Error;

/// Maximum length in bytes of a file name in `RamFs`.
pub const MAX_FILENAME_LEN: usize = 32;

/// Strongly-typed POSIX File Descriptor index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileDescriptor(pub usize);

/// Standard POSIX input stream (`STDIN_FILENO = 0`).
pub const STDIN_FD: FileDescriptor = FileDescriptor(0);
/// Standard POSIX output stream (`STDOUT_FILENO = 1`).
pub const STDOUT_FD: FileDescriptor = FileDescriptor(1);
/// Standard POSIX error stream (`STDERR_FILENO = 2`).
pub const STDERR_FD: FileDescriptor = FileDescriptor(2);

/// Classification of an open VFS file description.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InodeKind {
    /// Terminal / Serial console character device (`ttyS0` / `console`).
    ConsoleStream,
    /// Read-only in-memory file stored in `RamFs` (`initramfs`).
    RamFile {
        /// Index of the file entry inside `RamFs`.
        file_idx: usize,
    },
}

/// State of a single slot in a process [`FdTable`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FdEntry {
    /// Underlying VFS inode kind.
    pub kind: InodeKind,
    /// Current byte read/write offset.
    pub offset: usize,
    /// True if readable.
    pub readable: bool,
    /// True if writable.
    pub writable: bool,
}

/// Per-process pre-allocated File Descriptor table with zero heap allocation.
#[derive(Debug)]
pub struct FdTable<const MAX_FDS: usize> {
    slots: [Option<FdEntry>; MAX_FDS],
}

impl<const MAX_FDS: usize> Default for FdTable<MAX_FDS> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const MAX_FDS: usize> FdTable<MAX_FDS> {
    /// Creates a new `FdTable` with `stdin` (0), `stdout` (1), and `stderr` (2) pre-opened.
    ///
    /// # Safety Invariants
    /// Uses stack/static array initialization with zero heap allocation.
    pub const fn new() -> Self {
        let mut slots = [None; MAX_FDS];
        if MAX_FDS > 0 {
            slots[0] = Some(FdEntry {
                kind: InodeKind::ConsoleStream,
                offset: 0,
                readable: true,
                writable: false,
            });
        }
        if MAX_FDS > 1 {
            slots[1] = Some(FdEntry {
                kind: InodeKind::ConsoleStream,
                offset: 0,
                readable: false,
                writable: true,
            });
        }
        if MAX_FDS > 2 {
            slots[2] = Some(FdEntry {
                kind: InodeKind::ConsoleStream,
                offset: 0,
                readable: false,
                writable: true,
            });
        }
        Self { slots }
    }

    /// Allocates the lowest available `FileDescriptor` for `entry`.
    ///
    /// # Safety Invariants
    /// Bounded scan over `MAX_FDS`; returns `Err(Error::OutOfMemory)` when full.
    pub fn alloc_fd(&mut self, entry: FdEntry) -> Result<FileDescriptor, Error> {
        for (idx, slot) in self.slots.iter_mut().enumerate() {
            if slot.is_none() {
                *slot = Some(entry);
                return Ok(FileDescriptor(idx));
            }
        }
        Err(Error::OutOfMemory)
    }

    /// Returns a reference to the [`FdEntry`] at `fd`.
    ///
    /// # Safety Invariants
    /// Bounds-checked lookup returning `Err(Error::NotFound)` for invalid/closed FDs.
    pub fn get(&self, fd: FileDescriptor) -> Result<&FdEntry, Error> {
        self.slots
            .get(fd.0)
            .and_then(|opt| opt.as_ref())
            .ok_or(Error::NotFound)
    }

    /// Closes an open `FileDescriptor` and frees its slot.
    ///
    /// # Safety Invariants
    /// Bounds-checked removal returning `Err(Error::NotFound)` if already closed.
    pub fn close(&mut self, fd: FileDescriptor) -> Result<(), Error> {
        let slot = self.slots.get_mut(fd.0).ok_or(Error::NotFound)?;
        if slot.is_none() {
            return Err(Error::NotFound);
        }
        *slot = None;
        Ok(())
    }
}

/// Static metadata entry for a file registered in [`RamFs`].
#[derive(Debug, Clone, Copy)]
struct RamFileEntry {
    name: [u8; MAX_FILENAME_LEN],
    name_len: usize,
    data: &'static [u8],
}

/// Pre-allocated in-memory filesystem (`initramfs`) for boot binaries and configs.
#[derive(Debug)]
pub struct RamFs<const MAX_FILES: usize> {
    files: [Option<RamFileEntry>; MAX_FILES],
}

impl<const MAX_FILES: usize> Default for RamFs<MAX_FILES> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const MAX_FILES: usize> RamFs<MAX_FILES> {
    /// Creates an empty `RamFs` table.
    ///
    /// # Safety Invariants
    /// Uses `const` initialization on the stack/BSS without heap allocation.
    pub const fn new() -> Self {
        Self {
            files: [None; MAX_FILES],
        }
    }

    /// Registers a static byte slice as a named file in `RamFs`.
    ///
    /// # Safety Invariants
    /// Validates `name` length (`1..=MAX_FILENAME_LEN`) and prevents duplicate names.
    pub fn register_file(&mut self, name: &[u8], data: &'static [u8]) -> Result<usize, Error> {
        if name.is_empty() || name.len() > MAX_FILENAME_LEN {
            return Err(Error::InvalidArgument);
        }
        for existing in self.files.iter().flatten() {
            if &existing.name[..existing.name_len] == name {
                return Err(Error::InvalidArgument);
            }
        }
        for (idx, slot) in self.files.iter_mut().enumerate() {
            if slot.is_none() {
                let mut name_buf = [0u8; MAX_FILENAME_LEN];
                name_buf[..name.len()].copy_from_slice(name);
                *slot = Some(RamFileEntry {
                    name: name_buf,
                    name_len: name.len(),
                    data,
                });
                return Ok(idx);
            }
        }
        Err(Error::OutOfMemory)
    }

    /// Looks up a file by `name` and returns its static payload slice.
    ///
    /// # Safety Invariants
    /// Read-only bounded slice comparison.
    pub fn lookup(&self, name: &[u8]) -> Result<&'static [u8], Error> {
        for entry in self.files.iter().flatten() {
            if &entry.name[..entry.name_len] == name {
                return Ok(entry.data);
            }
        }
        Err(Error::NotFound)
    }

    /// Reads bytes from a `RamFs` file at `offset` into a safe [`VmWriter`].
    ///
    /// # Safety Invariants
    /// Uses `VmReader` and `VmWriter` capabilities; zero raw pointer arithmetic.
    pub fn read_into(
        &self,
        file_idx: usize,
        offset: usize,
        writer: &mut VmWriter<'_>,
    ) -> Result<usize, Error> {
        let entry = self
            .files
            .get(file_idx)
            .and_then(|opt| opt.as_ref())
            .ok_or(Error::NotFound)?;

        if offset >= entry.data.len() {
            return Ok(0);
        }
        let mut reader = VmReader::from_slice(&entry.data[offset..]);
        let mut scratch = [0u8; 64];
        let to_copy = scratch.len().min(reader.remaining()).min(writer.remaining());
        let read_bytes = reader.read_bytes(&mut scratch[..to_copy])?;
        writer.write_bytes(&scratch[..read_bytes])
    }
}

/// Writes console stream data to the hardware UART/serial port.
///
/// # Safety Invariants
/// Stubbed until `zenith-frame` exposes a safe UART/IoPort capability.
pub fn write_console_serial(_reader: &mut VmReader<'_>) -> Result<usize, Error> {
    // TODO: requires console_serial_write capability in zenith-frame
    Err(Error::NotImplemented)
}

/// Reads an inode payload from a persistent block device (Ext4/NVMe/VirtIO-Blk).
///
/// # Safety Invariants
/// Stubbed until `zenith-drivers` block subsystem is registered.
pub fn read_block_inode(_inode_num: u64, _writer: &mut VmWriter<'_>) -> Result<usize, Error> {
    // TODO: requires block_device_read_inode service
    Err(Error::NotImplemented)
}
