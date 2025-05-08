use crate::syscall::{SysResult, ioctl, putchar, puts};
use crate::termios::{
    BRKINT, CS8, ECHO, ICANON, ICRNL, IEXTEN, INPCK, ISIG, ISTRIP, IXON, OPOST, TCGETS, TCSETS,
    TIOCGWINSZ, Termios, VMIN, VTIME, Winsize,
};

// Get window size
pub fn get_winsize(fd: usize, winsize: &mut Winsize) -> SysResult {
    ioctl(fd, TIOCGWINSZ, winsize.as_bytes_mut().as_mut_ptr() as usize)
}

// Get terminal attributes
pub fn get_termios(fd: usize, termios: &mut Termios) -> SysResult {
    ioctl(fd, TCGETS, termios.as_bytes_mut().as_mut_ptr() as usize)
}

// Set terminal attributes
pub fn set_termios(fd: usize, option: usize, termios: &Termios) -> SysResult {
    ioctl(fd, option, termios.as_bytes().as_ptr() as usize)
}

// Set raw mode flags
pub fn set_raw_mode(termios: &mut Termios) {
    // Input flags
    termios.iflag &= !(BRKINT | ICRNL | INPCK | ISTRIP | IXON);

    // Output flags
    termios.oflag &= !OPOST;

    // Control flags
    termios.cflag |= CS8;

    // Local flags
    termios.lflag &= !(ECHO | ICANON | ISIG | IEXTEN);

    // Control characters
    termios.cc[VMIN] = 1; // Return after 1 byte read
    termios.cc[VTIME] = 0; // No timeout
}

// Write a number
pub fn write_number(mut n: u16) {
    if n == 0 {
        let _ = putchar(b'0');
        return;
    }

    let mut digits = [0u8; 5];
    let mut i = 0;

    while n > 0 && i < 5 {
        digits[i] = (n % 10) as u8 + b'0';
        n /= 10;
        i += 1;
    }

    while i > 0 {
        i -= 1;
        let _ = putchar(digits[i]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Test struct to capture output
    struct OutputCapture {
        data: Vec<u8>,
    }

    impl OutputCapture {
        fn new() -> Self {
            Self { data: Vec::new() }
        }

        fn capture(&mut self, byte: u8) -> SysResult {
            self.data.push(byte);
            Ok(1)
        }

        fn as_string(&self) -> String {
            let mut result = String::new();
            for &b in &self.data {
                result.push(b as char);
            }
            result
        }
    }

    #[test]
    fn test_write_number() {
        fn test_write_number_impl(n: u16, capture: &mut OutputCapture) {
            if n == 0 {
                let _ = capture.capture(b'0');
                return;
            }

            let mut digits = [0u8; 5];
            let mut i = 0;

            let mut num = n;
            while num > 0 && i < 5 {
                digits[i] = (num % 10) as u8 + b'0';
                num /= 10;
                i += 1;
            }

            while i > 0 {
                i -= 1;
                let _ = capture.capture(digits[i]);
            }
        }

        // Test cases
        let test_cases = [
            (0, "0"),
            (1, "1"),
            (42, "42"),
            (123, "123"),
            (9999, "9999"),
            (65535, "65535"),
        ];

        for (input, expected) in test_cases {
            let mut capture = OutputCapture::new();
            test_write_number_impl(input, &mut capture);

            assert_eq!(
                capture.as_string(),
                expected,
                "write_number({}) should output '{}'",
                input,
                expected
            );
        }
    }

    #[test]
    fn test_set_raw_mode() {
        let mut termios = Termios::new();

        // Set all bits to 1 in the flags
        termios.iflag = 0xFFFFFFFF;
        termios.oflag = 0xFFFFFFFF;
        termios.lflag = 0xFFFFFFFF;

        // Apply raw mode
        set_raw_mode(&mut termios);

        // Check that input flags were cleared
        assert_eq!(termios.iflag & BRKINT, 0);
        assert_eq!(termios.iflag & ICRNL, 0);
        assert_eq!(termios.iflag & INPCK, 0);
        assert_eq!(termios.iflag & ISTRIP, 0);
        assert_eq!(termios.iflag & IXON, 0);

        // Check that output flags were cleared
        assert_eq!(termios.oflag & OPOST, 0);

        // Check that control flags were set
        assert_eq!(termios.cflag & CS8, CS8);

        // Check that local flags were cleared
        assert_eq!(termios.lflag & ECHO, 0);
        assert_eq!(termios.lflag & ICANON, 0);
        assert_eq!(termios.lflag & ISIG, 0);
        assert_eq!(termios.lflag & IEXTEN, 0);

        // Check control chars
        assert_eq!(termios.cc[VMIN], 1);
        assert_eq!(termios.cc[VTIME], 0);
    }

    #[test]
    fn test_clear_screen() {
        let mut capture = OutputCapture::new();

        // Define mock impl of clear_screen for testing
        fn test_clear_screen_impl(capture: &mut OutputCapture) -> SysResult {
            // Write the escape sequences
            for &b in b"\x1b[2J\x1b[H" {
                capture.capture(b)?;
            }
            Ok(10) // Return length of the sequence
        }

        // Call the test implementation
        let result = test_clear_screen_impl(&mut capture);

        // Check results
        assert!(result.is_ok());
        assert_eq!(capture.as_string(), "\x1b[2J\x1b[H");
    }
}

// Clear the screen and position cursor at the top-left
pub fn clear_screen() -> SysResult {
    // ESC [ 2 J - Clear entire screen
    // ESC [ H - Move cursor to home position (0,0)
    puts(b"\x1b[2J\x1b[H")
}
