use crate::syscall::{STDIN, read};

// Function to read one character
fn read_char() -> Option<u8> {
    let mut buf = [0u8; 1];
    match read(STDIN, &mut buf, 1) {
        Ok(n) if n > 0 => Some(buf[0]),
        _ => None,
    }
}

// Define key types
#[derive(PartialEq, Copy, Clone)]
pub(in crate::editor) enum Key {
    Char(u8),
    ArrowUp,
    ArrowDown,
    ArrowRight,
    ArrowLeft,
    Enter,
    Backspace,
    Delete,
    Quit,
    Refresh,
    Home,
    End,
    PageUp,
    PageDown,
    OpenFile,
    SaveFile,
    Search,
    ReverseSearch,
    Escape,
    FirstChar,
    LastChar,
    ExitSearch,
    Combination([u8; 2]),
}

// Process an escape sequence and return the corresponding key
fn process_escape_sequence() -> Key {
    // Read the second character of the escape sequence
    let Some(second_ch) = read_char() else {
        return Key::Escape; // ESC pressed without a sequence
    };

    match second_ch {
        // Alt+< for first character of file
        b'<' => Key::FirstChar,
        // Alt+> for last character of file
        b'>' => Key::LastChar,
        // Alt+v for PageUp (Emacs-style)
        b'v' => Key::PageUp,

        // Standard escape sequences starting with ESC [
        b'[' => {
            // Read the third character of the sequence
            let Some(third_ch) = read_char() else {
                return Key::Char(second_ch);
            };

            match third_ch {
                // Arrow keys
                b'A' => Key::ArrowUp,
                b'B' => Key::ArrowDown,
                b'C' => Key::ArrowRight,
                b'D' => Key::ArrowLeft,
                b'H' => Key::Home, // Home key
                b'F' => Key::End,  // End key

                // Page Up: ESC [ 5 ~
                b'5' => {
                    let Some(fourth_ch) = read_char() else {
                        return Key::Char(third_ch);
                    };

                    if fourth_ch == b'~' {
                        return Key::PageUp;
                    }
                    Key::Char(fourth_ch)
                }

                // Page Down: ESC [ 6 ~
                b'6' => {
                    let Some(fourth_ch) = read_char() else {
                        return Key::Char(third_ch);
                    };

                    if fourth_ch == b'~' {
                        return Key::PageDown;
                    }
                    Key::Char(fourth_ch)
                }

                // Delete key: ESC [ 3 ~
                b'3' => {
                    let Some(fourth_ch) = read_char() else {
                        return Key::Char(third_ch);
                    };

                    if fourth_ch == b'~' {
                        return Key::Delete;
                    }
                    Key::Char(fourth_ch)
                }

                // Home key: ESC [ 1 ~
                b'1' => {
                    let Some(fourth_ch) = read_char() else {
                        return Key::Char(third_ch);
                    };

                    if fourth_ch == b'~' {
                        return Key::Home; // Home key on some terminals
                    } else if fourth_ch == b';' {
                        // Extended keys: ESC [ 1 ; X Y where X is modifier and Y is key
                        // Skip modifier key
                        let _ = read_char();

                        // Read the key code
                        if let Some(code) = read_char() {
                            match code {
                                b'A' => return Key::ArrowUp,
                                b'B' => return Key::ArrowDown,
                                b'C' => return Key::ArrowRight,
                                b'D' => return Key::ArrowLeft,
                                _ => return Key::Char(code),
                            }
                        }
                    }
                    Key::Char(fourth_ch)
                }

                // End key: ESC [ 4 ~
                b'4' => {
                    let Some(fourth_ch) = read_char() else {
                        return Key::Char(third_ch);
                    };

                    if fourth_ch == b'~' {
                        return Key::End; // End key on some terminals
                    }
                    Key::Char(fourth_ch)
                }

                _ => Key::Char(third_ch),
            }
        }

        // Alternative format for xterm/rxvt keys: ESC O X
        b'O' => {
            let Some(third_ch) = read_char() else {
                return Key::Char(second_ch);
            };

            match third_ch {
                b'A' => Key::ArrowUp,    // Up arrow
                b'B' => Key::ArrowDown,  // Down arrow
                b'C' => Key::ArrowRight, // Right arrow
                b'D' => Key::ArrowLeft,  // Left arrow
                b'H' => Key::Home,       // Home
                b'F' => Key::End,        // End
                _ => Key::Char(third_ch),
            }
        }

        // Could not recognize the escape sequence
        _ => Key::Char(second_ch),
    }
}

// Read a key, handling escape sequences and control characters
pub(in crate::editor) fn read_key() -> Option<Key> {
    // Read the first character
    let ch = read_char()?;

    // Handle regular keys
    match ch {
        // Enter key
        b'\r' => Some(Key::Enter),

        // Backspace
        127 | 8 => Some(Key::Backspace),

        // Emacs key bindings - Control characters
        1 => Some(Key::Home),           // C-a (beginning-of-line)
        2 => Some(Key::ArrowLeft),      // C-b (backward-char)
        4 => Some(Key::Delete),         // C-d (delete-char)
        5 => Some(Key::End),            // C-e (end-of-line)
        6 => Some(Key::ArrowRight),     // C-f (forward-char)
        7 => Some(Key::ExitSearch),     // C-g (exit-search-mode)
        12 => Some(Key::Refresh),       // C-l (refresh screen)
        14 => Some(Key::ArrowDown),     // C-n (next-line)
        16 => Some(Key::ArrowUp),       // C-p (previous-line)
        18 => Some(Key::ReverseSearch), // C-r (reverse-search)
        19 => Some(Key::Search),        // C-s (search)
        22 => Some(Key::PageDown),      // C-v (page-down)

        // Ctrl+X prefix for key combinations
        24 => {
            // Ctrl+X
            // Wait for the next key
            if let Some(next_ch) = read_char() {
                // Ctrl+X Ctrl+C (Quit)
                if next_ch == 3 {
                    // Ctrl+X Ctrl+C (Quit)
                    return Some(Key::Quit);
                } else if next_ch == 6 {
                    // Ctrl+X Ctrl+F (Open file)
                    return Some(Key::OpenFile);
                } else if next_ch == 19 {
                    // Ctrl+X Ctrl+S (Save file)
                    return Some(Key::SaveFile);
                }

                // Return the combination
                return Some(Key::Combination([ch, next_ch]));
            }
            Some(Key::Char(ch))
        }

        // Escape sequence or Escape key
        27 => Some(process_escape_sequence()),

        // Regular character
        _ => Some(Key::Char(ch)),
    }
}
