use crate::syscall::{
    MAP_PRIVATE, O_RDONLY, PROT_READ, SEEK_END, SEEK_SET, STDIN, STDOUT, SysResult, close, lseek,
    mmap, open, putchar, read,
};
use crate::terminal::{
    clear_line, clear_screen, draw_status_bar, enter_alternate_screen, exit_alternate_screen,
    get_winsize, move_cursor, print_error, print_message,
};
use crate::{
    syscall::write_buf,
    terminal::{restore_cursor, save_cursor},
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
    fn scroll_to_cursor(&mut self) {
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

    // Move cursor to beginning of line (Home/Ctrl+A)
    fn cursor_home(&mut self) {
        self.file_col = 0;
        // Note: scroll_to_cursor is now called by the key handler
    }

    // Move cursor to end of line (End/Ctrl+E)
    fn cursor_end(&mut self, file_buffer: &FileBuffer) {
        self.file_col = file_buffer.line_length(self.file_row, self.tab_size);
        // Note: scroll_to_cursor is now called by the key handler
    }

    // Page up (Alt+V): move cursor up by a screen's worth of lines
    fn page_up(&mut self, file_buffer: &FileBuffer) {
        // Get the number of lines to scroll (screen height)
        let lines_to_scroll = self.editing_rows();

        // First update scroll position
        if self.scroll_row >= lines_to_scroll {
            self.scroll_row -= lines_to_scroll;
        } else {
            self.scroll_row = 0;
        }

        // Then update cursor position
        if self.file_row >= lines_to_scroll {
            self.file_row -= lines_to_scroll;
        } else {
            self.file_row = 0;
        }

        // Make sure cursor doesn't go beyond the end of the current line
        let current_line_len = file_buffer.line_length(self.file_row, self.tab_size);
        if self.file_col > current_line_len {
            self.file_col = current_line_len;
        }

        // Update cursor position based on scroll
        self.cursor_row = self.file_row - self.scroll_row;
    }

    // Page down (Ctrl+V): move cursor down by a screen's worth of lines
    fn page_down(&mut self, file_buffer: &FileBuffer) {
        // Get the number of lines to scroll (screen height)
        let lines_to_scroll = self.editing_rows();
        let line_count = file_buffer.count_lines();

        // Update cursor position, but don't go beyond the end of file
        if self.file_row + lines_to_scroll < line_count {
            self.file_row += lines_to_scroll;
        } else {
            self.file_row = line_count - 1;
        }

        // Update scroll position
        let max_scroll_row = self.file_row - self.editing_rows() + 1;
        if max_scroll_row > 0 {
            self.scroll_row = max_scroll_row;
        }

        // Make sure cursor doesn't go beyond the end of the current line
        let current_line_len = file_buffer.line_length(self.file_row, self.tab_size);
        if self.file_col > current_line_len {
            self.file_col = current_line_len;
        }

        // Update cursor position based on scroll
        self.cursor_row = self.file_row - self.scroll_row;
    }
}

fn draw_screen(state: &EditorState, file_buffer: &FileBuffer) -> SysResult {
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

        clear_line()?;
        let file_line_idx = state.scroll_row + i;

        if file_line_idx >= line_count {
            // We're past the end of file, leave the rest of screen empty
            continue;
        }

        // Get the line from file buffer
        if let Some(line) = file_buffer.get_line(file_line_idx) {
            if line.is_empty() {
                continue;
            }

            // Calculate how much to skip from the start (for horizontal scrolling)
            let mut chars_to_skip = state.scroll_col;
            let mut col = 0;
            let mut screen_col = 0;

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
                            let visible_spaces = spaces - chars_to_skip;
                            for _ in 0..visible_spaces {
                                if screen_col < state.winsize.cols as usize {
                                    putchar(b' ')?;
                                    screen_col += 1;
                                } else {
                                    break;
                                }
                            }
                            chars_to_skip = 0;
                        }
                    } else {
                        // Draw spaces for tab
                        for _ in 0..spaces {
                            if screen_col < state.winsize.cols as usize {
                                putchar(b' ')?;
                                screen_col += 1;
                            } else {
                                break;
                            }
                        }
                    }
                } else {
                    col += 1;

                    // Only print if we've scrolled past the horizontal skip point
                    if chars_to_skip > 0 {
                        chars_to_skip -= 1;
                    } else if screen_col < state.winsize.cols as usize {
                        putchar(byte)?;
                        screen_col += 1;
                    } else {
                        break;
                    }
                }
            }
        }
    }
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
#[derive(PartialEq, Copy, Clone)]
enum Key {
    Char(u8),
    ArrowUp,
    ArrowDown,
    ArrowRight,
    ArrowLeft,
    Enter,
    Backspace,
    Quit,
    Refresh,
    Home,
    End,
    PageUp,
    PageDown,
    OpenFile,
    Combination([u8; 2]),
}

// Process an escape sequence and return the corresponding key
fn process_escape_sequence() -> Key {
    // Read the second character of the escape sequence
    let Some(second_ch) = read_char() else {
        return Key::Char(27);
    };

    match second_ch {
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
fn read_key() -> Option<Key> {
    // Read the first character
    let ch = read_char()?;

    // Handle regular keys
    match ch {
        // Enter key
        b'\r' => Some(Key::Enter),

        // Backspace
        127 | 8 => Some(Key::Backspace),

        // Emacs key bindings - Control characters
        1 => Some(Key::Home),       // C-a (beginning-of-line)
        2 => Some(Key::ArrowLeft),  // C-b (backward-char)
        5 => Some(Key::End),        // C-e (end-of-line)
        6 => Some(Key::ArrowRight), // C-f (forward-char)
        12 => Some(Key::Refresh),   // C-l (refresh screen)
        14 => Some(Key::ArrowDown), // C-n (next-line)
        16 => Some(Key::ArrowUp),   // C-p (previous-line)
        22 => Some(Key::PageDown),  // C-v (page-down)

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
                }

                // Return the combination
                return Some(Key::Combination([ch, next_ch]));
            }
            Some(Key::Char(ch))
        }

        // Escape sequence
        27 => Some(process_escape_sequence()),

        // Regular character
        _ => Some(Key::Char(ch)),
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

fn open_file(file_path: &[u8]) -> Result<FileBuffer, EditorError> {
    let Ok(fd) = open(file_path, O_RDONLY) else {
        return Err(EditorError::OpenFile);
    };

    if fd == 0 {
        return Err(EditorError::LoadFile);
    }
    let file_size = lseek(fd, 0, SEEK_END)?;
    lseek(fd, 0, SEEK_SET)?;

    let (addr, size) = if file_size == 0 {
        static EMPTY: u8 = 0;
        close(fd)?;
        (&raw const EMPTY as usize, 0)
    } else {
        let Ok(addr) = mmap(0, file_size, PROT_READ, MAP_PRIVATE, fd, 0) else {
            return Err(EditorError::MMapFile);
        };
        (addr, file_size)
    };
    Ok(FileBuffer::load_from_mmap(addr, size)?)
}

#[cfg(not(tarpaulin_include))]
fn process_cursor_key(key: Key, state: &mut EditorState, file_buffer: &FileBuffer) -> SysResult {
    match key {
        Key::ArrowUp => state.cursor_up(file_buffer),
        Key::ArrowDown => state.cursor_down(file_buffer),
        Key::ArrowLeft => state.cursor_left(file_buffer),
        Key::ArrowRight => state.cursor_right(file_buffer),
        Key::Home => state.cursor_home(),
        Key::End => state.cursor_end(file_buffer),
        Key::PageUp => state.page_up(file_buffer),
        Key::PageDown => state.page_down(file_buffer),
        Key::Enter => {
            let line_count = file_buffer.count_lines();
            if state.file_row + 1 < line_count {
                state.file_row += 1;
                state.file_col = 0;
            }
        }
        Key::Backspace => {
            if state.file_col > 0 {
                state.file_col -= 1;
            }
        }
        _ => return Ok(0),
    }
    state.scroll_to_cursor();
    draw_screen(state, file_buffer)
}

#[cfg(not(tarpaulin_include))]
fn handle_open_file(state: &mut EditorState) -> Result<FileBuffer, EditorError> {
    save_cursor()?;
    let prompt: &str = "Enter filename: ";
    print_message(state.winsize, prompt)?;
    move_cursor(state.winsize.rows as usize - 1, prompt.len())?;

    let mut filename: [u8; 64] = [0; 64];
    let mut len: usize = 0;
    loop {
        if let Some(key) = read_key() {
            match key {
                Key::Enter if len > 0 => {
                    filename[len] = 0;
                    break;
                }
                Key::Char(ch) if len < 62 && ch.is_ascii_graphic() || ch == b' ' => {
                    filename[len] = ch;
                    len += 1;
                    putchar(ch)?;
                }
                Key::Backspace if len > 0 => {
                    len -= 1;
                    move_cursor(state.winsize.rows as usize - 1, prompt.len())?;
                    write_buf(&filename[..len])?;
                    clear_line()?;
                }
                _ => {}
            }
        }
    }
    move_cursor(state.winsize.rows as usize - 1, 0)?;
    clear_line()?;
    restore_cursor()?;

    match open_file(&filename) {
        Ok(new_buffer) => {
            state.file_row = 0;
            state.file_col = 0;
            state.scroll_row = 0;
            state.scroll_col = 0;

            clear_screen()?;
            draw_screen(state, &new_buffer)?;
            print_message(state.winsize, "File opened successfully")?;
            move_cursor(0, 0)?;
            Ok(new_buffer)
        }
        Err(e) => {
            print_error(state.winsize, "Error: Failed to open file")?;
            Err(e)
        }
    }
}

pub fn run_editor() -> Result<(), EditorError> {
    enter_alternate_screen()?;
    clear_screen()?;

    let mut winsize = Winsize::new();
    get_winsize(STDOUT, &mut winsize)?;

    let mut state = EditorState::new(winsize);
    // Use a static const for the filename to avoid any potential memory issues
    let file_path = b"file.txt\0";
    let mut file_buffer = match open_file(file_path) {
        Ok(file_buffer) => file_buffer,
        Err(e) => {
            print_error(winsize, "Error: Failed to open file")?;
            return Err(e);
        }
    };

    draw_screen(&state, &file_buffer)?;
    draw_status_bar(state.winsize, state.cursor_row, state.cursor_col)?;
    print_message(winsize, "File opened successfully")?;

    let mut running = true;
    while running {
        if let Some(key) = read_key() {
            match key {
                Key::Quit => running = false,
                Key::Refresh => {
                    clear_screen()?;
                    draw_screen(&state, &file_buffer)?;
                }
                Key::OpenFile => {
                    if let Ok(buf) = handle_open_file(&mut state) {
                        file_buffer = buf;
                    }
                }
                Key::ArrowUp
                | Key::ArrowDown
                | Key::ArrowLeft
                | Key::ArrowRight
                | Key::Home
                | Key::End
                | Key::PageUp
                | Key::PageDown
                | Key::Enter
                | Key::Backspace => {
                    process_cursor_key(key, &mut state, &file_buffer)?;
                }
                Key::Char(_) | Key::Combination(_) => {}
            }
        }
        draw_status_bar(state.winsize, state.file_row, state.file_col)?;
    }

    exit_alternate_screen()?;
    Ok(())
}

#[cfg(test)]
pub mod tests {
    #[cfg(test)]
    pub const _: usize = 0;

    use super::*;
    use crate::terminal::tests::{disable_test_mode, enable_test_mode};

    // Helper function for testing
    fn is_error(result: usize) -> bool {
        const MAX_ERRNO: usize = 4095;
        result > usize::MAX - MAX_ERRNO
    }

    #[test]
    fn test_open_file() {
        use crate::syscall::{close, write};

        // Create test file using the syscalls directly
        // Define required flags for file operations
        const O_WRONLY: usize = 1;
        const O_CREAT: usize = 64;
        const O_TRUNC: usize = 512;

        // Test creating and writing to file.txt directly through syscalls
        let test_content = b"Test file content\nSecond line\n";
        let test_file = b"file.txt\0";

        // Create the test file with content
        let result = unsafe {
            syscall!(
                OPEN,
                test_file.as_ptr(),
                O_WRONLY | O_CREAT | O_TRUNC,
                0o666
            )
        };
        let fd = if is_error(result) { 0 } else { result };
        assert!(fd > 0, "Failed to create test file");

        // Write test content to the file
        let write_result = write(fd, test_content);
        assert!(write_result.is_ok(), "Failed to write to test file");

        // Close the file
        let close_result = close(fd);
        assert!(close_result.is_ok(), "Failed to close test file");

        // Test our open_file function with the file we just created
        let file_path = b"file.txt\0";
        let result = open_file(file_path);
        assert!(
            result.is_ok(),
            "open_file should successfully open a valid file"
        );

        // Verify the returned buffer has the expected content
        if let Ok(buffer) = result {
            // Test buffer methods
            let lines = buffer.count_lines();
            // The content has 2 newlines which creates 3 lines
            assert_eq!(lines, 3, "File should have exactly 3 lines");

            // Test finding line start
            let start = buffer.find_line_start(0);
            assert_eq!(start, Some(0), "First line should start at position 0");

            let second_line_start = buffer.find_line_start(1);
            assert!(
                second_line_start.is_some(),
                "Should find start of second line"
            );

            // Test finding line end
            let end = buffer.find_line_end(0);
            assert!(end.is_some(), "Should find end of first line");

            // Verify we can get line content
            let line = buffer.get_line(0);
            assert!(line.is_some(), "Should get first line content");

            // Test line length calculation
            let line_length = buffer.line_length(0, 4); // tab_size=4
            assert_eq!(line_length, 17, "First line length should be 17");
        }

        // Test with a nonexistent file path
        let invalid_path = b"nonexistent_file.txt\0";
        let result = open_file(invalid_path);
        assert!(result.is_err(), "Should return error for nonexistent file");
        match result {
            Err(EditorError::OpenFile) => (), // Expected error
            _ => panic!("Expected OpenFile error for nonexistent file"),
        }
    }

    #[test]
    fn test_handle_open_file() {
        // Create a test environment
        enable_test_mode();

        // Set up a mock editor state
        let mut winsize = Winsize::new();
        winsize.rows = 24;
        winsize.cols = 80;
        let _state = EditorState::new(winsize); // Prefixed with _ to avoid unused variable warning

        // For this test, we need to ensure handle_open_file's read_key calls
        // would receive the expected input. In a real implementation, we would
        // inject a mock read_key function, but for now we'll verify components.

        // Verify that terminal functions used by handle_open_file work
        let save_result = save_cursor();
        assert!(save_result.is_ok(), "save_cursor should work in test mode");

        let message_result = print_message(winsize, "Test message");
        assert!(
            message_result.is_ok(),
            "print_message should work in test mode"
        );

        // Verify that we can move the cursor as handle_open_file would
        let move_result = move_cursor(winsize.rows as usize - 1, 14); // "Enter filename: ".len()
        assert!(move_result.is_ok(), "move_cursor should work in test mode");

        // Verify that open_file called with a valid path works
        let open_result = open_file(b"file.txt\0");
        assert!(
            open_result.is_ok(),
            "open_file should succeed with valid file"
        );

        // Since we can't fully mock read_key, we're testing the components
        // that handle_open_file uses rather than calling it directly.
        // In a real implementation with dependency injection or function pointers,
        // we would override read_key with a mock function.

        // The full test for handle_open_file would then call:
        // - Let mock function return "file.txt" followed by Enter
        // - Call handle_open_file(&mut state)
        // - Verify it returns Ok and the buffer contains the expected file

        // Clean up
        disable_test_mode();
    }
}
