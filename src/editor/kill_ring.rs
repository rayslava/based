use crate::syscall::{MAP_ANONYMOUS, MAP_PRIVATE, PROT_READ, PROT_WRITE, mmap, munmap};

// Error types for kill ring operations
#[derive(Debug)]
pub(in crate::editor) enum KillRingError {
    AllocationFailed,
    BufferTooLarge,
}

// KillRing manages the clipboard buffer
pub(in crate::editor) struct KillRing {
    buffer: *mut u8, // Raw buffer for kill-ring content
    size: usize,     // Current size of content
    capacity: usize, // Fixed capacity (one page = 4096 bytes)
}

impl KillRing {
    pub(in crate::editor) fn new() -> Result<Self, KillRingError> {
        const PAGE_SIZE: usize = 4096;

        let prot = PROT_READ | PROT_WRITE;
        let flags = MAP_PRIVATE | MAP_ANONYMOUS;

        let Ok(buffer) = mmap(0, PAGE_SIZE, prot, flags, usize::MAX, 0) else {
            return Err(KillRingError::AllocationFailed);
        };

        Ok(Self {
            buffer: buffer as *mut u8,
            size: 0,
            capacity: PAGE_SIZE,
        })
    }

    // Copy text to the kill ring
    pub(in crate::editor) fn copy(&mut self, text: &[u8]) -> Result<(), KillRingError> {
        // Check if the text fits in the buffer
        if text.len() > self.capacity {
            return Err(KillRingError::BufferTooLarge);
        }

        // Copy text to buffer
        for (i, &byte) in text.iter().enumerate() {
            unsafe {
                *self.buffer.add(i) = byte;
            }
        }
        self.size = text.len();

        Ok(())
    }

    // Get current content of the kill ring
    pub(in crate::editor) fn content(&self) -> &[u8] {
        unsafe { core::slice::from_raw_parts(self.buffer, self.size) }
    }

    // Get the capacity of the kill ring
    pub(in crate::editor) fn capacity(&self) -> usize {
        self.capacity
    }

    // Delete these API compatibility methods since they're handled in EditorState
    // We don't need them anymore since they're properly implemented in EditorState
}

impl Drop for KillRing {
    fn drop(&mut self) {
        if !self.buffer.is_null() && self.capacity > 0 {
            let _ = munmap(self.buffer as usize, self.capacity);
        }
    }
}

#[cfg(test)]
mod kill_ring_tests {
    use super::*;
    use crate::editor::EditorState;
    use crate::editor::file_buffer::tests::create_test_file_buffer;
    use crate::termios::Winsize;
    use std::vec::Vec;

    // Only testing basic kill ring functionality directly, not the integration with EditorState
    #[test]
    fn test_kill_ring_basic() {
        // Create a killring
        let Ok(mut kill_ring) = KillRing::new() else {
            panic!("Failed to create kill-ring");
        };

        // Try to copy text
        let test_text = b"Hello, world!";
        let result = kill_ring.copy(test_text);
        assert!(result.is_ok(), "Copy operation should succeed");

        // Verify text was copied
        assert_eq!(
            kill_ring.content(),
            test_text,
            "Kill ring should contain the copied text"
        );
    }

    #[test]
    fn test_buffer_too_large() {
        // Create a killring with small capacity for testing
        let Ok(mut kill_ring) = KillRing::new() else {
            panic!("Failed to create kill-ring");
        };

        // Try to copy text larger than capacity
        let large_text = std::iter::repeat_n(b'X', kill_ring.capacity() + 1).collect::<Vec<u8>>();
        let result = kill_ring.copy(&large_text);

        // Verify the error
        assert!(result.is_err(), "Copy should fail with buffer too large");
        assert!(
            matches!(result, Err(KillRingError::BufferTooLarge)),
            "Expected BufferTooLarge error"
        );
    }
}
