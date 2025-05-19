mod editor_state;
mod file_buffer;
mod key_handlers;
mod search_state;

pub(in crate::editor) use editor_state::EditorState;
pub(in crate::editor) use file_buffer::{FileBuffer, FileBufferError};
pub(in crate::editor) use key_handlers::{Key, read_key};
pub(in crate::editor) use search_state::SearchState;

use crate::syscall::{
    MAP_ANONYMOUS, MAP_PRIVATE, O_RDONLY, PROT_READ, PROT_WRITE, SEEK_END, SEEK_SET, STDOUT, close,
    lseek, mmap, open,
};
use crate::syscall::{SysResult, read};
use crate::terminal::move_cursor;
use crate::terminal::{
    clear_screen, enter_alternate_screen, exit_alternate_screen, get_winsize, save_cursor,
};
use crate::termios::Winsize;
use crate::{
    syscall::{MAX_PATH, putchar, write_buf},
    terminal::clear_line,
};

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

    let buffer = {
        if file_size == 0 {
            close(fd)?;
            let new_capacity = 4096; // Start with one page
            let prot = PROT_READ | PROT_WRITE;
            let flags = MAP_PRIVATE | MAP_ANONYMOUS;
            let Ok(new_buffer) = mmap(0, new_capacity, prot, flags, usize::MAX, 0) else {
                return Err(FileBufferError::WrongSize.into());
            };

            FileBuffer {
                content: new_buffer as *mut u8,
                size: 0,
                capacity: new_capacity,
                modified: true,
            }
        } else {
            let new_capacity = file_size;
            // Round up to the next page
            let prot = PROT_READ | PROT_WRITE;
            let flags = MAP_PRIVATE;
            let Ok(new_buffer) = mmap(0, new_capacity, prot, flags, fd, 0) else {
                return Err(EditorError::MMapFile);
            };

            FileBuffer {
                content: new_buffer as *mut u8,
                size: file_size,
                capacity: new_capacity,
                modified: false,
            }
        }
    };
    Ok(buffer)
}

#[cfg(not(tarpaulin_include))]
fn process_cursor_key(key: Key, state: &mut EditorState) -> SysResult {
    // Skip normal processing if in search mode

    if state.search.mode {
        return Ok(0);
    }

    match key {
        Key::ArrowUp => state.cursor_up(),
        Key::ArrowDown => state.cursor_down(),
        Key::ArrowLeft => state.cursor_left(),
        Key::ArrowRight => state.cursor_right(),
        Key::Home => state.cursor_home(),
        Key::End => state.cursor_end(),
        Key::PageUp => state.page_up(),
        Key::PageDown => state.page_down(),
        Key::FirstChar => state.cursor_first_char(),
        Key::LastChar => state.cursor_last_char(),
        Key::Enter => {
            // Insert a newline at the current cursor position
            if let Err(e) = state.buffer.insert_newline(state.file_row, state.file_col) {
                state.print_error(match e {
                    FileBufferError::BufferFull => "Buffer is full",
                    _ => "Failed to insert newline",
                })?;
                return Ok(0);
            }

            // Move cursor to beginning of next line
            state.file_row += 1;
            state.file_col = 0;
        }
        Key::Backspace => {
            // Delete the character before the cursor
            if state.file_col > 0 || state.file_row > 0 {
                // Try to delete the character
                if let Err(e) = state.buffer.backspace_at(state.file_row, state.file_col) {
                    state.print_error(match e {
                        FileBufferError::InvalidOperation => "Can't delete at this position",
                        _ => "Error deleting character",
                    })?;
                    return Ok(0);
                }

                // Update cursor position
                if state.file_col > 0 {
                    state.file_col -= 1;
                } else if state.file_row > 0 {
                    // We've joined the current line with the previous one
                    // Move cursor to the end of the previous line
                    state.file_row -= 1;
                    state.file_col = state.buffer.line_length(state.file_row, state.tab_size);
                }
            }
        }
        Key::Delete => {
            // Delete the character at the cursor position
            let line_count = state.buffer.count_lines();
            let current_line_len = state.buffer.line_length(state.file_row, state.tab_size);

            if state.file_col < current_line_len {
                // Normal case - delete character at cursor in the same line
                if let Err(e) = state.buffer.delete_char(state.file_row, state.file_col) {
                    state.print_error(match e {
                        FileBufferError::InvalidOperation => "Can't delete at this position",
                        _ => "Error deleting character",
                    })?;
                    return Ok(0);
                }
                // Cursor position stays the same
            } else if state.file_row + 1 < line_count {
                // At end of line - join with next line by deleting the newline
                if let Some(line_end) = state.buffer.find_line_end(state.file_row) {
                    if let Err(e) = state.buffer.delete_at_position(line_end) {
                        state.print_error(match e {
                            FileBufferError::InvalidOperation => "Can't join lines",
                            _ => "Error deleting newline",
                        })?;
                        return Ok(0);
                    }
                    // Cursor stays at same position (now in middle of joined line)
                }
            }
        }
        Key::Char(ch) => {
            // Insert the character at the current cursor position
            if let Err(e) = state.buffer.insert_char(state.file_row, state.file_col, ch) {
                state.print_error(match e {
                    FileBufferError::BufferFull => "Buffer is full",
                    _ => "Failed to insert character",
                })?;
                return Ok(0);
            }

            // Move cursor right
            state.file_col += 1;
        }
        _ => return Ok(0),
    }

    state.scroll_to_cursor();
    state.draw_screen()
}

#[cfg(not(tarpaulin_include))]
fn handle_open_file(state: &mut EditorState) -> Result<(), EditorError> {
    use crate::terminal::{clear_screen, restore_cursor};

    save_cursor()?;
    let prompt: &str = "Enter filename: ";
    state.print_message(prompt)?;
    move_cursor(state.winsize.rows as usize - 1, prompt.len())?;

    let mut filename = [0u8; MAX_PATH];
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
            state.buffer = new_buffer;

            clear_screen()?;
            state.draw_screen()?;
            state.print_message("File opened successfully")?;
            move_cursor(0, 0)?;
            state.filename[..filename.len()].copy_from_slice(&filename);
            Ok(())
        }
        Err(e) => {
            state.print_error("Error: Failed to open file")?;
            Err(e)
        }
    }
}

// Handle saving the file
fn handle_save_file(state: &mut EditorState) -> SysResult {
    match state.buffer.save_to_file(&state.filename) {
        Ok(_) => Ok(state.print_message("File saved successfully")?),
        Err(e) => {
            state.print_error("Error saving file")?;
            Err(e)
        }
    }
}

// Implement Drop for FileBuffer
impl Drop for FileBuffer {
    fn drop(&mut self) {
        self.cleanup();
    }
}

fn check_terminal_resize(state: &mut EditorState) -> SysResult {
    let mut new_winsize = Winsize::new();
    get_winsize(STDOUT, &mut new_winsize)?;

    // Check if terminal size has changed
    if new_winsize.rows != state.winsize.rows || new_winsize.cols != state.winsize.cols {
        // Update the editor state with new dimensions
        state.update_winsize(new_winsize);

        // Redraw everything
        clear_screen()?;
        state.draw_screen()?;
        state.draw_status_bar()?;
        state.print_message("Terminal resized")?;
    }

    Ok(0)
}

// Process key input in search mode
fn process_search_key(state: &mut EditorState, key: Key) -> SysResult {
    match key {
        Key::Escape | Key::ExitSearch => {
            // Cancel search and return to original position
            state.cancel_search()?;
        }
        Key::Enter => {
            // Accept search and stay at current match position
            state.accept_search()?;
        }
        Key::Backspace => {
            // Remove last character from search
            state.remove_search_char()?;
        }
        Key::Search => {
            // Switch to forward search if we're in reverse search,
            // otherwise find next match
            if state.search.reverse {
                state.switch_search_direction()?;
            }
            state.find_next_match()?;
        }
        Key::ReverseSearch => {
            // Switch to reverse search if we're in forward search,
            // otherwise find next match
            if !state.search.reverse {
                state.switch_search_direction()?;
            }
            state.find_next_match()?;
        }
        Key::Char(ch) => {
            // Add character to search query and find matches
            if ch.is_ascii_graphic() || ch == b' ' {
                state.add_search_char(ch)?;
            }
        }
        _ => {} // Ignore other keys in search mode
    }

    Ok(0)
}

// Process key input in normal editor mode
fn process_normal_key(state: &mut EditorState, key: Key, running: &mut bool) -> SysResult {
    state.print_message("")?;

    match key {
        Key::Quit => *running = false,
        Key::Refresh => {
            // Force a terminal resize check (also handles redrawing)
            check_terminal_resize(state)?;
            if check_terminal_resize(state).is_err() {
                clear_screen()?;
                state.draw_screen()?;
            }
        }
        Key::OpenFile => {
            let _ = handle_open_file(state);
        }
        Key::SaveFile => {
            handle_save_file(state)?;
        }
        Key::Search => {
            // Enter forward search mode
            state.start_search(false)?;
        }
        Key::ReverseSearch => {
            // Enter reverse search mode
            state.start_search(true)?;
        }
        Key::ArrowUp
        | Key::ArrowDown
        | Key::ArrowLeft
        | Key::ArrowRight
        | Key::Home
        | Key::End
        | Key::PageUp
        | Key::PageDown
        | Key::FirstChar
        | Key::LastChar
        | Key::Enter
        | Key::Backspace
        | Key::Delete => {
            process_cursor_key(key, state)?;
        }
        Key::Char(_) | Key::Combination(_) => {
            process_cursor_key(key, state)?;
        }
        Key::Escape | Key::ExitSearch => {}
    }

    Ok(0)
}

fn get_cmdline_filename() -> Result<[u8; MAX_PATH], EditorError> {
    let mut filename = [0u8; MAX_PATH];
    let mut empty_file = false;

    let cmdline_path = b"/proc/self/cmdline\0";
    if let Ok(fd) = open(cmdline_path, O_RDONLY) {
        let mut cmdline_buf = [0u8; MAX_PATH];
        if let Ok(bytes_read) = read(fd, &mut cmdline_buf, MAX_PATH) {
            close(fd)?;

            let mut i = 0;
            while i < bytes_read && cmdline_buf[i] != 0 {
                i += 1;
            }

            i += 1;
            if i < bytes_read {
                if cmdline_buf[i] == 0 {
                    // Empty argument provided, use empty buffer
                    empty_file = true;
                } else {
                    // Non-empty argument, use it as filename
                    filename = [0u8; MAX_PATH];
                    let mut j = 0;
                    while i < bytes_read && cmdline_buf[i] != 0 && j < MAX_PATH - 1 {
                        filename[j] = cmdline_buf[i];
                        i += 1;
                        j += 1;
                    }
                    filename[j] = 0;
                }
            }
        }
    }

    if empty_file {
        filename = [0u8; MAX_PATH];
    }

    Ok(filename)
}

pub fn run_editor() -> Result<(), EditorError> {
    enter_alternate_screen()?;
    clear_screen()?;

    let mut winsize = Winsize::new();
    get_winsize(STDOUT, &mut winsize)?;

    let filename = get_cmdline_filename()?;
    let mut state = EditorState::new(winsize, &filename);

    // Check if filename is empty (all zeros)
    let is_empty_filename = filename.iter().all(|&b| b == 0);

    state.buffer = if is_empty_filename {
        // Create an empty buffer
        let new_capacity = 4096;
        let prot = PROT_READ | PROT_WRITE;
        let flags = MAP_PRIVATE | MAP_ANONYMOUS;
        let Ok(new_buffer) = mmap(0, new_capacity, prot, flags, usize::MAX, 0) else {
            state.print_error("Error: Failed to create empty buffer")?;
            return Err(EditorError::MMapFile);
        };

        FileBuffer {
            content: new_buffer as *mut u8,
            size: 0,
            capacity: new_capacity,
            modified: true,
        }
    } else {
        // Open existing file
        match open_file(&filename) {
            Ok(buffer) => buffer,
            Err(e) => {
                state.print_error("Error: Failed to open file")?;
                return Err(e);
            }
        }
    };

    // Initial screen render
    state.draw_screen()?;
    state.draw_status_bar()?;
    if is_empty_filename {
        state.print_message("Empty buffer created")?;
    } else {
        state.print_message("File opened successfully")?;
    }

    // Main editor loop
    let mut running = true;

    while running {
        if let Some(key) = read_key() {
            // Process key based on current mode
            if state.search.mode {
                process_search_key(&mut state, key)?;
            } else {
                process_normal_key(&mut state, key, &mut running)?;
            }
        }

        // Update status bar and cursor position
        state.draw_status_bar()?;
        move_cursor(state.cursor_row, state.cursor_col)?;
    }

    exit_alternate_screen()?;
    Ok(())
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub const _: usize = 0;

    use super::*;
    use crate::editor::file_buffer::tests::create_test_file_buffer;
    use crate::syscall::{O_CREAT, O_RDONLY, O_TRUNC, O_WRONLY, close, write};
    use crate::terminal::tests::{disable_test_mode, enable_test_mode};

    // Helper function for testing
    fn is_error(result: usize) -> bool {
        const MAX_ERRNO: usize = 4095;
        result > usize::MAX - MAX_ERRNO
    }

    fn create_test_file() {
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
    }

    #[test]
    fn test_open_file() {
        create_test_file();
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

        // Verify that terminal functions used by handle_open_file work
        let save_result = save_cursor();
        assert!(save_result.is_ok(), "save_cursor should work in test mode");

        // Since print_message is now a method on EditorState, we can't directly test it here
        // Let's verify that we can move the cursor as handle_open_file would
        let move_result = move_cursor(winsize.rows as usize - 1, 14); // "Enter filename: ".len()
        assert!(move_result.is_ok(), "move_cursor should work in test mode");

        create_test_file();
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

    // Tests for EditorState struct and its methods
    #[test]
    fn test_editor_state_new() {
        // Create a test winsize
        let mut winsize = Winsize::new();
        winsize.rows = 24;
        winsize.cols = 80;

        // Create a new editor state
        let state = EditorState::new(winsize, &[0; MAX_PATH]);

        // Verify initial state
        assert_eq!(state.winsize.rows, 24, "Winsize rows should match");
        assert_eq!(state.winsize.cols, 80, "Winsize cols should match");
        assert_eq!(state.cursor_row, 0, "Initial cursor_row should be 0");
        assert_eq!(state.cursor_col, 0, "Initial cursor_col should be 0");
        assert_eq!(state.file_row, 0, "Initial file_row should be 0");
        assert_eq!(state.file_col, 0, "Initial file_col should be 0");
        assert_eq!(state.scroll_row, 0, "Initial scroll_row should be 0");
        assert_eq!(state.scroll_col, 0, "Initial scroll_col should be 0");
        assert_eq!(state.tab_size, 4, "Initial tab_size should be 4");
    }

    #[test]
    fn test_editor_state_editing_rows() {
        // Test with normal size terminal (rows >= 2)
        let mut winsize = Winsize::new();
        winsize.rows = 24; // Normal sized terminal
        winsize.cols = 80;
        let state = EditorState::new(winsize, &[0; MAX_PATH]);

        // Should have rows - 2 available for editing (2 rows for status and message)
        assert_eq!(
            state.editing_rows(),
            22,
            "Should have 22 rows available for editing"
        );

        // Test with small terminal (rows < 2)
        let mut small_winsize = Winsize::new();
        small_winsize.rows = 1; // Too small for status bars
        small_winsize.cols = 80;
        let small_state = EditorState::new(small_winsize, &[0; MAX_PATH]);

        // Should use all available rows since it's too small for status bars
        assert_eq!(
            small_state.editing_rows(),
            0,
            "Should handle small terminal correctly"
        );

        // Test with zero rows terminal (edge case)
        let mut zero_winsize = Winsize::new();
        zero_winsize.rows = 0;
        zero_winsize.cols = 80;
        let zero_state = EditorState::new(zero_winsize, &[0; MAX_PATH]);

        // Should handle zero rows gracefully
        assert_eq!(
            zero_state.editing_rows(),
            0,
            "Should handle zero-row terminal"
        );
    }

    #[test]
    fn test_editor_state_scroll_to_cursor() {
        // Create a test state
        let mut winsize = Winsize::new();
        winsize.rows = 10; // Small window for easier testing
        winsize.cols = 20;
        let mut state = EditorState::new(winsize, &[0; MAX_PATH]);

        // Test case 1: Cursor is within visible area - nothing should change
        state.file_row = 5;
        state.file_col = 5;
        state.scroll_row = 0;
        state.scroll_col = 0;

        state.scroll_to_cursor();

        assert_eq!(
            state.scroll_row, 0,
            "Scroll row shouldn't change when cursor is visible"
        );
        assert_eq!(
            state.scroll_col, 0,
            "Scroll col shouldn't change when cursor is visible"
        );
        assert_eq!(
            state.cursor_row, 5,
            "Cursor row should be file_row - scroll_row"
        );
        assert_eq!(
            state.cursor_col, 5,
            "Cursor col should be file_col - scroll_col"
        );

        // Test case 2: Cursor is below visible area - scroll down
        state.file_row = 15; // Beyond the visible area
        state.file_col = 5;
        state.scroll_row = 0;
        state.scroll_col = 0;

        state.scroll_to_cursor();

        // Should scroll to make cursor visible
        assert!(
            state.scroll_row > 0,
            "Should scroll down when cursor is below visible area"
        );
        assert_eq!(state.scroll_col, 0, "Scroll col shouldn't change");
        assert_eq!(
            state.cursor_row,
            state.file_row - state.scroll_row,
            "Cursor row should be file_row - scroll_row"
        );

        // Test case 3: Cursor is above visible area - scroll up
        state.file_row = 3;
        state.file_col = 5;
        state.scroll_row = 5; // Scrolled down too far
        state.scroll_col = 0;

        state.scroll_to_cursor();

        assert_eq!(
            state.scroll_row, 3,
            "Should scroll up when cursor is above visible area"
        );
        assert_eq!(state.scroll_col, 0, "Scroll col shouldn't change");
        assert_eq!(
            state.cursor_row, 0,
            "Cursor row should be file_row - scroll_row"
        );

        // Test case 4: Cursor is right of visible area - scroll right
        state.file_row = 5;
        state.file_col = 25; // Beyond visible columns (0-19)
        state.scroll_row = 0;
        state.scroll_col = 0;

        state.scroll_to_cursor();

        assert_eq!(state.scroll_row, 0, "Scroll row shouldn't change");
        assert!(
            state.scroll_col > 0,
            "Should scroll right when cursor is beyond right edge"
        );
        assert_eq!(
            state.cursor_col,
            state.file_col - state.scroll_col,
            "Cursor col should be file_col - scroll_col"
        );

        // Test case 5: Cursor is left of visible area - scroll left
        state.file_row = 5;
        state.file_col = 5;
        state.scroll_row = 0;
        state.scroll_col = 10; // Scrolled right too far

        state.scroll_to_cursor();

        assert_eq!(state.scroll_row, 0, "Scroll row shouldn't change");
        assert_eq!(
            state.scroll_col, 5,
            "Should scroll left when cursor is beyond left edge"
        );
        assert_eq!(
            state.cursor_col, 0,
            "Cursor col should be file_col - scroll_col"
        );
    }

    #[test]
    fn test_editor_state_cursor_movement() {
        // Create a test buffer with some content for cursor movement tests
        let test_content = b"First line\nSecond line\nThird line with\ttab\nFourth line\n";

        // Create an editor state with a small window
        let mut winsize = Winsize::new();
        winsize.rows = 10;
        winsize.cols = 20;
        let mut state = EditorState::new(winsize, &[0; MAX_PATH]);
        state.buffer = create_test_file_buffer(test_content);

        // Test cursor_up when already at top row - should do nothing
        state.file_row = 0;
        state.file_col = 5;
        state.cursor_up();
        assert_eq!(state.file_row, 0, "Can't move up from the top row");
        assert_eq!(state.file_col, 5, "Column shouldn't change");

        // Test cursor_up from a lower position
        state.file_row = 2;
        state.file_col = 5;
        state.cursor_up();
        assert_eq!(state.file_row, 1, "Should move up one row");
        assert_eq!(
            state.file_col, 5,
            "Column shouldn't change when it fits on the line"
        );

        // Test cursor_down
        state.file_row = 1;
        state.file_col = 5;
        state.cursor_down();
        assert_eq!(state.file_row, 2, "Should move down one row");
        assert_eq!(
            state.file_col, 5,
            "Column shouldn't change when it fits on the line"
        );

        // Test cursor_down at bottom row - should do nothing
        state.file_row = state.buffer.count_lines() - 1; // Last line
        state.file_col = 5;
        state.cursor_down();
        assert_eq!(
            state.file_row,
            state.buffer.count_lines() - 1,
            "Can't move down from the bottom row"
        );
        assert_eq!(state.file_col, 5, "Column shouldn't change");

        // Test cursor_left
        state.file_row = 1;
        state.file_col = 5;
        state.cursor_left();
        assert_eq!(state.file_row, 1, "Row shouldn't change");
        assert_eq!(state.file_col, 4, "Column should decrease by 1");

        // Test cursor_left at beginning of line - move to end of previous line
        state.file_row = 1;
        state.file_col = 0;
        state.cursor_left();
        assert_eq!(state.file_row, 0, "Should move to previous row");
        assert!(
            state.file_col > 0,
            "Column should move to end of previous line"
        );

        // Test cursor_right
        state.file_row = 1;
        state.file_col = 5;
        state.cursor_right();
        assert_eq!(state.file_row, 1, "Row shouldn't change");
        assert_eq!(state.file_col, 6, "Column should increase by 1");

        // Test cursor_right at end of line - move to beginning of next line
        // First get the line length
        let line_len = state.buffer.line_length(1, state.tab_size);
        state.file_row = 1;
        state.file_col = line_len; // End of line
        state.cursor_right();
        assert_eq!(state.file_row, 2, "Should move to next row");
        assert_eq!(
            state.file_col, 0,
            "Column should be at beginning of next line"
        );

        // Test cursor_home - move to beginning of line
        state.file_row = 1;
        state.file_col = 5;
        state.cursor_home();
        assert_eq!(state.file_row, 1, "Row shouldn't change");
        assert_eq!(state.file_col, 0, "Column should be 0 (beginning of line)");

        // Test cursor_end - move to end of line
        state.file_row = 1;
        state.file_col = 0;
        state.cursor_end();
        assert_eq!(state.file_row, 1, "Row shouldn't change");
        assert_eq!(state.file_col, line_len, "Column should be at end of line");
    }

    #[test]
    fn test_editor_state_page_navigation() {
        // Create a test buffer with multiple lines
        let mut test_content = Vec::new();
        for i in 0..20 {
            let line = format!("Line {i}\n");
            test_content.extend_from_slice(line.as_bytes());
        }

        // Create an editor state with a small window
        let mut winsize = Winsize::new();
        winsize.rows = 10; // 8 editing rows after subtracting status bars
        winsize.cols = 20;
        let mut state = EditorState::new(winsize, &[0; MAX_PATH]);
        state.buffer = create_test_file_buffer(&test_content);

        // Test page_up from top (should stay at top)
        state.file_row = 0;
        state.file_col = 2;
        state.scroll_row = 0;
        state.page_up();
        assert_eq!(state.file_row, 0, "Should remain at top row");
        assert_eq!(state.scroll_row, 0, "Scroll row should remain at top");

        // Move to middle of file and test page_up
        let editing_rows = state.editing_rows();
        state.file_row = 15;
        state.file_col = 2;
        state.scroll_row = 10;

        // First store the current values to calculate expected results
        let prev_file_row = state.file_row;
        let prev_scroll_row = state.scroll_row;

        // Page up should move cursor and scroll up by editing_rows
        state.page_up();

        // Check that we moved up by the correct number of rows
        assert!(state.file_row < prev_file_row, "Should move cursor up");
        assert!(state.scroll_row < prev_scroll_row, "Should scroll up");

        // If we were already at top, don't go negative
        if prev_file_row >= editing_rows {
            assert_eq!(
                state.file_row,
                prev_file_row - editing_rows,
                "Should move up by editing_rows"
            );
        } else {
            assert_eq!(state.file_row, 0, "Should move to top row if near top");
        }

        // Similar check for scroll
        if prev_scroll_row >= editing_rows {
            assert_eq!(
                state.scroll_row,
                prev_scroll_row - editing_rows,
                "Should scroll up by editing_rows"
            );
        } else {
            assert_eq!(state.scroll_row, 0, "Should scroll to top row if near top");
        }

        // Test page_down from current position
        let prev_file_row = state.file_row;
        let prev_scroll_row = state.scroll_row;

        state.page_down();

        // Check that we moved down by the correct number of rows
        assert!(state.file_row > prev_file_row, "Should move cursor down");
        assert!(
            state.scroll_row >= prev_scroll_row,
            "Should scroll down or stay"
        );

        // Test page_down from bottom of file
        state.file_row = state.buffer.count_lines() - 2;
        state.file_col = 2;
        state.page_down();
        assert_eq!(
            state.file_row,
            state.buffer.count_lines() - 1,
            "Should move to last line but not beyond"
        );
    }

    #[test]
    fn test_editor_state_cursor_column_adjustments() {
        // Create a buffer with varying line lengths to test column adjustments
        let varying_content =
            b"Short\nLoooooooonger line\nVery very long line for testing\nShort again";

        // Create an editor state
        let mut winsize = Winsize::new();
        winsize.rows = 10;
        winsize.cols = 20;
        let mut state = EditorState::new(winsize, &[0; MAX_PATH]);
        state.buffer = create_test_file_buffer(varying_content);

        // Position cursor at end of long line
        state.file_row = 2; // "Very very long line for testing"
        state.file_col = 30;

        // Now move up to shorter line
        state.cursor_up();

        // Verify cursor column is adjusted to fit the shorter line
        assert_eq!(state.file_row, 1, "Should move up to shorter line");
        assert!(
            state.file_col < 30,
            "Column should be adjusted to fit shorter line"
        );
        let line1_len = state.buffer.line_length(1, state.tab_size);
        assert_eq!(
            state.file_col, line1_len,
            "Column should be at end of shorter line"
        );

        // Test similar adjustment moving down to shorter line
        state.file_row = 1; // "Loooooooonger line"
        state.file_col = line1_len; // At end of this line

        // Move down to next line (which is longer)
        state.cursor_down();

        // Verify cursor position - should maintain column
        assert_eq!(state.file_row, 2, "Should move down to next line");
        assert_eq!(
            state.file_col, line1_len,
            "Column should be preserved when moving to longer line"
        );

        // Move down to shortest line
        state.file_row = 2;
        state.file_col = 20; // Somewhere in the middle of the long line
        state.cursor_down();

        // Verify cursor is adjusted
        assert_eq!(state.file_row, 3, "Should move down to shorter line");
        let line3_len = state.buffer.line_length(3, state.tab_size);
        assert_eq!(
            state.file_col, line3_len,
            "Column should be adjusted to end of shortest line"
        );
    }

    #[test]
    fn test_editor_state_integrated_operations() {
        // Create a buffer with multiple lines
        let content = b"First line\nSecond line\nThird line\nFourth line\nFifth line";

        // Create an editor state
        let mut winsize = Winsize::new();
        winsize.rows = 5; // Small window to test scrolling
        winsize.cols = 15;
        let mut state = EditorState::new(winsize, &[0; MAX_PATH]);
        state.buffer = create_test_file_buffer(content);

        // Test a sequence of operations that would typically be performed

        // 1. Start at the beginning
        assert_eq!(state.file_row, 0);
        assert_eq!(state.file_col, 0);

        // 2. Move down several times
        for _ in 0..3 {
            state.cursor_down();
        }
        assert_eq!(
            state.file_row, 3,
            "Should be at fourth line after moving down 3 times"
        );

        // 3. Move to end of line and then right (should go to next line)
        state.cursor_end();
        let line_len = state.buffer.line_length(3, state.tab_size);
        assert_eq!(state.file_col, line_len, "Should be at end of line");

        state.cursor_right();
        assert_eq!(
            state.file_row, 4,
            "Should move to next line after right from end"
        );
        assert_eq!(state.file_col, 0, "Should be at beginning of new line");

        // 4. Ensure scrolling happens appropriately
        state.scroll_to_cursor();
        // In a small 5-row window with 2 rows for status, only 3 content rows are visible
        // We're now at line 4, so scrolling should have happened
        assert!(state.scroll_row > 0, "Should have scrolled down for line 4");

        // 5. Page up should move back toward the top
        state.page_up();
        assert!(state.file_row < 4, "Page up should move cursor up");
        assert!(
            state.scroll_row < state.file_row,
            "Scroll row should be less than file row after page up"
        );

        // 6. Home followed by repeated right movements
        state.cursor_home();
        assert_eq!(state.file_col, 0, "Home should move to beginning of line");

        for _ in 0..5 {
            state.cursor_right();
        }
        assert_eq!(state.file_col, 5, "Should move 5 positions right");

        // 7. Left movements including across line boundaries
        state.file_row = 1;
        state.file_col = 0;
        state.cursor_left();
        assert_eq!(state.file_row, 0, "Should move up to previous line");
        let prev_line_len = state.buffer.line_length(0, state.tab_size);
        assert_eq!(
            state.file_col, prev_line_len,
            "Should move to end of previous line"
        );
    }
}
