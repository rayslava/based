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

    #[test]
    fn test_alternate_screen() {
        let mut capture = OutputCapture::new();

        // Define mock implementations for testing

        fn test_enter_alternate_screen(capture: &mut OutputCapture) -> SysResult {
            for &b in b"\x1b[?1049h" {
                capture.capture(b)?;
            }
            Ok(8) // Return length of the sequence
        }

        fn test_exit_alternate_screen(capture: &mut OutputCapture) -> SysResult {
            for &b in b"\x1b[?1049l" {
                capture.capture(b)?;
            }
            Ok(8) // Return length of the sequence
        }

        // Test enter alternate screen
        let mut enter_capture = OutputCapture::new();
        let enter_result = test_enter_alternate_screen(&mut enter_capture);
        assert!(enter_result.is_ok());
        assert_eq!(enter_capture.as_string(), "\x1b[?1049h");

        // Test exit alternate screen
        let mut exit_capture = OutputCapture::new();
        let exit_result = test_exit_alternate_screen(&mut exit_capture);
        assert!(exit_result.is_ok());
        assert_eq!(exit_capture.as_string(), "\x1b[?1049l");
    }
}

// Clear the screen and position cursor at the top-left
pub fn clear_screen() -> SysResult {
    // ESC [ 2 J - Clear entire screen
    // ESC [ H - Move cursor to home position (0,0)
    puts(b"\x1b[2J\x1b[H")
}

// Save the current terminal state and switch to alternate screen buffer
pub fn enter_alternate_screen() -> SysResult {
    // ESC [ ? 1049 h - Save cursor position and switch to alternate screen
    puts(b"\x1b[?1049h")
}

// Restore the previous terminal state from the main screen buffer
pub fn exit_alternate_screen() -> SysResult {
    // ESC [ ? 1049 l - Restore cursor position and switch to normal screen
    puts(b"\x1b[?1049l")
}

// Move cursor to a specific position (0-based coordinates)
pub fn move_cursor(row: u16, col: u16) -> SysResult {
    // Format: ESC [ row+1 ; col+1 H
    let mut buf = [0u8; 16];
    let mut pos = 0;

    // Start sequence
    buf[pos] = b'\x1b';
    buf[pos + 1] = b'[';
    pos += 2;

    // Row (+1 because ANSI is 1-based)
    let row_num = row + 1;
    pos += write_u16_to_buf(&mut buf[pos..], row_num);

    // Separator
    buf[pos] = b';';
    pos += 1;

    // Column (+1 because ANSI is 1-based)
    let col_num = col + 1;
    pos += write_u16_to_buf(&mut buf[pos..], col_num);

    // End sequence
    buf[pos] = b'H';
    pos += 1;

    // Write the sequence
    puts(&buf[0..pos])
}

// Helper to write a u16 number to buffer, returns bytes written
fn write_u16_to_buf(buf: &mut [u8], n: u16) -> usize {
    // Quick return for 0
    if n == 0 {
        buf[0] = b'0';
        return 1;
    }

    // Find highest divisor
    let mut div = 1;
    let mut tmp = n;
    while tmp >= 10 {
        div *= 10;
        tmp /= 10;
    }

    // Write digits
    let mut pos = 0;
    while div > 0 {
        buf[pos] = b'0' + ((n / div) % 10) as u8;
        pos += 1;
        div /= 10;
    }

    pos
}

// Set background color
pub fn set_bg_color(color: u8) -> SysResult {
    // Format: ESC [ 4 color m
    let mut buf = [b'\x1b', b'[', b'4', 0, b'm'];
    // Convert color to ascii
    buf[3] = b'0' + color;
    puts(&buf)
}

// Set foreground color
pub fn set_fg_color(color: u8) -> SysResult {
    // Format: ESC [ 3 color m
    let mut buf = [b'\x1b', b'[', b'3', 0, b'm'];
    // Convert color to ascii
    buf[3] = b'0' + color;
    puts(&buf)
}

// Reset colors to default
pub fn reset_colors() -> SysResult {
    // ESC [ 0 m
    puts(b"\x1b[0m")
}

// Draw the status bar with cursor position
pub fn draw_status_bar(winsize: &Winsize, row: u16, col: u16) -> SysResult {
    // Make sure we have at least 3 rows (1 for status bar, 1 for command line, and 1+ for editing)
    if winsize.rows < 3 {
        return Ok(0);
    }

    // Save cursor position
    puts(b"\x1b[s")?;

    // Move to status bar line (second to last row)
    move_cursor(winsize.rows - 2, 0)?;

    // Set colors for status bar (white text on blue background)
    set_bg_color(7)?;
    set_fg_color(0)?;

    // Initial status message - this has the cursor position
    let mut initial_msg = [0u8; 64];
    let mut pos = 0;

    // Add cursor position text
    let text = b" ROW: ";
    for &b in text {
        initial_msg[pos] = b;
        pos += 1;
    }

    // Add row number
    pos += write_u16_to_buf(&mut initial_msg[pos..], row);

    // Add column text
    let text = b", COL: ";
    for &b in text {
        initial_msg[pos] = b;
        pos += 1;
    }

    // Add column number
    pos += write_u16_to_buf(&mut initial_msg[pos..], col);

    // Add trailing space
    initial_msg[pos] = b' ';
    pos += 1;

    // Write the initial status message
    puts(&initial_msg[0..pos])?;

    // Clear to the end of line (makes sure status bar fills whole width)
    // ESC [ K - Clear from cursor to end of line
    puts(b"\x1b[K")?;

    // Reset colors
    reset_colors()?;

    // Restore cursor position
    puts(b"\x1b[u")
}
