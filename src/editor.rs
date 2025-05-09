use crate::syscall::{
    MAP_PRIVATE, O_RDONLY, PROT_READ, STDIN, STDOUT, SysResult, mmap, open, putchar, read,
};
use crate::terminal::{
    clear_screen, draw_status_bar, enter_alternate_screen, exit_alternate_screen, get_winsize,
    move_cursor, print_error,
};
use crate::termios::Winsize;

enum FileBufferError {
    WrongSize,
}

struct FileBuffer {
    content: *const u8, // Pointer to file content
    size: usize,        // Size of the file
}

impl FileBuffer {
    fn load_from_mmap(addr: usize, size: usize) -> Result<Self, FileBufferError> {
        match (addr, size) {
            (0, _) | (_, 0) => Err(FileBufferError::WrongSize),
            _ => Ok(FileBuffer {
                content: addr as *const u8,
                size,
            }),
        }
    }

    // Count the number of lines in the file
    fn count_lines(&self) -> usize {
        if self.content.is_null() || self.size == 0 {
            return 0;
        }

        let mut count = 1; // Start with 1 for the first line
        for i in 0..self.size {
            let byte = unsafe { *self.content.add(i) };
            if byte == 0 {
                // End of file marker
                break;
            }
            if byte == b'\n' {
                count += 1;
            }
        }
        count
    }

    // Find the start position of a specific line
    fn find_line_start(&self, line_idx: usize) -> Option<usize> {
        if self.content.is_null() || self.size == 0 {
            return None;
        }

        if line_idx == 0 {
            return Some(0); // First line always starts at 0
        }

        let mut current_line = 0;
        let mut pos = 0;

        while pos < self.size {
            let byte = unsafe { *self.content.add(pos) };

            if byte == 0 {
                // End of file marker
                break;
            }

            if byte == b'\n' {
                current_line += 1;
                if current_line == line_idx {
                    return Some(pos + 1); // Start of the next line is after the newline
                }
            }

            pos += 1;
        }

        // If line_idx is beyond the number of lines in the file
        None
    }

    // Find the end position of a specific line (exclusive of newline)
    fn find_line_end(&self, line_idx: usize) -> Option<usize> {
        // Get the start of this line
        let start = self.find_line_start(line_idx)?;

        let mut pos = start;
        while pos < self.size {
            let byte = unsafe { *self.content.add(pos) };

            if byte == 0 || byte == b'\n' {
                // End of line or file
                return Some(pos);
            }

            pos += 1;
        }

        // End of file
        Some(self.size)
    }

    // Get a specific line from the buffer
    fn get_line(&self, line_idx: usize) -> Option<&[u8]> {
        // Find start and end positions of the line
        let start = self.find_line_start(line_idx)?;
        let end = self.find_line_end(line_idx)?;

        if start >= end || start >= self.size || end > self.size {
            return None;
        }

        // Create a slice directly from pointers
        unsafe {
            let start_ptr = self.content.add(start);
            let len = end - start;
            Some(core::slice::from_raw_parts(start_ptr, len))
        }
    }

    // Get a line's length, treating tabs as the specified number of spaces
    fn line_length(&self, line_idx: usize, tab_size: usize) -> usize {
        if let Some(line) = self.get_line(line_idx) {
            let mut length = 0;
            for &byte in line {
                if byte == b'\t' {
                    // Add spaces until the next tab stop
                    let spaces_to_add = tab_size - (length % tab_size);
                    length += spaces_to_add;
                } else if byte == 0 {
                    break; // Stop at null byte
                } else {
                    length += 1;
                }
            }
            length
        } else {
            0
        }
    }
}

// Editor state structure to track view and cursor position
struct EditorState {
    winsize: Winsize,  // Terminal window size
    cursor_row: usize, // Cursor row in the visible window
    cursor_col: usize, // Cursor column in the visible window
    file_row: usize,   // Row in the file (0-based)
    file_col: usize,   // Column in the file (0-based)
    scroll_row: usize, // Top row of the file being displayed
    scroll_col: usize, // Leftmost column being displayed
    tab_size: usize,   // Number of spaces per tab
}

impl EditorState {
    // Create a new editor state
    fn new(winsize: Winsize) -> Self {
        EditorState {
            winsize,
            cursor_row: 0,
            cursor_col: 0,
            file_row: 0,
            file_col: 0,
            scroll_row: 0,
            scroll_col: 0,
            tab_size: 4,
        }
    }

    // Get the number of rows available for editing (excluding status bars)
    fn editing_rows(&self) -> usize {
        if self.winsize.rows >= 2 {
            // Use all rows except status bar and message line (2 rows)
            self.winsize.rows as usize - 2
        } else {
            self.winsize.rows as usize
        }
    }

    // Adjust scrolling to make sure cursor is visible
    fn scroll_to_cursor(&mut self, _file_buffer: &FileBuffer) -> bool {
        let old_scroll_row = self.scroll_row;
        let old_scroll_col = self.scroll_col;

        // If cursor is above the visible area, scroll up
        if self.file_row < self.scroll_row {
            self.scroll_row = self.file_row;
        }

        // If cursor is below the visible area, scroll down
        let max_visible_row = self.scroll_row + self.editing_rows();
        if self.file_row >= max_visible_row {
            self.scroll_row = self.file_row - self.editing_rows() + 1;
        }

        // If cursor is left of visible area, scroll left
        if self.file_col < self.scroll_col {
            self.scroll_col = self.file_col;
        }

        // If cursor is right of visible area, scroll right
        if self.file_col >= self.scroll_col + self.winsize.cols as usize {
            self.scroll_col = self.file_col - self.winsize.cols as usize + 1;
        }

        self.cursor_row = self.file_row - self.scroll_row;
        self.cursor_col = self.file_col - self.scroll_col;

        // Return true if scrolling actually happened
        old_scroll_row != self.scroll_row || old_scroll_col != self.scroll_col
    }

    // Move cursor up
    fn cursor_up(&mut self, file_buffer: &FileBuffer) {
        if self.file_row > 0 {
            self.file_row -= 1;

            // Make sure cursor doesn't go beyond the end of the current line
            let current_line_len = file_buffer.line_length(self.file_row, self.tab_size);
            if self.file_col > current_line_len {
                self.file_col = current_line_len;
            }

            // Note: scroll_to_cursor is now called by the key handler
        }
    }

    // Move cursor down
    fn cursor_down(&mut self, file_buffer: &FileBuffer) {
        let line_count = file_buffer.count_lines();
        if self.file_row + 1 < line_count {
            self.file_row += 1;

            // Make sure cursor doesn't go beyond the end of the current line
            let current_line_len = file_buffer.line_length(self.file_row, self.tab_size);
            if self.file_col > current_line_len {
                self.file_col = current_line_len;
            }

            // Note: scroll_to_cursor is now called by the key handler
        }
    }

    // Move cursor left
    fn cursor_left(&mut self, file_buffer: &FileBuffer) {
        if self.file_col > 0 {
            self.file_col -= 1;
            // Note: scroll_to_cursor is now called by the key handler
        } else if self.file_row > 0 {
            // At beginning of line, move to end of previous line
            self.file_row -= 1;
            self.file_col = file_buffer.line_length(self.file_row, self.tab_size);
            // Note: scroll_to_cursor is now called by the key handler
        }
    }

    // Move cursor right
    fn cursor_right(&mut self, file_buffer: &FileBuffer) {
        let current_line_len = file_buffer.line_length(self.file_row, self.tab_size);
        let line_count = file_buffer.count_lines();

        if self.file_col < current_line_len {
            self.file_col += 1;
            // Note: scroll_to_cursor is now called by the key handler
        } else if self.file_row + 1 < line_count {
            // At end of line, move to beginning of next line
            self.file_row += 1;
            self.file_col = 0;
            // Note: scroll_to_cursor is now called by the key handler
        }
    }
}

// Draw the file content on the screen
fn draw_screen(state: &EditorState, file_buffer: &FileBuffer, clear: bool) -> SysResult {
    // Only clear and redraw when necessary (scrolling occurred)
    if clear {
        // Clear the screen
        clear_screen()?;

        // Calculate available height for content
        let available_rows = state.editing_rows();
        // Convert to usize for iterator
        let line_count = file_buffer.count_lines();

        // Draw lines from the file buffer
        // Using a manually bounded loop to avoid clippy warnings
        for i in 0..available_rows {
            // Position cursor at start of each line
            // We know i < available_rows_usize which came from a u16, so we can safely convert back
            move_cursor(i, 0)?;

            let file_line_idx = state.scroll_row + i;

            if file_line_idx >= line_count {
                // We're past the end of file, leave the rest of screen empty
                continue;
            }

            // Get the line from file buffer
            if let Some(line) = file_buffer.get_line(file_line_idx) {
                // Skip lines with only newlines
                if line.is_empty() {
                    continue;
                }

                // Calculate how much to skip from the start (for horizontal scrolling)
                let mut chars_to_skip = state.scroll_col;
                let mut col = 0;

                // Display each character in the line
                for &byte in line {
                    if byte == 0 {
                        // Stop at null byte
                        break;
                    }

                    if byte == b'\t' {
                        // Handle tabs - convert to spaces
                        let spaces = state.tab_size - (col % state.tab_size);
                        col += spaces;

                        // Skip if we're still scrolled horizontally
                        if chars_to_skip > 0 {
                            if chars_to_skip >= spaces {
                                chars_to_skip -= spaces;
                            } else {
                                // Draw partial spaces after the horizontal scroll point
                                for _ in 0..(spaces - chars_to_skip) {
                                    putchar(b' ')?;
                                }
                                chars_to_skip = 0;
                            }
                        } else {
                            // Draw spaces for tab
                            for _ in 0..spaces {
                                putchar(b' ')?;
                            }
                        }
                    } else {
                        col += 1;

                        // Only print if we've scrolled past the horizontal skip point
                        if chars_to_skip > 0 {
                            chars_to_skip -= 1;
                        } else {
                            putchar(byte)?;
                        }
                    }

                    // Stop if we reach the edge of the screen
                    if col - state.scroll_col >= state.winsize.cols as usize {
                        break;
                    }
                }

                // No newlines needed since we're positioning cursor for each line
            }
        }
    }

    // Always position cursor, regardless of whether we redrew the screen
    move_cursor(state.cursor_row, state.cursor_col)?;

    Ok(0)
}

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

pub enum EditorError {
    OpenFile,
    LoadFile,
    MMapFile,
    FileBuffer,
    SysError(usize),
}

impl From<usize> for EditorError {
    fn from(error_code: usize) -> Self {
        EditorError::SysError(error_code)
    }
}

impl From<FileBufferError> for EditorError {
    fn from(_: FileBufferError) -> Self {
        EditorError::FileBuffer
    }
}

#[cfg(not(tarpaulin_include))]
pub fn run_editor() -> Result<(), EditorError> {
    // Enter alternate screen and ensure it's clear

    use crate::terminal::print_message;
    enter_alternate_screen()?;
    clear_screen()?;

    // Get terminal size
    let mut winsize = Winsize::new();
    get_winsize(STDOUT, &mut winsize)?;

    // Create editor state
    let mut state = EditorState::new(winsize);
    let file_path = b"file.txt\0";

    let file_buffer: FileBuffer = {
        let Ok(fd) = open(file_path, O_RDONLY) else {
            print_error(winsize, b"Error: Failed to open file.txt")?;
            return Err(EditorError::OpenFile);
        };

        if fd == 0 {
            print_error(winsize, b"Error: Failed to open file.txt")?;
            return Err(EditorError::LoadFile);
        }

        let max_file_size = 1024 * 1024; // 1MB, enough for now

        let Ok(addr) = mmap(0, max_file_size, PROT_READ, MAP_PRIVATE, fd, 0) else {
            print_error(winsize, b"Error: Failed to load file content")?;
            return Err(EditorError::MMapFile);
        };

        FileBuffer::load_from_mmap(addr, max_file_size)?
    };

    draw_screen(&state, &file_buffer, true)?;
    draw_status_bar(state.winsize, state.cursor_row, state.cursor_col)?;
    print_message(winsize, b"File opened successfully")?;

    let mut running = true;

    while running {
        if let Some(key) = read_key() {
            match key {
                Key::Quit => running = false,

                Key::ArrowUp => {
                    state.cursor_up(&file_buffer);
                    // Only clear screen if scrolling occurred
                    let did_scroll = state.scroll_to_cursor(&file_buffer);
                    draw_screen(&state, &file_buffer, did_scroll)?;
                    // Print debug info
                }

                Key::ArrowDown => {
                    state.cursor_down(&file_buffer);
                    // Only clear screen if scrolling occurred
                    let did_scroll = state.scroll_to_cursor(&file_buffer);
                    draw_screen(&state, &file_buffer, did_scroll)?;
                    // Add debug info for all key presses
                }

                Key::ArrowRight => {
                    state.cursor_right(&file_buffer);
                    // Only clear screen if scrolling occurred
                    let did_scroll = state.scroll_to_cursor(&file_buffer);
                    draw_screen(&state, &file_buffer, did_scroll)?;
                    // Add debug info for all key presses
                }

                Key::ArrowLeft => {
                    state.cursor_left(&file_buffer);
                    // Only clear screen if scrolling occurred
                    let did_scroll = state.scroll_to_cursor(&file_buffer);
                    draw_screen(&state, &file_buffer, did_scroll)?;
                    // Add debug info for all key presses
                }

                Key::Enter => {
                    // In a full editor, this would insert a newline
                    // For now, we just move the cursor down and to column 0
                    let line_count = file_buffer.count_lines();
                    if state.file_row + 1 < line_count {
                        state.file_row += 1;
                        state.file_col = 0;
                        let did_scroll = state.scroll_to_cursor(&file_buffer);
                        // Only clear screen if scrolling occurred
                        draw_screen(&state, &file_buffer, did_scroll)?;
                        // Add debug info for all key presses
                    }
                }

                Key::Backspace => {
                    // In a full editor, this would delete characters
                    // For now, just move the cursor left
                    if state.file_col > 0 {
                        state.file_col -= 1;
                        let did_scroll = state.scroll_to_cursor(&file_buffer);
                        // Only clear screen if scrolling occurred
                        draw_screen(&state, &file_buffer, did_scroll)?;
                        // Add debug info for all key presses
                    }
                }

                Key::Char(_) => {
                    // In a full editor, this would insert characters
                    // For now, we're just viewing the file, so we ignore character input
                }
            }
        }
        draw_status_bar(state.winsize, state.file_row, state.file_col)?;
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
