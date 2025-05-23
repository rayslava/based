use crate::syscall::{STDIN, read};

fn read_char() -> Option<u8> {
    let mut buf = [0u8; 1];
    match read(STDIN, &mut buf, 1) {
        Ok(n) if n > 0 => Some(buf[0]),
        _ => None,
    }
}

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
    OpenLine,
    WordForward,
    WordBackward,
    ToggleCase,
    SetMark,  // For selecting text with Ctrl+Space
    Cut,      // Cut selected text with Ctrl+w
    Copy,     // Copy selected text with Alt+w
    Paste,    // Paste text with Ctrl+y
    KillLine, // Kill to end of line with Ctrl+k
    Combination([u8; 2]),
}

fn process_escape_sequence() -> Key {
    let Some(second_ch) = read_char() else {
        return Key::Escape; // ESC pressed without a sequence
    };

    match second_ch {
        b'<' => Key::FirstChar,
        b'>' => Key::LastChar,
        b'v' => Key::PageUp,
        b'f' => Key::WordForward,
        b'b' => Key::WordBackward,
        b'c' => Key::ToggleCase,
        b'w' => Key::Copy,

        b'[' => {
            let Some(third_ch) = read_char() else {
                return Key::Char(second_ch);
            };

            match third_ch {
                b'A' => Key::ArrowUp,
                b'B' => Key::ArrowDown,
                b'C' => Key::ArrowRight,
                b'D' => Key::ArrowLeft,
                b'H' => Key::Home, // Home key
                b'F' => Key::End,  // End key

                b'5' => {
                    let Some(fourth_ch) = read_char() else {
                        return Key::Char(third_ch);
                    };

                    if fourth_ch == b'~' {
                        return Key::PageUp;
                    }
                    Key::Char(fourth_ch)
                }

                b'6' => {
                    let Some(fourth_ch) = read_char() else {
                        return Key::Char(third_ch);
                    };

                    if fourth_ch == b'~' {
                        return Key::PageDown;
                    }
                    Key::Char(fourth_ch)
                }

                b'3' => {
                    let Some(fourth_ch) = read_char() else {
                        return Key::Char(third_ch);
                    };

                    if fourth_ch == b'~' {
                        return Key::Delete;
                    }
                    Key::Char(fourth_ch)
                }

                b'1' => {
                    let Some(fourth_ch) = read_char() else {
                        return Key::Char(third_ch);
                    };

                    if fourth_ch == b'~' {
                        return Key::Home; // Home key on some terminals
                    } else if fourth_ch == b';' {
                        let _ = read_char();

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

        _ => Key::Char(second_ch),
    }
}

pub(in crate::editor) fn read_key() -> Option<Key> {
    let ch = read_char()?;

    match ch {
        b'\r' => Some(Key::Enter),

        127 | 8 => Some(Key::Backspace),

        1 => Some(Key::Home),           // C-a (beginning-of-line)
        2 => Some(Key::ArrowLeft),      // C-b (backward-char)
        4 => Some(Key::Delete),         // C-d (delete-char)
        5 => Some(Key::End),            // C-e (end-of-line)
        6 => Some(Key::ArrowRight),     // C-f (forward-char)
        7 => Some(Key::ExitSearch),     // C-g (exit-search-mode)
        11 => Some(Key::KillLine),      // C-k (kill-line)
        12 => Some(Key::Refresh),       // C-l (refresh screen)
        14 => Some(Key::ArrowDown),     // C-n (next-line)
        15 => Some(Key::OpenLine),      // C-o (open-line)
        16 => Some(Key::ArrowUp),       // C-p (previous-line)
        18 => Some(Key::ReverseSearch), // C-r (reverse-search)
        19 => Some(Key::Search),        // C-s (search)
        22 => Some(Key::PageDown),      // C-v (page-down)
        23 => Some(Key::Cut),           // C-w (kill-region/cut)
        25 => Some(Key::Paste),         // C-y (yank/paste)
        0 => Some(Key::SetMark),        // C-space (set-mark, ASCII 0 = NUL)

        24 => {
            if let Some(next_ch) = read_char() {
                if next_ch == 3 {
                    return Some(Key::Quit);
                } else if next_ch == 6 {
                    return Some(Key::OpenFile);
                } else if next_ch == 19 {
                    return Some(Key::SaveFile);
                }

                return Some(Key::Combination([ch, next_ch]));
            }
            Some(Key::Char(ch))
        }

        27 => Some(process_escape_sequence()),

        _ => Some(Key::Char(ch)),
    }
}
