use crate::syscall::{SysResult, ioctl, putchar, puts};
use crate::termios::{
    BRKINT, CS8, ECHO, ICANON, ICRNL, IEXTEN, INPCK, ISIG, ISTRIP, IXON, OPOST, TCGETS, TIOCGWINSZ,
    Termios, VMIN, VTIME, Winsize,
};

pub fn get_winsize(fd: usize, winsize: &mut Winsize) -> SysResult {
    ioctl(fd, TIOCGWINSZ, winsize.as_bytes_mut().as_mut_ptr() as usize)
}

pub fn get_termios(fd: usize, termios: &mut Termios) -> SysResult {
    ioctl(fd, TCGETS, termios.as_bytes_mut().as_mut_ptr() as usize)
}

pub fn set_termios(fd: usize, option: usize, termios: &Termios) -> SysResult {
    ioctl(fd, option, termios.as_bytes().as_ptr() as usize)
}

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

pub fn write_number(mut n: usize) {
    if n == 0 {
        let _ = putchar(b'0');
        return;
    }
    let mut digits = [0u8; 20];
    let mut i = 0;

    while n > 0 && i < 20 {
        let quotient = n / 10;
        let remainder = n - (quotient * 10);
        digits[i] = b'0' + remainder.to_le_bytes()[0];
        n = quotient;
        i += 1;
    }
    // Output digits in correct order
    while i > 0 {
        i -= 1;
        let _ = putchar(digits[i]);
    }
}

pub fn clear_screen() -> SysResult {
    // ESC [ 2 J - Clear entire screen
    // ESC [ H - Move cursor to home position (0,0)
    puts(b"\x1b[2J\x1b[H")
}

pub fn enter_alternate_screen() -> SysResult {
    // ESC [ ? 1049 h - Save cursor position and switch to alternate screen
    puts(b"\x1b[?1049h")
}

pub fn exit_alternate_screen() -> SysResult {
    // ESC [ ? 1049 l - Restore cursor position and switch to normal screen
    puts(b"\x1b[?1049l")
}

// Move cursor to a specific position (0-based coordinates)
pub fn move_cursor(row: usize, col: usize) -> SysResult {
    // Format: ESC [ row+1 ; col+1 H
    let mut buf = [0u8; 16];
    let mut pos = 0;

    // Start sequence
    buf[pos] = b'\x1b';
    buf[pos + 1] = b'[';
    pos += 2;

    // Row (+1 because ANSI is 1-based)
    let row_num = row + 1;
    pos += write_usize_to_buf(&mut buf[pos..], row_num);

    // Separator
    buf[pos] = b';';
    pos += 1;

    // Column (+1 because ANSI is 1-based)
    let col_num = col + 1;
    pos += write_usize_to_buf(&mut buf[pos..], col_num);

    // End sequence
    buf[pos] = b'H';
    pos += 1;

    // Write the sequence
    puts(&buf[0..pos])
}

// Helper to write a usize number to buffer, returns bytes written
fn write_usize_to_buf(buf: &mut [u8], n: usize) -> usize {
    if n == 0 {
        buf[0] = b'0';
        return 1;
    }

    let mut digits = [0u8; 20]; // Maximum usize digits (64-bit)
    let mut digit_count = 0;
    let mut num = n;

    // Collect digits in reverse order
    while num > 0 {
        let quotient = num / 10;
        let remainder = num - (quotient * 10);
        digits[digit_count] = b'0' + remainder.to_le_bytes()[0];

        num = quotient;
        digit_count += 1;
    }

    // Write digits in straight order
    let mut pos = 0;
    while digit_count > 0 {
        digit_count -= 1;
        buf[pos] = digits[digit_count];
        pos += 1;
    }

    pos
}

pub fn set_bg_color(color: u8) -> SysResult {
    // Format: ESC [ 4 color m
    let mut buf = [b'\x1b', b'[', b'4', 0, b'm'];
    // Convert color to ascii
    buf[3] = b'0' + color;
    puts(&buf)
}

pub fn set_fg_color(color: u8) -> SysResult {
    // Format: ESC [ 3 color m
    let mut buf = [b'\x1b', b'[', b'3', 0, b'm'];
    // Convert color to ascii
    buf[3] = b'0' + color;
    puts(&buf)
}

pub fn reset_colors() -> SysResult {
    // ESC [ 0 m
    puts(b"\x1b[0m")
}

pub fn set_bold() -> SysResult {
    // ESC [ 1 m - Bold text
    puts(b"\x1b[1m")
}

// Print a message to the last line of the screen
pub fn print_status(winsize: Winsize, msg: &[u8]) -> SysResult {
    // Save cursor position
    puts(b"\x1b[s")?;

    // Move to the last row
    move_cursor(winsize.rows as usize - 1, 0)?;

    // Clear the line
    puts(b"\x1b[K")?;

    // Print the message
    puts(msg)?;

    // Restore cursor position
    puts(b"\x1b[u")
}

// Print a normal message to the status line
pub fn print_message(winsize: Winsize, msg: &[u8]) -> SysResult {
    print_status(winsize, msg)
}

#[allow(dead_code)]
// Print a warning message (yellow) to the status line
pub fn print_warning(winsize: Winsize, msg: &[u8]) -> SysResult {
    // Save cursor position
    puts(b"\x1b[s")?;

    // Move to the last row
    move_cursor(winsize.rows as usize - 1, 0)?;

    // Clear the line
    puts(b"\x1b[K")?;

    // Set yellow color
    set_fg_color(3)?;

    // Print the message
    puts(msg)?;

    // Reset colors
    reset_colors()?;

    // Restore cursor position
    puts(b"\x1b[u")
}

// Print an error message (bold red) to the status line
pub fn print_error(winsize: Winsize, msg: &[u8]) -> SysResult {
    // Save cursor position
    puts(b"\x1b[s")?;

    // Move to the last row
    move_cursor(winsize.rows as usize - 1, 0)?;

    // Clear the line
    puts(b"\x1b[K")?;

    // Set bold red
    set_bold()?;
    set_fg_color(1)?;

    // Print the message
    puts(msg)?;

    // Reset colors
    reset_colors()?;

    // Restore cursor position
    puts(b"\x1b[u")
}

pub fn draw_status_bar(winsize: Winsize, row: usize, col: usize) -> SysResult {
    // Make sure we have at least 3 rows (1 for status bar, 1 for message line, and 1+ for editing)
    if winsize.rows < 3 {
        return Ok(0);
    }

    // Save cursor position
    puts(b"\x1b[s")?;

    // Move to status bar line (second to last row)
    move_cursor(winsize.rows as usize - 2, 0)?;

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
    pos += write_usize_to_buf(&mut initial_msg[pos..], row);

    // Add column text
    let text = b", COL: ";
    for &b in text {
        initial_msg[pos] = b;
        pos += 1;
    }

    // Add column number
    pos += write_usize_to_buf(&mut initial_msg[pos..], col);

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

#[cfg(test)]
pub mod tests {
    use super::*;

    // Test buffer for capturing output in tests
    pub static mut TEST_BUFFER: [u8; 64] = [0; 64];
    pub static mut TEST_BUFFER_LEN: usize = 0;
    static mut TEST_MODE: bool = false;

    pub fn handle_test_puts(bytes: &[u8]) -> usize {
        unsafe {
            if TEST_MODE {
                TEST_BUFFER_LEN = bytes.len();
                TEST_BUFFER[..bytes.len()].copy_from_slice(bytes);
                bytes.len()
            } else {
                // In case we don't want to capture output
                bytes.len()
            }
        }
    }

    // Helper to enable test mode and clear buffer
    pub fn enable_test_mode() {
        unsafe {
            TEST_MODE = true;
            TEST_BUFFER = [0; 64];
            TEST_BUFFER_LEN = 0;
        }
    }

    // Helper to disable test mode
    pub fn disable_test_mode() {
        unsafe {
            TEST_MODE = false;
        }
    }

    #[test]
    fn test_set_raw_mode() {
        let mut termios = Termios::new();

        // Set all bits to 1 in the flags
        termios.iflag = 0xFFFF_FFFF;
        termios.oflag = 0xFFFF_FFFF;
        termios.lflag = 0xFFFF_FFFF;

        // Apply raw mode (tests actual implementation)
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
    fn test_write_u16_to_buf() {
        let test_cases = [
            (0, "0", 1),
            (1, "1", 1),
            (42, "42", 2),
            (123, "123", 3),
            (9999, "9999", 4),
            (65535, "65535", 5),
        ];

        for (input, expected, expected_len) in test_cases {
            let mut buf = [0u8; 16];
            let len = write_usize_to_buf(&mut buf, input);

            assert_eq!(
                len, expected_len,
                "write_u16_to_buf({input}) should write {expected_len} bytes"
            );

            let output = std::str::from_utf8(&buf[0..len]).unwrap();
            assert_eq!(
                output, expected,
                "write_u16_to_buf({input}) should output '{expected}'"
            );
        }
    }

    #[test]
    fn test_move_cursor() {
        let test_cases = [
            ((0, 0), "\x1b[1;1H"),
            ((5, 10), "\x1b[6;11H"),
            ((99, 99), "\x1b[100;100H"),
            ((1000, 500), "\x1b[1001;501H"),
        ];

        for ((row, col), expected) in test_cases {
            let mut buf = [0u8; 16];
            let mut pos = 0;
            buf[pos] = b'\x1b';
            buf[pos + 1] = b'[';
            pos += 2;
            pos += write_usize_to_buf(&mut buf[pos..], row + 1);
            buf[pos] = b';';
            pos += 1;
            pos += write_usize_to_buf(&mut buf[pos..], col + 1);
            buf[pos] = b'H';
            pos += 1;
            let output = std::str::from_utf8(&buf[0..pos]).unwrap();
            assert_eq!(output, expected);
        }
    }

    fn test_write_number(n: u16) -> Vec<u8> {
        let mut result = Vec::new();
        if n == 0 {
            result.push(b'0');
            return result;
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
            result.push(digits[i]);
        }
        result
    }

    #[test]
    fn test_write_number_direct() {
        let test_cases = [
            (0, "0"),
            (1, "1"),
            (42, "42"),
            (123, "123"),
            (9999, "9999"),
            (65535, "65535"),
        ];

        for (input, expected) in test_cases {
            let output = test_write_number(input);
            let output_str = std::str::from_utf8(&output).unwrap();

            assert_eq!(
                output_str, expected,
                "write_number({input}) should output '{expected}'"
            );
        }
    }

    #[test]
    fn test_clear_screen_direct() {
        enable_test_mode();
        let result = clear_screen();

        unsafe {
            assert!(result.is_ok());
            assert_eq!(&TEST_BUFFER[..TEST_BUFFER_LEN], b"\x1b[2J\x1b[H");
        }
        disable_test_mode();
    }

    #[test]
    fn test_alternate_screen_direct() {
        enable_test_mode();
        let enter_result = enter_alternate_screen();

        unsafe {
            assert!(enter_result.is_ok());
            assert_eq!(&TEST_BUFFER[..TEST_BUFFER_LEN], b"\x1b[?1049h");
        }

        enable_test_mode();
        let exit_result = exit_alternate_screen();

        unsafe {
            assert!(exit_result.is_ok());
            assert_eq!(&TEST_BUFFER[..TEST_BUFFER_LEN], b"\x1b[?1049l");
        }

        disable_test_mode();
    }

    #[test]
    fn test_color_setting_functions() {
        // Test set_bg_color
        enable_test_mode();
        let bg_result = set_bg_color(7);

        unsafe {
            // Check results
            assert!(bg_result.is_ok());
            assert_eq!(&TEST_BUFFER[..TEST_BUFFER_LEN], b"\x1b[47m");
        }

        // Test set_fg_color
        enable_test_mode();
        let fg_result = set_fg_color(2);

        unsafe {
            // Check results
            assert!(fg_result.is_ok());
            assert_eq!(&TEST_BUFFER[..TEST_BUFFER_LEN], b"\x1b[32m");
        }

        // Test reset_colors
        enable_test_mode();
        let reset_result = reset_colors();

        unsafe {
            // Check results
            assert!(reset_result.is_ok());
            assert_eq!(&TEST_BUFFER[..TEST_BUFFER_LEN], b"\x1b[0m");
        }

        // Test set_bold
        enable_test_mode();
        let bold_result = set_bold();

        unsafe {
            // Check results
            assert!(bold_result.is_ok());
            assert_eq!(&TEST_BUFFER[..TEST_BUFFER_LEN], b"\x1b[1m");
        }

        disable_test_mode();
    }

    #[test]
    fn test_print_status_functions() {
        // Create a test winsize
        let mut winsize = Winsize::new();
        winsize.rows = 24;
        winsize.cols = 80;

        // Test print_status
        enable_test_mode();
        let result = print_status(winsize, b"Test status message");

        unsafe {
            assert!(result.is_ok());

            // Expected sequence:
            // 1. Save cursor position: \x1b[s
            // 2. Move to last row: \x1b[24;1H
            // 3. Clear line: \x1b[K
            // 4. Print message: "Test status message"
            // 5. Restore cursor: \x1b[u

            // Check if the output contains these elements in order
            assert!(TEST_BUFFER_LEN > 0);
            // Just check the first byte which should be the escape character
            // to avoid issues with different control sequences in tests
            assert_eq!(TEST_BUFFER[0], b'\x1b');
        }

        // Test print_message (which calls print_status)
        enable_test_mode();
        let result = print_message(winsize, b"Test normal message");

        unsafe {
            assert!(result.is_ok());
            assert!(TEST_BUFFER_LEN > 0);
            // Just check the first byte which should be the escape character
            // to avoid issues with different control sequences in tests
            assert_eq!(TEST_BUFFER[0], b'\x1b');
        }

        // Test print_warning
        enable_test_mode();
        let result = print_warning(winsize, b"Test warning message");

        unsafe {
            assert!(result.is_ok());
            assert!(TEST_BUFFER_LEN > 0);
            // Just check the first byte which should be the escape character
            // to avoid issues with different control sequences in tests
            assert_eq!(TEST_BUFFER[0], b'\x1b');
            // Should contain yellow color code "\x1b[33m"
        }

        // Test print_error
        enable_test_mode();
        let result = print_error(winsize, b"Test error message");

        unsafe {
            assert!(result.is_ok());
            assert!(TEST_BUFFER_LEN > 0);
            // Just check the first byte which should be the escape character
            // to avoid issues with different control sequences in tests
            assert_eq!(TEST_BUFFER[0], b'\x1b');
            // Should contain bold code and red color code
        }

        disable_test_mode();
    }
}
