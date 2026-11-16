// framework/zenith-frame/src/memory.rs
//! Physical frame allocation and safe memory buffer reader/writer capabilities.

use crate::Error;

/// Standard 4 KiB page/frame size on x86_64.
pub const PAGE_SIZE: usize = 4096;

/// Strongly-typed physical memory address.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PhysAddr(pub usize);

impl PhysAddr {
    /// Checks whether this physical address is aligned to [`PAGE_SIZE`].
    ///
    /// # Safety Invariants
    /// Pure arithmetic check without memory dereference.
    pub const fn is_page_aligned(self) -> bool {
        (self.0 & (PAGE_SIZE - 1)) == 0
    }
}

/// Strongly-typed canonical virtual memory address.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct VirtAddr(pub usize);

impl VirtAddr {
    /// Checks whether this virtual address is aligned to [`PAGE_SIZE`].
    ///
    /// # Safety Invariants
    /// Pure arithmetic check without memory dereference.
    pub const fn is_page_aligned(self) -> bool {
        (self.0 & (PAGE_SIZE - 1)) == 0
    }
}

/// Safe capability token representing ownership of an allocated 4 KiB physical frame.
#[derive(Debug, PartialEq, Eq)]
pub struct VmFrame {
    index: usize,
    paddr: PhysAddr,
}

impl VmFrame {
    /// Returns the starting physical address of this 4 KiB frame.
    ///
    /// # Safety Invariants
    /// Guaranteed to be 4 KiB aligned and within the managed physical pool.
    pub const fn start_paddr(&self) -> PhysAddr {
        self.paddr
    }

    /// Returns the frame index within the allocator pool.
    ///
    /// # Safety Invariants
    /// Pure accessor for allocator bookkeeping.
    pub const fn index(&self) -> usize {
        self.index
    }
}

/// Static bitmap-backed physical frame allocator with zero heap allocation.
///
/// Manages `WORDS * 64` physical 4 KiB frames starting at `base_paddr`.
#[derive(Debug)]
pub struct FrameAllocator<const WORDS: usize> {
    base_paddr: usize,
    bitmap: [u64; WORDS],
}

impl<const WORDS: usize> Default for FrameAllocator<WORDS> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const WORDS: usize> FrameAllocator<WORDS> {
    /// Creates a new frame allocator with all frames initially marked free.
    ///
    /// # Safety Invariants
    /// Initializes static bitmap in-place on the stack/BSS with zero heap allocation.
    pub const fn new() -> Self {
        Self {
            base_paddr: 0x0010_0000, // Default 1 MiB physical base
            bitmap: [0u64; WORDS],
        }
    }

    /// Allocates a single 4 KiB physical [`VmFrame`] in O(WORDS) time.
    ///
    /// # Safety Invariants
    /// Ensures exclusive ownership of the returned `VmFrame` by setting its bitmap bit.
    pub fn alloc_frame(&mut self) -> Result<VmFrame, Error> {
        for (word_idx, word) in self.bitmap.iter_mut().enumerate() {
            if *word != u64::MAX {
                let bit = (*word).trailing_ones() as usize;
                *word |= 1u64 << bit;
                let frame_idx = word_idx * 64 + bit;
                let paddr = PhysAddr(self.base_paddr + frame_idx * PAGE_SIZE);
                return Ok(VmFrame {
                    index: frame_idx,
                    paddr,
                });
            }
        }
        Err(Error::OutOfMemory)
    }

    /// Releases a previously allocated [`VmFrame`] back to the pool.
    ///
    /// # Safety Invariants
    /// Consumes `frame` by value so the caller cannot use the frame after freeing it.
    pub fn free_frame(&mut self, frame: VmFrame) -> Result<(), Error> {
        let word_idx = frame.index / 64;
        let bit = frame.index % 64;
        if word_idx >= WORDS {
            return Err(Error::InvalidArgument);
        }
        let mask = 1u64 << bit;
        if (self.bitmap[word_idx] & mask) == 0 {
            return Err(Error::InvalidArgument);
        }
        self.bitmap[word_idx] &= !mask;
        Ok(())
    }
}

/// Safe fallible reader over an untrusted or user-space memory buffer.
#[derive(Debug)]
pub struct VmReader<'a> {
    slice: &'a [u8],
    cursor: usize,
}

impl<'a> VmReader<'a> {
    /// Constructs a `VmReader` from a safe byte slice.
    ///
    /// # Safety Invariants
    /// Bound to lifetime `'a` of the underlying validated slice.
    pub const fn from_slice(slice: &'a [u8]) -> Self {
        Self { slice, cursor: 0 }
    }

    /// Constructs a `VmReader` from a raw pointer and length inside the TCB.
    ///
    /// # Safety
    /// The caller in `zenith-frame` must guarantee that `ptr..ptr + len` is mapped,
    /// readable, and valid for lifetime `'a`.
    ///
    /// # Safety Invariants
    /// Rejects null pointers and address arithmetic overflows before creating a slice.
    pub unsafe fn from_raw_parts(ptr: *const u8, len: usize) -> Result<Self, Error> {
        if ptr.is_null() || (ptr as usize).checked_add(len).is_none() {
            return Err(Error::InvalidAddress);
        }
        // SAFETY: Validated non-null and non-overflowing range; caller guarantees mapping.
        let slice = unsafe { core::slice::from_raw_parts(ptr, len) };
        Ok(Self { slice, cursor: 0 })
    }

    /// Returns the number of unread bytes remaining.
    ///
    /// # Safety Invariants
    /// Guaranteed `self.cursor <= self.slice.len()`.
    pub const fn remaining(&self) -> usize {
        self.slice.len() - self.cursor
    }

    /// Copies up to `dst.len()` bytes into `dst` and advances the cursor.
    ///
    /// # Safety Invariants
    /// Performs bounds-checked slice copy with zero raw pointer arithmetic.
    pub fn read_bytes(&mut self, dst: &mut [u8]) -> Result<usize, Error> {
        let count = dst.len().min(self.remaining());
        dst[..count].copy_from_slice(&self.slice[self.cursor..self.cursor + count]);
        self.cursor += count;
        Ok(count)
    }
}

/// Safe fallible writer over an untrusted or user-space memory buffer.
#[derive(Debug)]
pub struct VmWriter<'a> {
    slice: &'a mut [u8],
    cursor: usize,
}

impl<'a> VmWriter<'a> {
    /// Constructs a `VmWriter` from a safe mutable byte slice.
    ///
    /// # Safety Invariants
    /// Bound to exclusive lifetime `'a` of the underlying slice.
    pub fn from_slice(slice: &'a mut [u8]) -> Self {
        Self { slice, cursor: 0 }
    }

    /// Constructs a `VmWriter` from a raw mutable pointer and length inside the TCB.
    ///
    /// # Safety
    /// The caller in `zenith-frame` must guarantee that `ptr..ptr + len` is uniquely mapped,
    /// writable, and valid for lifetime `'a`.
    ///
    /// # Safety Invariants
    /// Rejects null pointers and address arithmetic overflows before creating a mutable slice.
    pub unsafe fn from_raw_parts(ptr: *mut u8, len: usize) -> Result<Self, Error> {
        if ptr.is_null() || (ptr as usize).checked_add(len).is_none() {
            return Err(Error::InvalidAddress);
        }
        // SAFETY: Validated non-null and non-overflowing range; caller guarantees exclusive mapping.
        let slice = unsafe { core::slice::from_raw_parts_mut(ptr, len) };
        Ok(Self { slice, cursor: 0 })
    }

    /// Returns the number of writable bytes remaining.
    ///
    /// # Safety Invariants
    /// Guaranteed `self.cursor <= self.slice.len()`.
    pub const fn remaining(&self) -> usize {
        self.slice.len() - self.cursor
    }

    /// Copies up to `src.len()` bytes from `src` into the buffer and advances the cursor.
    ///
    /// # Safety Invariants
    /// Performs bounds-checked slice copy with zero raw pointer arithmetic.
    pub fn write_bytes(&mut self, src: &[u8]) -> Result<usize, Error> {
        let count = src.len().min(self.remaining());
        self.slice[self.cursor..self.cursor + count].copy_from_slice(&src[..count]);
        self.cursor += count;
        Ok(count)
    }
}
