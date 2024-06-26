/// Uses up Buf before resorting to allocations. If it runs out of space in buf, doubles the
/// size of buf for next time.
pub struct BufferAllocator {
    buf: Vec<u8>,
    used: bool, // interesting note: this could be made threadsafe if this was atomic.
}

impl BufferAllocator {
    pub fn new() -> Self {
        Self {
            buf: Vec::new(),
            used: false,
        }
    }
}

unsafe impl capnp::message::Allocator for BufferAllocator {
    fn allocate_segment(&mut self, minimum_size: u32) -> (*mut u8, u32) {
        if self.used || minimum_size as usize > self.buf.len() {
            let layout = std::alloc::Layout::from_size_align(
                minimum_size as usize * std::mem::size_of::<u64>(),
                8,
            )
            .unwrap();
            unsafe { (std::alloc::alloc_zeroed(layout), minimum_size) }
        } else {
            self.used = true;
            (self.buf.as_mut_ptr(), self.buf.len() as u32)
        }
    }

    unsafe fn deallocate_segment(&mut self, ptr: *mut u8, word_size: u32, words_used: u32) {
        if self.buf.as_ptr() == ptr {
            self.buf[..words_used as usize].fill(0);
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
