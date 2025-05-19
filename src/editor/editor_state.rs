use crate::{
    syscall::{MAX_PATH, STDOUT, SysResult, putchar, puts, write_buf, write_unchecked},
    terminal::{
        clear_line, move_cursor, reset_colors, restore_cursor, save_cursor, set_bg_color, set_bold,
        set_fg_color, write_number,
    },
    termios::Winsize,
};

use super::{FileBuffer, SearchState};

pub(in crate::editor) struct EditorState {
    pub(in crate::editor) winsize: Winsize, // Terminal window size
    pub(in crate::editor) cursor_row: usize, // Cursor row in the visible window
    pub(in crate::editor) cursor_col: usize, // Cursor column in the visible window
    pub(in crate::editor) file_row: usize,  // Row in the file (0-based)
    pub(in crate::editor) file_col: usize,  // Column in the file (0-based)
    pub(in crate::editor) preferred_col: usize, // Remembered column position for vertical movement
    pub(in crate::editor) scroll_row: usize, // Top row of the file being displayed
    pub(in crate::editor) scroll_col: usize, // Leftmost column being displayed
    pub(in crate::editor) tab_size: usize,  // Number of spaces per tab
    pub(in crate::editor) filename: [u8; MAX_PATH], // Current file name
    pub(in crate::editor) buffer: FileBuffer,
    pub(in crate::editor) search: SearchState, // Search state
}

impl EditorState {
    // Create a new editor state
    pub(in crate::editor) fn new(winsize: Winsize, filename: &[u8; MAX_PATH]) -> Self {
        let mut own_filename = [0u8; MAX_PATH];
        own_filename[..filename.len()].copy_from_slice(filename);

        Self {
            winsize,
            cursor_row: 0,
            cursor_col: 0,
            file_row: 0,
            file_col: 0,
            preferred_col: 0,
            scroll_row: 0,
            scroll_col: 0,
            tab_size: 4,
            filename: own_filename,
            buffer: FileBuffer {
                content: core::ptr::null_mut(),
                size: 0,
                capacity: 0,
                modified: false,
            },
            search: SearchState::new(),
        }
    }

    // Update the editor state when terminal size changes
    pub(in crate::editor) fn update_winsize(&mut self, new_winsize: Winsize) {
        // Store the new window size
        self.winsize = new_winsize;

        // Make sure cursor stays within visible area
        self.scroll_to_cursor();

        // Update cursor position relative to scroll position
        self.cursor_row = self.file_row.saturating_sub(self.scroll_row);
        self.cursor_col = self.file_col.saturating_sub(self.scroll_col);
    }

    // Start search mode
    pub(in crate::editor) fn start_search(&mut self, reverse: bool) -> SysResult {
        // Save current position to return to if search is cancelled
        self.search.orig_row = self.file_row;
        self.search.orig_col = self.file_col;

        // Reset search state
        self.search.mode = true;
        self.search.reverse = reverse;
        self.search.query_len = 0;
        self.search.match_len = 0; // Reset match length

        // Clear the query array
        let mut i = 0;
        while i < self.search.query.len() {
            self.search.query[i] = 0;
            i += 1;
        }

        // Show search prompt with appropriate direction indicator

        if reverse {
            self.print_message("Reverse search: ")
        } else {
            self.print_message("Search: ")
        }
    }

    // Cancel search mode and return to original position
    pub(in crate::editor) fn cancel_search(&mut self) -> SysResult {
        self.search.mode = false;
        self.search.match_len = 0; // Clear highlighting

        // Restore original cursor position
        self.file_row = self.search.orig_row;
        self.file_col = self.search.orig_col;
        self.scroll_to_cursor();

        // Redraw and clear search message
        self.draw_screen()?;
        self.print_message("")
    }

    // Accept current search match and exit search mode
    pub(in crate::editor) fn accept_search(&mut self) -> SysResult {
        self.search.mode = false;
        self.search.match_len = 0; // Clear highlighting
        self.draw_screen()?;
        self.print_message("")
    }

    // Update search when query changes
    pub(in crate::editor) fn update_search(&mut self) -> SysResult {
        // Initialize result to Ok(0), will be updated if needed
        let mut result = Ok(0);

        // Only continue if the search query is not empty
        if self.search.query_len > 0 {
            // Flag to track if a match is found
            let mut match_found = false;

            // First check if current position still matches the updated query
            if self.search.match_len > 0 {
                // Assume current match is not valid initially
                let mut current_match_valid = false;

                // Get the current line to check if it still matches the query
                let line_option = self.buffer.get_line(self.file_row);

                if line_option.is_some() {
                    let line = line_option.unwrap();

                    // Check if the line is long enough for the query
                    if self.file_col + self.search.query_len <= line.len() {
                        let query = &self.search.query[..self.search.query_len];

                        // Start by assuming the match is valid
                        current_match_valid = true;

                        // Compare each character with the updated query
                        let mut j = 0;
                        while j < self.search.query_len {
                            if self.file_col + j >= line.len()
                                || line[self.file_col + j] != query[j]
                            {
                                current_match_valid = false;
                                break;
                            }
                            j += 1;
                        }
                    }
                }

                // If current position still matches the updated query, just update match length
                if current_match_valid {
                    self.search.match_row = self.file_row;
                    self.search.match_col = self.file_col;
                    self.search.match_len = self.search.query_len;
                    self.draw_screen()?;
                    match_found = true;
                }
            }

            // If current position doesn't match, find a new match
            if !match_found {
                // Determine search direction and execute search
                let search_result = if self.search.reverse {
                    self.search
                        .find_substring_backward(&self.buffer, self.file_row, self.file_col)
                } else {
                    self.search
                        .find_substring_forward(&self.buffer, self.file_row, self.file_col)
                };

                // Process the search result if a match was found
                if search_result.is_some() {
                    let (row, col, len) = search_result.unwrap();

                    // Store match info
                    self.search.match_row = row;
                    self.search.match_col = col;
                    self.search.match_len = len;

                    // Move cursor to match position
                    self.file_row = row;
                    self.file_col = col;
                    self.scroll_to_cursor();

                    // Redraw the screen to show the match
                    self.draw_screen()?;
                    match_found = true;
                }

                // If no match was found, show an error message
                if !match_found {
                    result = self.print_error("No match found");
                }
            }
        }

        result
    }

    // Add a character to the search query
    pub(in crate::editor) fn add_search_char(&mut self, ch: u8) -> SysResult {
        // Initialize result
        let mut result;

        // Check if the query still has space for one more character
        if self.search.query_len < self.search.query.len() - 1 {
            // Add the character to the query
            self.search.query[self.search.query_len] = ch;
            self.search.query_len += 1;

            // Update search prompt
            result = self.print_status(|| {
                let mut inner_result;

                // Display appropriate search prompt based on direction
                if self.search.reverse {
                    inner_result = puts("Reverse search: ");
                } else {
                    inner_result = puts("Search: ");
                }

                // Check if prompt was displayed successfully
                if inner_result.is_ok() {
                    // Display the query text
                    inner_result =
                        write_unchecked(STDOUT, self.search.query.as_ptr(), self.search.query_len);
                }

                // Return status
                if inner_result.is_ok() {
                    inner_result = Ok(0);
                }
                inner_result
            });

            // If status update was successful, search for matches
            if result.is_ok() {
                result = self.update_search();
            }
        } else {
            // Query is too long, show error
            result = self.print_error("Search query too long");
        }

        result
    }

    // Switch search direction and update display
    pub(in crate::editor) fn switch_search_direction(&mut self) -> SysResult {
        // Only proceed if in search mode and with a valid query
        if !self.search.mode || self.search.query_len == 0 {
            return Ok(0);
        }

        // Switch the direction
        self.search.switch_direction();

        // Update the search prompt based on the new direction
        let result = self.print_status(|| {
            let mut inner_result;

            // Display appropriate search prompt based on new direction
            if self.search.reverse {
                inner_result = puts("Reverse search: ");
            } else {
                inner_result = puts("Search: ");
            }

            // Display the query text
            if inner_result.is_ok() {
                inner_result =
                    write_unchecked(STDOUT, self.search.query.as_ptr(), self.search.query_len);
            }

            if inner_result.is_ok() {
                inner_result = Ok(0);
            }
            inner_result
        });

        if result.is_ok() {
            // No need to change the current match yet - keep it the same until the user presses
            // the key to find the next match in the new direction
            self.draw_screen()
        } else {
            result
        }
    }

    // Remove the last character from the search query
    pub(in crate::editor) fn remove_search_char(&mut self) -> SysResult {
        // Initialize result to Ok(0), will be updated if needed
        let mut result = Ok(0);

        // Only proceed if there is at least one character in the query
        if self.search.query_len > 0 {
            // Remove the last character
            self.search.query_len -= 1;

            // Update search prompt
            result = self.print_status(|| {
                let mut inner_result;

                // Display appropriate search prompt based on direction
                if self.search.reverse {
                    inner_result = puts("Reverse search: ");
                } else {
                    inner_result = puts("Search: ");
                }

                // Check if prompt was displayed successfully
                if inner_result.is_ok() {
                    // Display the query text
                    inner_result =
                        write_unchecked(STDOUT, self.search.query.as_ptr(), self.search.query_len);
                }

                // Return status
                if inner_result.is_ok() {
                    inner_result = Ok(0);
                }
                inner_result
            });

            // If status update was successful, process the updated query
            if result.is_ok() {
                // Check if there are still characters in the query
                if self.search.query_len > 0 {
                    // Adjust match length if needed
                    if self.search.match_len > self.search.query_len {
                        self.search.match_len = self.search.query_len;
                    }

                    // Search for new matches with updated query
                    result = self.update_search();
                } else {
                    // No characters left in query, reset to original position
                    self.file_row = self.search.orig_row;
                    self.file_col = self.search.orig_col;
                    self.search.match_len = 0; // Clear highlights

                    // Update screen display
                    self.scroll_to_cursor();
                    result = self.draw_screen();
                }
            }
        }

        result
    }

    // Find the next match for the current search query
    pub(in crate::editor) fn find_next_match(&mut self) -> SysResult {
        // Initialize result to Ok(0), will be updated if needed
        let mut result = Ok(0);

        // Only proceed if there is a query to search for
        if self.search.query_len > 0 {
            // Variables to hold the position to start searching from
            let search_row;
            let search_col;

            // Determine search starting position based on direction
            if self.search.reverse {
                // For reverse search, calculate starting position
                if self.search.match_col > 0 {
                    // Start at the position before the current match
                    search_row = self.search.match_row;
                    search_col = self.search.match_col - 1;
                } else if self.search.match_row > 0 {
                    // We're at the start of a line, so move to the end of previous line
                    search_row = self.search.match_row - 1;
                    let mut prev_line_len = 0;

                    // Get the length of the previous line
                    let prev_line = self.buffer.get_line(search_row);
                    if prev_line.is_some() {
                        prev_line_len = prev_line.unwrap().len();
                    }

                    search_col = prev_line_len;
                } else {
                    // We're at the beginning of the file, wrap around to the end
                    search_row = self.buffer.count_lines() - 1;
                    let mut last_line_len = 0;

                    // Get the length of the last line
                    let last_line = self.buffer.get_line(search_row);
                    if last_line.is_some() {
                        last_line_len = last_line.unwrap().len();
                    }

                    search_col = last_line_len;
                }
            } else {
                // For forward search, start after the end of the current match
                search_row = self.search.match_row;
                search_col = self.search.match_col + self.search.match_len;
            }

            // Execute the search in the appropriate direction
            let search_result = if self.search.reverse {
                self.search
                    .find_substring_backward(&self.buffer, search_row, search_col)
            } else {
                self.search
                    .find_substring_forward(&self.buffer, search_row, search_col)
            };

            // Process the search result
            if search_result.is_some() {
                let (row, col, len) = search_result.unwrap();

                // Store the match information
                self.search.match_row = row;
                self.search.match_col = col;
                self.search.match_len = len;

                // Move cursor to the match position
                self.file_row = row;
                self.file_col = col;
                self.scroll_to_cursor();

                // Update the display
                result = self.draw_screen();
            } else {
                // No more matches found
                result = self.print_error("No more matches");
            }
        }

        result
    }

    // Get the number of rows available for editing (excluding status bars)
    pub(in crate::editor) fn editing_rows(&self) -> usize {
        (self.winsize.rows as usize).saturating_sub(2)
    }

    pub(in crate::editor) fn scroll_to_cursor(&mut self) {
        // Handle vertical scrolling
        self.scroll_row = match self.file_row {
            // If cursor is above visible area, scroll up
            row if row < self.scroll_row => row,

            // If cursor is below visible area, scroll down
            row if row >= self.scroll_row + self.editing_rows() => {
                row.saturating_sub(self.editing_rows()).saturating_add(1)
            }

            // Otherwise keep current scroll position
            _ => self.scroll_row,
        };

        // Handle horizontal scrolling
        let visible_cols = self.winsize.cols as usize;
        self.scroll_col = match self.file_col {
            // If cursor is left of visible area, scroll left
            col if col < self.scroll_col => col,

            // If cursor is right of visible area, scroll right
            col if col >= self.scroll_col + visible_cols => {
                col.saturating_sub(visible_cols).saturating_add(1)
            }

            // Otherwise keep current scroll position
            _ => self.scroll_col,
        };

        // Update cursor position relative to scroll position
        self.cursor_row = self.file_row.saturating_sub(self.scroll_row);
        self.cursor_col = self.file_col.saturating_sub(self.scroll_col);
    }

    pub(in crate::editor) fn cursor_up(&mut self) {
        if self.file_row > 0 {
            self.file_row -= 1;

            let new_line_len = self.buffer.line_length(self.file_row, self.tab_size);
            if self.preferred_col > new_line_len {
                self.file_col = new_line_len;
            } else {
                self.file_col = self.preferred_col;
            }
        }
    }

    pub(in crate::editor) fn cursor_down(&mut self) {
        let line_count = self.buffer.count_lines();
        if self.file_row + 1 < line_count {
            // Move to the next line
            self.file_row += 1;

            // Set cursor column to either preferred position or end of line
            let new_line_len = self.buffer.line_length(self.file_row, self.tab_size);
            if self.preferred_col > new_line_len {
                self.file_col = new_line_len;
            } else {
                self.file_col = self.preferred_col;
            }
        }
    }

    // Move cursor left
    pub(in crate::editor) fn cursor_left(&mut self) {
        if self.file_col > 0 {
            self.file_col -= 1;
            // Update preferred column
            self.preferred_col = self.file_col;
        } else if self.file_row > 0 {
            // At beginning of line, move to end of previous line
            self.file_row -= 1;
            self.file_col = self.buffer.line_length(self.file_row, self.tab_size);
            self.preferred_col = self.file_col;
        }
    }

    pub(in crate::editor) fn cursor_right(&mut self) {
        let current_line_len = self.buffer.line_length(self.file_row, self.tab_size);
        let line_count = self.buffer.count_lines();

        if self.file_col < current_line_len {
            self.file_col += 1;
            self.preferred_col = self.file_col;
        } else if self.file_row + 1 < line_count {
            self.file_row += 1;
            self.file_col = 0;
            self.preferred_col = self.file_col;
        }
    }

    pub(in crate::editor) fn cursor_home(&mut self) {
        self.file_col = 0;
        self.preferred_col = self.file_col;
    }

    pub(in crate::editor) fn cursor_end(&mut self) {
        self.file_col = self.buffer.line_length(self.file_row, self.tab_size);
        self.preferred_col = self.file_col;
    }

    pub(in crate::editor) fn page_up(&mut self) {
        // Get the number of lines to scroll (screen height)
        let lines_to_scroll = self.editing_rows();

        self.scroll_row = self.scroll_row.saturating_sub(lines_to_scroll);
        self.file_row = self.file_row.saturating_sub(lines_to_scroll);

        // Make sure cursor doesn't go beyond the end of the current line
        // but preserve the preferred column during vertical movement
        let current_line_len = self.buffer.line_length(self.file_row, self.tab_size);
        if self.file_col > current_line_len {
            self.file_col = current_line_len;
        }

        self.cursor_row = self.file_row - self.scroll_row;
    }

    pub(in crate::editor) fn page_down(&mut self) {
        let lines_to_scroll = self.editing_rows();
        let line_count = self.buffer.count_lines();

        if self.file_row + lines_to_scroll < line_count {
            self.file_row += lines_to_scroll;
        } else {
            self.file_row = line_count - 1;
        }

        let max_scroll_row = self.file_row - self.editing_rows() + 1;
        if max_scroll_row > 0 {
            self.scroll_row = max_scroll_row;
        }

        let current_line_len = self.buffer.line_length(self.file_row, self.tab_size);
        if self.file_col > current_line_len {
            self.file_col = current_line_len;
        }

        self.cursor_row = self.file_row - self.scroll_row;
    }

    // Move cursor to the beginning of the file (first line, first column)
    pub(in crate::editor) fn cursor_first_char(&mut self) {
        self.file_row = 0;
        self.file_col = 0;
        // Update preferred column
        self.preferred_col = self.file_col;
        // Note: scroll_to_cursor is called by the key handler
    }

    // Move cursor to the end of the file (last line, last column)
    pub(in crate::editor) fn cursor_last_char(&mut self) {
        let line_count = self.buffer.count_lines();
        if line_count > 0 {
            self.file_row = line_count - 1;
            self.file_col = self.buffer.line_length(self.file_row, self.tab_size);
            // Update preferred column
            self.preferred_col = self.file_col;
        }
        // Note: scroll_to_cursor is called by the key handler
    }

    pub(in crate::editor) fn draw_status_bar(&self) -> SysResult {
        if self.winsize.rows < 3 {
            return Ok(0);
        }
        save_cursor()?;
        move_cursor(self.winsize.rows as usize - 2, 0)?;
        set_bg_color(7)?;
        set_fg_color(0)?;

        write_buf(&self.filename)?;

        // Modified mark
        if self.buffer.is_modified() {
            puts("*")?;
        }

        puts(" L")?;
        write_number(self.file_row);
        puts(":")?;
        write_number(self.file_col);

        #[cfg(debug_assertions)]
        {
            puts(" ")?;
            write_number(self.buffer.size);
            puts(" ")?;
            write_number(self.buffer.capacity);

            puts(" Search: ")?;
            write_buf(&self.search.query[..self.search.query_len])?;
        }
        // Clear to the end of line (makes sure status bar fills whole width)
        // ESC [ K - Clear from cursor to end of line
        clear_line()?;

        // Reset colors
        reset_colors()?;

        // Restore cursor position
        restore_cursor()
    }

    pub(in crate::editor) fn draw_screen(&self) -> SysResult {
        // Calculate available height for content
        let available_rows = self.editing_rows();
        // Convert to usize for iterator
        let line_count = self.buffer.count_lines();

        // Draw lines from the file buffer
        // Using a manually bounded loop to avoid clippy warnings
        for i in 0..available_rows {
            // Position cursor at start of each line
            // We know i < available_rows_usize which came from a u16, so we can safely convert back
            move_cursor(i, 0)?;

            clear_line()?;
            let file_line_idx = self.scroll_row + i;

            if file_line_idx >= line_count {
                // We're past the end of file, leave the rest of screen empty
                continue;
            }

            // Get the line from file buffer
            if let Some(line) = self.buffer.get_line(file_line_idx) {
                if line.is_empty() {
                    continue;
                }

                // Calculate how much to skip from the start (for horizontal scrolling)
                let mut chars_to_skip = self.scroll_col;
                let mut col = 0;
                let mut screen_col = 0;

                // Check if this line contains the search match
                let is_match_line = self.search.mode
                    && self.search.query_len > 0
                    && file_line_idx == self.search.match_row;
                let match_start = if is_match_line {
                    self.search.match_col
                } else {
                    0
                };
                let match_end = if is_match_line {
                    self.search.match_col + self.search.match_len
                } else {
                    0
                };

                // Display each character in the line
                for (idx, &byte) in line.iter().enumerate() {
                    // Track if current character is part of a search match for highlighting
                    let is_highlight = is_match_line && idx >= match_start && idx < match_end;

                    if byte == 0 {
                        // Stop at null byte
                        break;
                    }

                    // Apply highlighting if this character is part of a match
                    if is_highlight && chars_to_skip == 0 && screen_col < self.winsize.cols as usize
                    {
                        // Set inverted colors for highlighting (reverse video)
                        set_bg_color(7)?;
                        set_fg_color(0)?;
                    }

                    if byte == b'\t' {
                        // Handle tabs - convert to spaces
                        let spaces = self.tab_size - (col % self.tab_size);
                        col += spaces;

                        // Skip if we're still scrolled horizontally
                        if chars_to_skip > 0 {
                            if chars_to_skip >= spaces {
                                chars_to_skip -= spaces;
                            } else {
                                // Draw partial spaces after the horizontal scroll point
                                let visible_spaces = spaces - chars_to_skip;
                                for _ in 0..visible_spaces {
                                    if screen_col < self.winsize.cols as usize {
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
                                if screen_col < self.winsize.cols as usize {
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
                        } else if screen_col < self.winsize.cols as usize {
                            putchar(byte)?;
                            screen_col += 1;
                        } else {
                            break;
                        }
                    }

                    // Reset colors after printing a highlighted character
                    if is_highlight
                        && chars_to_skip == 0
                        && screen_col <= self.winsize.cols as usize
                    {
                        reset_colors()?;
                    }
                }
                // Ensure colors are reset at the end of each line
                reset_colors()?;
            }
        }

        // Move cursor to the correct position
        move_cursor(self.cursor_row, self.cursor_col)?;
        Ok(0)
    }

    // Print a message to the last line of the screen
    fn print_status<F>(&self, writer: F) -> SysResult
    where
        F: FnOnce() -> SysResult,
    {
        save_cursor()?;
        move_cursor(self.winsize.rows as usize - 1, 0)?;
        clear_line()?;
        writer()?;
        restore_cursor()
    }

    // Print a normal message to the status line
    pub(in crate::editor) fn print_message(&self, msg: &str) -> SysResult {
        self.print_status(|| puts(msg))
    }

    #[allow(dead_code)]
    // Print a warning message (yellow) to the status line
    pub(in crate::editor) fn print_warning(&self, msg: &str) -> SysResult {
        self.print_status(|| {
            set_fg_color(3)?;
            puts(msg)?;
            reset_colors()
        })
    }

    // Print an error message (bold red) to the status line
    pub(in crate::editor) fn print_error(&self, msg: &str) -> SysResult {
        self.print_status(|| {
            set_bold()?;
            set_fg_color(1)?;
            puts(msg)?;
            reset_colors()
        })
    }
}

#[cfg(test)]
pub mod tests {
    use crate::editor::file_buffer::tests::create_test_file_buffer;

    use super::*;

    #[test]
    fn test_update_winsize() {
        use crate::terminal::tests::{disable_test_mode, enable_test_mode};

        // Enable test mode to prevent terminal output
        enable_test_mode();

        // Create a test file buffer
        let content = b"Line 1\nLine 2\nLine 3\nLine 4\nLine 5\n";
        let buffer = create_test_file_buffer(content);

        // Create a test editor state with initial size
        let mut winsize = Winsize::new();
        winsize.rows = 10;
        winsize.cols = 40;
        let mut filename = [0u8; MAX_PATH];
        let test_name = b"test_file.txt\0";
        filename[..test_name.len()].copy_from_slice(test_name);
        let mut state = EditorState::new(winsize, &filename);
        state.buffer = buffer;

        // Position cursor at line 3, column 2
        state.file_row = 2;
        state.file_col = 2;
        state.scroll_to_cursor();

        // Create a new terminal size that's smaller
        let mut new_winsize = Winsize::new();
        new_winsize.rows = 5; // Smaller height
        new_winsize.cols = 20; // Smaller width

        // Update the window size
        state.update_winsize(new_winsize);

        // Check that the window size was updated
        assert_eq!(state.winsize.rows, 5, "Winsize rows should be updated");
        assert_eq!(state.winsize.cols, 20, "Winsize cols should be updated");

        // Verify that cursor positions were recalculated after resize
        assert_eq!(state.file_row, 2, "File row shouldn't change on resize");
        assert_eq!(state.file_col, 2, "File col shouldn't change on resize");

        // Clean up
        disable_test_mode();
    }

    #[test]
    fn test_switch_search_direction() {
        use crate::terminal::tests::{disable_test_mode, enable_test_mode};

        // Enable test mode to prevent terminal output
        enable_test_mode();

        // Create a simplified test file buffer with content that has multiple search matches
        let content = b"First search\nSecond line\nThird search\n";
        let buffer = create_test_file_buffer(content);

        // Create a test editor state
        let mut winsize = Winsize::new();
        winsize.rows = 24;
        winsize.cols = 80;
        let mut filename = [0u8; MAX_PATH];
        let test_name = b"test_file.txt\0";
        filename[..test_name.len()].copy_from_slice(test_name);
        let mut state = EditorState::new(winsize, &filename);
        state.buffer = buffer;

        // Start with forward search
        let _ = state.start_search(false);
        assert!(!state.search.reverse, "Should start in forward search mode");

        // Add a search query
        let _ = state.add_search_char(b's');
        let _ = state.add_search_char(b'e');
        let _ = state.add_search_char(b'a');
        let _ = state.add_search_char(b'r');
        let _ = state.add_search_char(b'c');
        let _ = state.add_search_char(b'h');

        // Switch to reverse search
        let _ = state.switch_search_direction();
        assert!(state.search.reverse, "Search should now be in reverse mode");

        // Switch back to forward search
        let _ = state.switch_search_direction();
        assert!(
            !state.search.reverse,
            "Search should be back in forward mode"
        );

        // Clean up
        state.cancel_search().unwrap();
        disable_test_mode();
    }

    #[test]
    fn test_editor_state_search() {
        use crate::terminal::tests::{disable_test_mode, enable_test_mode};

        // Enable test mode to prevent terminal output
        enable_test_mode();

        // Create a test file buffer with known content
        let content = b"First line\nSecond line\nThird line with search term\nFourth line\n";
        let buffer = create_test_file_buffer(content);

        // Create a test editor state
        let mut winsize = Winsize::new();
        winsize.rows = 24;
        winsize.cols = 80;
        let mut filename = [0u8; MAX_PATH];
        let test_name = b"test_file.txt\0";
        filename[..test_name.len()].copy_from_slice(test_name);
        let mut state = EditorState::new(winsize, &filename);
        state.buffer = buffer;

        // Test starting search
        assert!(!state.search.mode, "Search mode should initially be off");
        let _ = state.start_search(false);
        assert!(
            state.search.mode,
            "Search mode should be on after start_search"
        );
        assert_eq!(state.search.query_len, 0, "Search query should be empty");

        // Test adding characters to search query
        let _ = state.add_search_char(b's');
        let _ = state.add_search_char(b'e');
        let _ = state.add_search_char(b'a');
        let _ = state.add_search_char(b'r');
        let _ = state.add_search_char(b'c');
        let _ = state.add_search_char(b'h');

        assert_eq!(
            state.search.query_len, 6,
            "Search query should have 6 characters"
        );

        // Verify the search query
        assert_eq!(state.search.query[0], b's', "First char should be 's'");
        assert_eq!(state.search.query[1], b'e', "Second char should be 'e'");
        assert_eq!(state.search.query[2], b'a', "Third char should be 'a'");
        assert_eq!(state.search.query[3], b'r', "Fourth char should be 'r'");
        assert_eq!(state.search.query[4], b'c', "Fifth char should be 'c'");
        assert_eq!(state.search.query[5], b'h', "Sixth char should be 'h'");

        // Verify that match was found in the expected location
        assert_eq!(state.search.match_row, 2, "Match should be found on line 3");
        assert_eq!(
            state.search.match_col, 16,
            "Match should start at column 16"
        );
        assert_eq!(state.search.match_len, 6, "Match should be 6 chars long");

        // Test backspacing in the search query
        let _ = state.remove_search_char();
        assert_eq!(
            state.search.query_len, 5,
            "Search query should have 5 characters after backspace"
        );

        // Test canceling search
        let initial_row = state.search.orig_row;
        let initial_col = state.search.orig_col;

        let _ = state.cancel_search();

        assert!(
            !state.search.mode,
            "Search mode should be off after canceling"
        );
        assert_eq!(
            state.file_row, initial_row,
            "Cursor row should return to original position"
        );
        assert_eq!(
            state.file_col, initial_col,
            "Cursor col should return to original position"
        );

        // Clean up test mode
        disable_test_mode();
    }

    #[test]
    fn test_editor_state_search_navigation() {
        use crate::terminal::tests::{disable_test_mode, enable_test_mode};

        // Enable test mode to prevent terminal output
        enable_test_mode();

        // Create a test file buffer with known content plus additional content
        let content = b"First line\nSecond line\nThird line with search term\nFourth line\n";
        let additional_content = b"Fifth line with another search term\n";
        let mut new_content = Vec::with_capacity(content.len() + additional_content.len());
        new_content.extend_from_slice(content);
        new_content.extend_from_slice(additional_content);

        let buffer = create_test_file_buffer(&new_content);

        // Create a test editor state
        let mut winsize = Winsize::new();
        winsize.rows = 24;
        winsize.cols = 80;
        let mut filename = [0u8; MAX_PATH];
        let test_name = b"test_file.txt\0";
        filename[..test_name.len()].copy_from_slice(test_name);
        let mut state = EditorState::new(winsize, &filename);
        state.buffer = buffer;

        // Start search (forward)
        let _ = state.start_search(false);
        let _ = state.add_search_char(b's');
        let _ = state.add_search_char(b'e');
        let _ = state.add_search_char(b'a');
        let _ = state.add_search_char(b'r');
        let _ = state.add_search_char(b'c');
        let _ = state.add_search_char(b'h');

        // Verify first match was found
        assert_eq!(state.search.match_row, 2, "Match should be found on line 3");
        assert_eq!(
            state.search.match_col, 16,
            "Match should start at column 16"
        );

        // Test find_next_match to find the second instance
        let _ = state.find_next_match();

        // Verify that next match was found
        assert_eq!(
            state.search.match_row, 4,
            "Second match should be found on line 5"
        );
        assert_eq!(
            state.search.match_col, 24,
            "Second match should start at column 24"
        );
        assert_eq!(
            state.search.match_len, 6,
            "Second match should be 6 chars long"
        );

        // Test accepting search
        let match_row = state.file_row;
        let match_col = state.file_col;

        let _ = state.accept_search();

        assert!(
            !state.search.mode,
            "Search mode should be off after accepting"
        );
        assert_eq!(
            state.file_row, match_row,
            "Cursor row should stay at match position"
        );
        assert_eq!(
            state.file_col, match_col,
            "Cursor col should stay at match position"
        );

        // Clean up test mode
        disable_test_mode();
    }

    #[test]
    fn test_editor_status_functions() {
        use crate::terminal::tests::{disable_test_mode, enable_test_mode};

        // Create a test editor state
        let mut winsize = Winsize::new();
        winsize.rows = 24;
        winsize.cols = 80;

        let state = EditorState::new(winsize, &[0; MAX_PATH]);

        // Test print_message
        enable_test_mode();
        let result = state.print_message("Test normal message");

        unsafe {
            assert!(result.is_ok(), "print_message should work in test mode");
            assert!(crate::terminal::tests::TEST_BUFFER_LEN > 0);
            assert_eq!(crate::terminal::tests::TEST_BUFFER[0], b'\x1b');
        }

        // Test print_warning
        enable_test_mode();
        let result = state.print_warning("Test warning message");

        unsafe {
            assert!(result.is_ok(), "print_warning should work in test mode");
            assert!(crate::terminal::tests::TEST_BUFFER_LEN > 0);
            assert_eq!(crate::terminal::tests::TEST_BUFFER[0], b'\x1b');
        }

        // Test print_error
        enable_test_mode();
        let result = state.print_error("Test error message");

        unsafe {
            assert!(result.is_ok(), "print_error should work in test mode");
            assert!(crate::terminal::tests::TEST_BUFFER_LEN > 0);
            assert_eq!(crate::terminal::tests::TEST_BUFFER[0], b'\x1b');
        }

        // Test print_status
        enable_test_mode();
        let result = state.print_status(|| puts("Test status message"));

        unsafe {
            assert!(result.is_ok(), "print_status should work in test mode");
            assert!(crate::terminal::tests::TEST_BUFFER_LEN > 0);
            assert_eq!(crate::terminal::tests::TEST_BUFFER[0], b'\x1b');
        }

        disable_test_mode();
    }
}
