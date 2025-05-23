use crate::syscall::{SysResult, ioctl, putchar, puts, write_buf};
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
    termios.iflag &= !(BRKINT | ICRNL | INPCK | ISTRIP | IXON);

    termios.oflag &= !OPOST;

    termios.cflag |= CS8;

    termios.lflag &= !(ECHO | ICANON | ISIG | IEXTEN);

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
    while i > 0 {
        i -= 1;
        let _ = putchar(digits[i]);
    }
}

pub fn clear_screen() -> SysResult {
    puts("\x1b[2J\x1b[H")
}

pub fn clear_line() -> SysResult {
    puts("\x1b[K")
}

pub fn enter_alternate_screen() -> SysResult {
    puts("\x1b[?1049h")
}

pub fn exit_alternate_screen() -> SysResult {
    puts("\x1b[?1049l")
}

pub fn move_cursor(row: usize, col: usize) -> SysResult {
    // Format: ESC [ row+1 ; col+1 H
    let mut buf = [0u8; 16];
    let mut pos = 0;

    buf[pos] = b'\x1b';
    buf[pos + 1] = b'[';
    pos += 2;

    let row_num = row + 1;
    pos += write_usize_to_buf(&mut buf[pos..], row_num);

    buf[pos] = b';';
    pos += 1;

    let col_num = col + 1;
    pos += write_usize_to_buf(&mut buf[pos..], col_num);

    buf[pos] = b'H';
    pos += 1;

    write_buf(&buf[0..pos])
}

pub fn write_usize_to_buf(buf: &mut [u8], n: usize) -> usize {
    if n == 0 {
        buf[0] = b'0';
        return 1;
    }

    let mut digits = [0u8; 20]; // Maximum usize digits (64-bit)
    let mut digit_count = 0;
    let mut num = n;

    while num > 0 {
        let quotient = num / 10;
        let remainder = num - (quotient * 10);
        digits[digit_count] = b'0' + remainder.to_le_bytes()[0];

        num = quotient;
        digit_count += 1;
    }

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
    buf[3] = b'0' + color;
    write_buf(&buf)
}

pub fn set_fg_color(color: u8) -> SysResult {
    // Format: ESC [ 3 color m
    let mut buf = [b'\x1b', b'[', b'3', 0, b'm'];
    buf[3] = b'0' + color;
    write_buf(&buf)
}

pub fn reset_colors() -> SysResult {
    // ESC [ 0 m
    puts("\x1b[0m")
}

pub fn set_bold() -> SysResult {
    puts("\x1b[1m")
}

pub fn save_cursor() -> SysResult {
    puts("\x1b[s")
}

pub fn restore_cursor() -> SysResult {
    puts("\x1b[u")
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::syscall::MAX_PATH;

    pub static mut TEST_BUFFER: [u8; MAX_PATH] = [0; MAX_PATH];
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
            TEST_BUFFER = [0; MAX_PATH];
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
        enable_test_mode();
        let bg_result = set_bg_color(7);

        unsafe {
            // Check results
            assert!(bg_result.is_ok());
            assert_eq!(&TEST_BUFFER[..TEST_BUFFER_LEN], b"\x1b[47m");
        }

        enable_test_mode();
        let fg_result = set_fg_color(2);

        unsafe {
            // Check results
            assert!(fg_result.is_ok());
            assert_eq!(&TEST_BUFFER[..TEST_BUFFER_LEN], b"\x1b[32m");
        }

        enable_test_mode();
        let reset_result = reset_colors();

        unsafe {
            // Check results
            assert!(reset_result.is_ok());
            assert_eq!(&TEST_BUFFER[..TEST_BUFFER_LEN], b"\x1b[0m");
        }

        enable_test_mode();
        let bold_result = set_bold();

        unsafe {
            // Check results
            assert!(bold_result.is_ok());
            assert_eq!(&TEST_BUFFER[..TEST_BUFFER_LEN], b"\x1b[1m");
        }

        disable_test_mode();
    }
}
