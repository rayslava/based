use crate::syscall::{STDIN, STDOUT, putchar, puts, read};
use crate::terminal::{
    clear_screen, draw_status_bar, enter_alternate_screen, exit_alternate_screen, get_winsize,
};
use crate::termios::Winsize;

// Function to read one character
fn read_char() -> Option<u8> {
    let mut buf = [0u8; 1];
    match read(STDIN, &mut buf, 1) {
        Ok(n) if n > 0 => Some(buf[0]),
        _ => None,
    }
}

// Define key types
#[derive(Debug, PartialEq)]
enum Key {
    Char(u8),
    ArrowUp,
    ArrowDown,
    ArrowRight,
    ArrowLeft,
    Enter,
    Backspace,
    Quit,
}

// Read a key, handling escape sequences and control characters
fn read_key() -> Option<Key> {
    if let Some(ch) = read_char() {
        match ch {
            // Quit key
            b'q' => Some(Key::Quit),

            // Enter key
            b'\r' => Some(Key::Enter),

            // Backspace
            127 | 8 => Some(Key::Backspace),

            // Escape sequence
            27 => {
                // Detect arrow keys
                if let Some(b'[') = read_char() {
                    if let Some(ch) = read_char() {
                        match ch {
                            b'A' => return Some(Key::ArrowUp),
                            b'B' => return Some(Key::ArrowDown),
                            b'C' => return Some(Key::ArrowRight),
                            b'D' => return Some(Key::ArrowLeft),
                            _ => return Some(Key::Char(ch)),
                        }
                    }
                }
                Some(Key::Char(ch))
            }

            // Emacs key bindings - Control characters
            2 => Some(Key::ArrowLeft),  // C-b (backward-char)
            6 => Some(Key::ArrowRight), // C-f (forward-char)
            14 => Some(Key::ArrowDown), // C-n (next-line)
            16 => Some(Key::ArrowUp),   // C-p (previous-line)

            // Regular character
            _ => Some(Key::Char(ch)),
        }
    } else {
        None
    }
}

#[cfg(not(tarpaulin_include))]
pub fn run_editor() -> Result<(), usize> {
    enter_alternate_screen()?;
    clear_screen()?;

    // Get terminal size
    let mut winsize = Winsize::new();
    get_winsize(STDOUT, &mut winsize)?;

    let mut running = true;

    // Track cursor position
    let mut cursor_row = 0;
    let mut cursor_col = 0;

    // Draw the initial status bar
    draw_status_bar(winsize, cursor_row, cursor_col)?;

    while running {
        if let Some(key) = read_key() {
            match key {
                Key::Quit => running = false,

                Key::Enter => {
                    let _ = puts(b"\r\n");
                    cursor_row += 1;
                    cursor_col = 0;
                }

                Key::Backspace => {
                    if cursor_col > 0 {
                        let _ = puts(b"\x08 \x08");
                        cursor_col -= 1;
                    }
                }

                Key::ArrowUp => {
                    if cursor_row > 0 {
                        let _ = puts(b"\x1b[A"); // Move cursor up
                        cursor_row -= 1;
                    }
                }

                Key::ArrowDown => {
                    let _ = puts(b"\x1b[B"); // Move cursor down
                    cursor_row += 1;
                }

                Key::ArrowRight => {
                    let _ = puts(b"\x1b[C"); // Move cursor right
                    cursor_col += 1;
                }

                Key::ArrowLeft => {
                    if cursor_col > 0 {
                        let _ = puts(b"\x1b[D"); // Move cursor left
                        cursor_col -= 1;
                    }
                }

                Key::Char(ch) => {
                    let _ = putchar(ch);
                    cursor_col += 1;
                }
            }
        }
        draw_status_bar(winsize, cursor_row, cursor_col)?;
    }
    exit_alternate_screen()?;
    Ok(())
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::VecDeque;

    // Thread-local storage for test input
    thread_local! {
        pub static TEST_INPUT: RefCell<VecDeque<u8>> = const { RefCell::new(VecDeque::new()) };
    }

    // Helper to set up test input
    pub fn set_test_input(input: &[u8]) {
        TEST_INPUT.with(|queue| {
            let mut queue = queue.borrow_mut();
            queue.clear();
            for &byte in input {
                queue.push_back(byte);
            }
        });
    }

    // Helper to clear test input
    pub fn clear_test_input() {
        TEST_INPUT.with(|queue| {
            queue.borrow_mut().clear();
        });
    }

    #[test]
    fn test_read_key() {
        struct TestCase {
            name: &'static str,
            input: &'static [u8],
            expected: Option<Key>,
        }

        let test_cases = [
            TestCase {
                name: "regular character",
                input: b"a",
                expected: Some(Key::Char(b'a')),
            },
            TestCase {
                name: "enter key",
                input: b"\r",
                expected: Some(Key::Enter),
            },
            TestCase {
                name: "backspace (127)",
                input: &[127],
                expected: Some(Key::Backspace),
            },
            TestCase {
                name: "backspace (8)",
                input: &[8],
                expected: Some(Key::Backspace),
            },
            TestCase {
                name: "quit key",
                input: b"q",
                expected: Some(Key::Quit),
            },
            TestCase {
                name: "arrow up (escape sequence)",
                input: &[27, b'[', b'A'],
                expected: Some(Key::ArrowUp),
            },
            TestCase {
                name: "arrow down (escape sequence)",
                input: &[27, b'[', b'B'],
                expected: Some(Key::ArrowDown),
            },
            TestCase {
                name: "arrow right (escape sequence)",
                input: &[27, b'[', b'C'],
                expected: Some(Key::ArrowRight),
            },
            TestCase {
                name: "arrow left (escape sequence)",
                input: &[27, b'[', b'D'],
                expected: Some(Key::ArrowLeft),
            },
            TestCase {
                name: "escape followed by other character",
                input: &[27, b'[', b'Z'], // Z is not a special key
                expected: Some(Key::Char(b'Z')),
            },
            TestCase {
                name: "partial escape sequence",
                input: &[27],
                expected: Some(Key::Char(27)),
            },
            TestCase {
                name: "Ctrl+B (left)",
                input: &[2],
                expected: Some(Key::ArrowLeft),
            },
            TestCase {
                name: "Ctrl+F (right)",
                input: &[6],
                expected: Some(Key::ArrowRight),
            },
            TestCase {
                name: "Ctrl+N (down)",
                input: &[14],
                expected: Some(Key::ArrowDown),
            },
            TestCase {
                name: "Ctrl+P (up)",
                input: &[16],
                expected: Some(Key::ArrowUp),
            },
            TestCase {
                name: "no input",
                input: &[],
                expected: None,
            },
        ];

        for tc in test_cases {
            // Set test input
            set_test_input(tc.input);

            // Call the function
            let result = read_key();

            // Assert result
            assert_eq!(
                result, tc.expected,
                "Test case '{}' failed: expected {:?}, got {:?}",
                tc.name, tc.expected, result
            );

            // Clear test input for next test
            clear_test_input();
        }
    }
}
