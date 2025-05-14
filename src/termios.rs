#![allow(dead_code)]

// Termios constants
pub const TCGETS: usize = 0x5401;
pub const TCSETS: usize = 0x5402;
pub const TCSETSW: usize = 0x5403;
pub const TCSETSF: usize = 0x5404;
pub const TIOCGWINSZ: usize = 0x5413;

// Termios flag constants
pub const ECHO: u32 = 0o000_010;
pub const ICANON: u32 = 0o000_002;
pub const ISIG: u32 = 0o000_001;
pub const IEXTEN: u32 = 0o100_000;
pub const BRKINT: u32 = 0o000_002;
pub const ICRNL: u32 = 0o000_400;
pub const INPCK: u32 = 0o000_020;
pub const ISTRIP: u32 = 0o000_040;
pub const IXON: u32 = 0o002_000;
pub const OPOST: u32 = 0o000_001;
pub const CS8: u32 = 0o000_060;

// Termios special character positions
pub const VMIN: usize = 6;
pub const VTIME: usize = 5;

// Window size structure
#[repr(C)]
#[derive(Clone)]
pub struct Winsize {
    pub rows: u16,
    pub cols: u16,
    pub xpixel: u16,
    pub ypixel: u16,
}

impl Winsize {
    pub fn new() -> Self {
        Self {
            rows: 0,
            cols: 0,
            xpixel: 0,
            ypixel: 0,
        }
    }

    pub fn as_bytes(&self) -> &[u8; 8] {
        unsafe { &*core::ptr::from_ref::<Winsize>(self).cast::<[u8; 8]>() }
    }

    pub fn as_bytes_mut(&mut self) -> &mut [u8; 8] {
        unsafe { &mut *core::ptr::from_mut::<Winsize>(self).cast::<[u8; 8]>() }
    }
}

// Termios structure
#[repr(C)]
#[derive(Clone)]
pub struct Termios {
    pub iflag: u32,   // Input modes
    pub oflag: u32,   // Output modes
    pub cflag: u32,   // Control modes
    pub lflag: u32,   // Local modes
    pub line: u8,     // Line discipline
    pub cc: [u8; 32], // Control characters
    pub ispeed: u32,  // Input speed
    pub ospeed: u32,  // Output speed
}

impl Termios {
    pub fn new() -> Self {
        Self {
            iflag: 0,
            oflag: 0,
            cflag: 0,
            lflag: 0,
            line: 0,
            cc: [0; 32],
            ispeed: 0,
            ospeed: 0,
        }
    }

    pub fn as_bytes(&self) -> &[u8; 60] {
        unsafe { &*core::ptr::from_ref::<Termios>(self).cast::<[u8; 60]>() }
    }

    pub fn as_bytes_mut(&mut self) -> &mut [u8; 60] {
        unsafe { &mut *core::ptr::from_mut::<Termios>(self).cast::<[u8; 60]>() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_winsize_new() {
        let winsize = Winsize::new();

        assert_eq!(winsize.rows, 0);
        assert_eq!(winsize.cols, 0);
        assert_eq!(winsize.xpixel, 0);
        assert_eq!(winsize.ypixel, 0);
    }

    #[test]
    fn test_termios_new() {
        let termios = Termios::new();

        assert_eq!(termios.iflag, 0);
        assert_eq!(termios.oflag, 0);
        assert_eq!(termios.cflag, 0);
        assert_eq!(termios.lflag, 0);
        assert_eq!(termios.line, 0);

        // Check that all control chars are 0
        for i in 0..32 {
            assert_eq!(termios.cc[i], 0);
        }

        assert_eq!(termios.ispeed, 0);
        assert_eq!(termios.ospeed, 0);
    }

    #[test]
    fn test_winsize_as_bytes() {
        let mut winsize = Winsize::new();
        winsize.rows = 25;
        winsize.cols = 80;

        // Test immutable bytes
        {
            let bytes = winsize.as_bytes();
            assert_eq!(bytes.len(), 8);

            // Verify bytes contain the expected values
            // (This depends on the memory layout, which might be platform-specific)
            // For a basic test, we can just ensure it's not all zeros
            let all_zeros = bytes.iter().all(|&b| b == 0);
            assert!(!all_zeros, "Bytes should not be all zeros");
        }

        // Test mutable bytes separately
        {
            let bytes_mut = winsize.as_bytes_mut();
            assert_eq!(bytes_mut.len(), 8);

            // Verify the mutable bytes can actually be modified
            // and that it affects the original struct
            bytes_mut[0] = 42;
            bytes_mut[1] = 43;

            // Check that modifying the bytes affected the struct
            // The exact mapping depends on endianness, but we know
            // the first two bytes should map to rows
            assert_ne!(winsize.rows, 25, "Modifying bytes should affect the struct");
        }
    }
}
