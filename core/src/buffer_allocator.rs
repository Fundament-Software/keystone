/// Uses up Buf before resorting to allocations. If it runs out of space in buf, doubles the
/// size of buf for next time.
#[derive(Default)]
pub struct BufferAllocator {
    buf: Vec<u8>,
    used: bool, // interesting note: this could be made threadsafe if this was atomic.
}

impl BufferAllocator {
    pub fn new() -> Self {
        Default::default()
    }

    /// Sets the internal buffer to have a capacity of at least minimum_size
    pub fn reserve(&mut self, minimum_size: usize) {
        if minimum_size > self.buf.len() {
            self.buf.resize(minimum_size, 0);
        }
    }
}

unsafe impl capnp::message::Allocator for BufferAllocator {
    fn allocate_segment(&mut self, minimum_size: u32) -> (*mut u8, u32) {
        let byte_count = minimum_size as usize * std::mem::size_of::<u64>();
        if self.used {
            let layout = std::alloc::Layout::from_size_align(byte_count, 8).unwrap();
            unsafe { (std::alloc::alloc_zeroed(layout), minimum_size) }
        } else {
            self.used = true;
            if byte_count > self.buf.len() {
                // If the buffer wasn't big enough, expand to double the minimum amount
                self.buf.resize(byte_count * 2, 0);
            }
            (
                self.buf.as_mut_ptr(),
                (self.buf.len() / std::mem::size_of::<u64>()) as u32,
            )
        }
    }

    unsafe fn deallocate_segment(&mut self, ptr: *mut u8, word_size: u32, words_used: u32) {
        if self.buf.as_ptr() == ptr {
            self.buf[..words_used as usize * std::mem::size_of::<u64>()].fill(0);
            self.used = false;
        } else {
            unsafe {
                std::alloc::dealloc(
                    ptr,
                    std::alloc::Layout::from_size_align(
                        word_size as usize * std::mem::size_of::<u64>(),
                        8,
                    )
                    .unwrap(),
                );
            }
        }
    }
}
