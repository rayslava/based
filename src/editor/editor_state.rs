use crate::{
    syscall::{MAX_PATH, STDOUT, SysResult, putchar, puts, write_buf, write_unchecked},
    terminal::{
        clear_line, move_cursor, reset_colors, restore_cursor, save_cursor, set_bg_color, set_bold,
        set_fg_color, write_number,
    },
    termios::Winsize,
};

use super::{
    FileBuffer, KillRing, KillRingError, SearchState, SyntaxHighlighter,
    syntax_highlight::HighlightColor,
};

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
    pub(in crate::editor) highlighter: SyntaxHighlighter, // Syntax highlighter
    pub(in crate::editor) mark_active: bool,   // Whether mark is active for selection
    pub(in crate::editor) mark_row: usize,     // Mark row position for selection
    pub(in crate::editor) mark_col: usize,     // Mark column position for selection
    pub(in crate::editor) kill_ring: KillRing, // Kill-ring for cut/copy/paste operations
}

impl EditorState {
    // Create a new editor state
    pub(in crate::editor) fn new(winsize: Winsize, filename: &[u8; MAX_PATH]) -> Self {
        let mut own_filename = [0u8; MAX_PATH];
        own_filename.copy_from_slice(filename);

        // Create and initialize the syntax highlighter
        let mut highlighter = SyntaxHighlighter::new();
        highlighter.detect_file_type(filename);

        // Create kill-ring
        // We need to explicitly create a KillRing since we can't access private fields
        let kill_ring = KillRing::new().unwrap_or_else(|_| {
            // Try again in case it was a temporary issue
            KillRing::new().expect("Failed to create KillRing twice")
        });

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
            highlighter,
            mark_active: false,
            mark_row: 0,
            mark_col: 0,
            kill_ring,
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
    fn display_search_direction(&self) -> SysResult {
        if self.search.reverse {
            puts("Reverse search")
        } else {
            puts("Search")
        }
    }

    fn display_case_sensitivity(&self) -> SysResult {
        if self.search.case_sensitive {
            puts(" (case-sensitive): ")
        } else {
            puts(" (case-insensitive): ")
        }
    }

    fn display_search_query(&self) -> SysResult {
        write_unchecked(STDOUT, self.search.query.as_ptr(), self.search.query_len)
    }

    fn display_complete_search_prompt(&self) -> SysResult {
        let mut result = self.display_search_direction();
        if result.is_ok() {
            result = self.display_case_sensitivity();
        }
        if result.is_ok() {
            result = self.display_search_query();
        }
        result
    }

    pub(in crate::editor) fn start_search(&mut self, reverse: bool) -> SysResult {
        // Save current position to return to if search is cancelled
        self.search.orig_row = self.file_row;
        self.search.orig_col = self.file_col;

        // Reset search state
        self.search.mode = true;
        self.search.reverse = reverse;
        self.search.query_len = 0;
        self.search.match_len = 0; // Reset match length
        self.search.case_sensitive = false; // Default to case-insensitive

        // Clear the query array
        let mut i = 0;
        while i < self.search.query.len() {
            self.search.query[i] = 0;
            i += 1;
        }

        self.print_status(|| self.display_complete_search_prompt())
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

    fn chars_match_case_sensitive(line_ch: u8, query_ch: u8, case_sensitive: bool) -> bool {
        if case_sensitive {
            line_ch == query_ch
        } else {
            let line_ch_lower = if line_ch.is_ascii_uppercase() {
                line_ch + 32
            } else {
                line_ch
            };

            let query_ch_lower = if query_ch.is_ascii_uppercase() {
                query_ch + 32
            } else {
                query_ch
            };

            line_ch_lower == query_ch_lower
        }
    }

    fn validate_current_match(&mut self) -> bool {
        match self.buffer.get_line(self.file_row) {
            Some(line) if self.file_col + self.search.query_len <= line.len() => {
                let query = &self.search.query[..self.search.query_len];

                for j in 0..self.search.query_len {
                    let line_ch = line[self.file_col + j];
                    let query_ch = query[j];

                    if !Self::chars_match_case_sensitive(
                        line_ch,
                        query_ch,
                        self.search.case_sensitive,
                    ) {
                        return false;
                    }
                }

                self.search.match_row = self.file_row;
                self.search.match_col = self.file_col;
                self.search.match_len = self.search.query_len;
                true
            }
            _ => false,
        }
    }

    fn execute_search(&mut self) -> SysResult {
        let search_result = if self.search.reverse {
            self.search
                .find_substring_backward(&self.buffer, self.file_row, self.file_col)
        } else {
            self.search
                .find_substring_forward(&self.buffer, self.file_row, self.file_col)
        };

        match search_result {
            Some((row, col, len)) => {
                self.search.match_row = row;
                self.search.match_col = col;
                self.search.match_len = len;
                self.file_row = row;
                self.file_col = col;
                self.scroll_to_cursor();
                self.draw_screen()
            }
            None => self.print_warning("No match found"),
        }
    }

    pub(in crate::editor) fn update_search(&mut self) -> SysResult {
        if self.search.query_len == 0 {
            return Ok(0);
        }

        let match_found = self.search.match_len > 0 && self.validate_current_match();

        if match_found {
            self.draw_screen()
        } else {
            self.execute_search()
        }
    }

    // Add a character to the search query
    pub(in crate::editor) fn add_search_char(&mut self, ch: u8) -> SysResult {
        // Check if the query still has space for one more character
        if self.search.query_len < self.search.query.len() - 1 {
            // Add the character to the query
            self.search.query[self.search.query_len] = ch;
            self.search.query_len += 1;

            // Update search prompt
            let result = self.print_status(|| self.display_complete_search_prompt());

            // If status update was successful, search for matches
            match result {
                Ok(_) => self.update_search(),
                err => err,
            }
        } else {
            // Query is too long, show error
            self.print_error("Search query too long")
        }
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
        let result = self.print_status(|| self.display_complete_search_prompt());

        // Draw screen if status update was successful
        match result {
            Ok(_) => self.draw_screen(),
            err => err,
        }
    }

    // Toggle case sensitivity in search mode
    pub(in crate::editor) fn toggle_search_case_sensitivity(&mut self) -> SysResult {
        // Only proceed if in search mode
        if !self.search.mode {
            return Ok(0);
        }

        // Toggle case sensitivity
        self.search.toggle_case_sensitivity();

        // Update the status line to show the case sensitivity state
        let result = self.print_status(|| self.display_complete_search_prompt());

        // Only continue if status update was successful and there's a query to search for
        match (result, self.search.query_len > 0) {
            (Ok(_), true) => {
                // Reset the match length to force a new search
                self.search.match_len = 0;

                // Update search with new case sensitivity setting
                let search_result = if self.search.reverse {
                    self.search
                        .find_substring_backward(&self.buffer, self.file_row, self.file_col)
                } else {
                    self.search
                        .find_substring_forward(&self.buffer, self.file_row, self.file_col)
                };

                // Process search result
                if let Some((row, col, len)) = search_result {
                    // Found a match with new case sensitivity setting
                    self.search.match_row = row;
                    self.search.match_col = col;
                    self.search.match_len = len;
                    self.file_row = row;
                    self.file_col = col;
                    self.scroll_to_cursor();
                    self.draw_screen()?;
                    Ok(0)
                } else {
                    // No match found with new case sensitivity setting
                    self.print_warning("No match found")?;
                    Ok(0)
                }
            }
            (Ok(_), false) => Ok(0), // No query to search for
            (err, _) => err,         // Propagate error from status update
        }
    }

    // Remove the last character from the search query
    pub(in crate::editor) fn remove_search_char(&mut self) -> SysResult {
        // Only proceed if there is at least one character in the query
        if self.search.query_len == 0 {
            Ok(0)
        } else {
            // Remove the last character
            self.search.query_len -= 1;

            // Update search prompt
            let result = self.print_status(|| self.display_complete_search_prompt());

            // Process the updated query if status update was successful
            match result {
                Ok(_) => {
                    if self.search.query_len > 0 {
                        // Adjust match length if needed
                        if self.search.match_len > self.search.query_len {
                            self.search.match_len = self.search.query_len;
                        }

                        // Search for new matches with updated query
                        self.update_search()
                    } else {
                        // No characters left in query, reset to original position
                        self.file_row = self.search.orig_row;
                        self.file_col = self.search.orig_col;
                        self.search.match_len = 0; // Clear highlights

                        // Update screen display
                        self.scroll_to_cursor();
                        self.draw_screen()
                    }
                }
                err => err,
            }
        }
    }

    // Find the next match for the current search query
    pub(in crate::editor) fn find_next_match(&mut self) -> SysResult {
        // Only proceed if there is a query to search for
        if self.search.query_len == 0 {
            Ok(0)
        } else {
            // Determine search starting position based on direction
            let (search_row, search_col) = if self.search.reverse {
                // For reverse search, calculate starting position
                match (self.search.match_col, self.search.match_row) {
                    (col, _) if col > 0 => {
                        // Start at the position before the current match
                        (self.search.match_row, col - 1)
                    }
                    (_, row) if row > 0 => {
                        // We're at the start of a line, so move to the end of previous line
                        let prev_row = row - 1;
                        let prev_line_len = match self.buffer.get_line(prev_row) {
                            Some(line) => line.len(),
                            None => 0,
                        };
                        (prev_row, prev_line_len)
                    }
                    _ => {
                        // We're at the beginning of the file, wrap around to the end
                        let last_row = self.buffer.count_lines() - 1;
                        let last_line_len = match self.buffer.get_line(last_row) {
                            Some(line) => line.len(),
                            None => 0,
                        };
                        (last_row, last_line_len)
                    }
                }
            } else {
                // For forward search, start after the end of the current match
                (
                    self.search.match_row,
                    self.search.match_col + self.search.match_len,
                )
            };

            // Execute the search in the appropriate direction
            let search_result = if self.search.reverse {
                self.search
                    .find_substring_backward(&self.buffer, search_row, search_col)
            } else {
                self.search
                    .find_substring_forward(&self.buffer, search_row, search_col)
            };

            // Process the search result
            match search_result {
                Some((row, col, len)) => {
                    // Store the match information
                    self.search.match_row = row;
                    self.search.match_col = col;
                    self.search.match_len = len;

                    // Move cursor to the match position
                    self.file_row = row;
                    self.file_col = col;
                    self.scroll_to_cursor();

                    // Update the display
                    self.draw_screen()
                }
                None => {
                    // No more matches found
                    self.print_warning("No more matches")
                }
            }
        }
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
            self.file_col = if self.preferred_col > new_line_len {
                new_line_len
            } else {
                self.preferred_col
            };
        }
    }

    pub(in crate::editor) fn cursor_down(&mut self) {
        let line_count = self.buffer.count_lines();
        if self.file_row + 1 < line_count {
            // Move to the next line
            self.file_row += 1;

            // Set cursor column to either preferred position or end of line
            let new_line_len = self.buffer.line_length(self.file_row, self.tab_size);
            self.file_col = if self.preferred_col > new_line_len {
                new_line_len
            } else {
                self.preferred_col
            };
        }
    }

    pub(in crate::editor) fn cursor_left(&mut self) {
        match (self.file_col > 0, self.file_row > 0) {
            (true, _) => {
                // Not at beginning of line, move left
                self.file_col -= 1;
                // Update preferred column
                self.preferred_col = self.file_col;
            }
            (false, true) => {
                // At beginning of line, move to end of previous line
                self.file_row -= 1;
                self.file_col = self.buffer.line_length(self.file_row, self.tab_size);
                self.preferred_col = self.file_col;
            }
            (false, false) => {} // At beginning of file, do nothing
        }
    }

    pub(in crate::editor) fn cursor_right(&mut self) {
        let current_line_len = self.buffer.line_length(self.file_row, self.tab_size);
        let line_count = self.buffer.count_lines();

        match (
            self.file_col < current_line_len,
            self.file_row + 1 < line_count,
        ) {
            (true, _) => {
                // Not at end of line, move right
                self.file_col += 1;
                self.preferred_col = self.file_col;
            }
            (false, true) => {
                // At end of line but not last line, move to beginning of next line
                self.file_row += 1;
                self.file_col = 0;
                self.preferred_col = self.file_col;
            }
            (false, false) => {} // At end of file, do nothing
        }
    }

    #[inline]
    fn is_alnum(ch: u8) -> bool {
        ch.is_ascii_alphanumeric()
    }

    fn find_word_start(line: &[u8], col: usize) -> usize {
        if line.is_empty() {
            return 0;
        }

        let mut pos = col.min(line.len() - 1);
        while pos > 0 && !Self::is_alnum(line[pos]) {
            pos -= 1;
        }
        while pos > 0 && Self::is_alnum(line[pos - 1]) {
            pos -= 1;
        }

        pos
    }

    fn skip_current_word(line: &[u8], start_col: usize) -> usize {
        let mut col = start_col;
        while col < line.len() && !Self::is_alnum(line[col]) {
            col += 1;
        }
        while col < line.len() && Self::is_alnum(line[col]) {
            col += 1;
        }
        col
    }

    fn find_word_forward(&self, line_count: usize) -> (usize, usize) {
        match self.buffer.get_line(self.file_row) {
            None if self.file_row + 1 < line_count => (self.file_row + 1, 0),
            None => (self.file_row, self.file_col),
            Some(line) if line.is_empty() || self.file_col >= line.len() => {
                if self.file_row + 1 < line_count {
                    (self.file_row + 1, 0)
                } else {
                    (self.file_row, self.file_col)
                }
            }
            Some(line) => {
                let new_col = EditorState::skip_current_word(line, self.file_col);
                if new_col >= line.len() && self.file_row + 1 < line_count {
                    (self.file_row + 1, 0)
                } else {
                    (self.file_row, new_col)
                }
            }
        }
    }

    fn find_prev_nonempty_line(&self) -> Option<(usize, usize)> {
        if self.file_row == 0 {
            return None;
        }

        let mut new_row = self.file_row - 1;
        loop {
            if let Some(line) = self.buffer.get_line(new_row) {
                if !line.is_empty() {
                    let col = line.len().saturating_sub(1);
                    let col = EditorState::find_word_start(line, col);
                    return Some((new_row, col));
                }
            }

            if new_row == 0 {
                break;
            }
            new_row -= 1;
        }
        None
    }

    fn find_word_backward(&self) -> (usize, usize) {
        match self.buffer.get_line(self.file_row) {
            None => (self.file_row, self.file_col),
            Some(line) if line.is_empty() || self.file_col == 0 => self
                .find_prev_nonempty_line()
                .unwrap_or((self.file_row, self.file_col)),
            Some(line) => {
                let adj_col = if self.file_col >= line.len() {
                    line.len().saturating_sub(1)
                } else {
                    self.file_col
                };

                let col = if adj_col < line.len()
                    && Self::is_alnum(line[adj_col])
                    && (adj_col == 0 || !Self::is_alnum(line[adj_col - 1]))
                {
                    if adj_col > 0 {
                        EditorState::find_word_start(line, adj_col - 1)
                    } else {
                        adj_col
                    }
                } else {
                    EditorState::find_word_start(line, adj_col)
                };

                (self.file_row, col)
            }
        }
    }

    pub(in crate::editor) fn find_word_boundary(&mut self, direction: bool) {
        let line_count = self.buffer.count_lines();
        if line_count == 0 || self.file_row >= line_count {
            return;
        }

        let (new_row, new_col) = if direction {
            self.find_word_forward(line_count)
        } else {
            self.find_word_backward()
        };

        self.file_row = new_row;
        self.file_col = new_col;
        self.preferred_col = new_col;
    }

    // Move cursor forward by one word
    pub(in crate::editor) fn cursor_word_forward(&mut self) {
        self.find_word_boundary(true);
    }

    // Move cursor backward by one word
    pub(in crate::editor) fn cursor_word_backward(&mut self) {
        self.find_word_boundary(false);
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
        self.file_col = if self.file_col > current_line_len {
            current_line_len
        } else {
            self.file_col
        };

        self.cursor_row = self.file_row - self.scroll_row;
    }

    pub(in crate::editor) fn page_down(&mut self) {
        let lines_to_scroll = self.editing_rows();
        let line_count = self.buffer.count_lines();

        // Move cursor down by screen height or to the last line
        self.file_row = if self.file_row + lines_to_scroll < line_count {
            self.file_row + lines_to_scroll
        } else {
            line_count - 1
        };

        // Adjust scroll position if needed
        let max_scroll_row = self.file_row.saturating_sub(self.editing_rows()) + 1;
        self.scroll_row = if max_scroll_row > 0 {
            max_scroll_row
        } else {
            self.scroll_row
        };

        // Make sure cursor doesn't go beyond the end of the current line
        let current_line_len = self.buffer.line_length(self.file_row, self.tab_size);
        self.file_col = if self.file_col > current_line_len {
            current_line_len
        } else {
            self.file_col
        };

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
        clear_line()?;
        reset_colors()?;
        restore_cursor()
    }

    // Handle tabs in the editor display - convert to spaces
    fn handle_tab_display(
        &self,
        _tab_byte: u8,
        col: &mut usize,
        screen_col: &mut usize,
        chars_to_skip: &mut usize,
    ) -> SysResult {
        let spaces = self.tab_size - (*col % self.tab_size);
        *col += spaces;

        // Skip if we're still scrolled horizontally
        if *chars_to_skip > 0 {
            if *chars_to_skip >= spaces {
                *chars_to_skip -= spaces;
                return Ok(0);
            }

            // Draw partial spaces after the horizontal scroll point
            let visible_spaces = spaces - *chars_to_skip;
            for _ in 0..visible_spaces {
                if *screen_col < self.winsize.cols as usize {
                    putchar(b' ')?;
                    *screen_col += 1;
                } else {
                    break;
                }
            }
            *chars_to_skip = 0;
        } else {
            // Draw spaces for tab
            for _ in 0..spaces {
                if *screen_col < self.winsize.cols as usize {
                    putchar(b' ')?;
                    *screen_col += 1;
                } else {
                    break;
                }
            }
        }

        Ok(0)
    }

    pub(in crate::editor) fn draw_screen(&mut self) -> SysResult {
        // Calculate available height for content
        let available_rows = self.editing_rows();
        let line_count = self.buffer.count_lines();

        // Draw lines from the file buffer
        for i in 0..available_rows {
            move_cursor(i, 0)?;
            clear_line()?;

            let file_line_idx = self.scroll_row + i;
            if file_line_idx >= line_count {
                // We're past the end of file, leave the rest of screen empty
                continue;
            }

            // Process this line
            self.draw_line_at_index(file_line_idx)?;
        }

        // Move cursor to the correct position
        move_cursor(self.cursor_row, self.cursor_col)?;
        Ok(0)
    }

    // Draw a specific line in the buffer by its index
    fn draw_line_at_index(&mut self, file_line_idx: usize) -> SysResult {
        // Get the line
        if let Some(line) = self.buffer.get_line(file_line_idx) {
            if line.is_empty() {
                return Ok(0);
            }

            // Get line info
            let line_start = self.buffer.find_line_start(file_line_idx).unwrap_or(0);

            // Check for search match
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

            // Calculate how much to skip from the start (for horizontal scrolling)
            let mut chars_to_skip = self.scroll_col;
            let mut col = 0;
            let mut screen_col = 0;

            // Display each character in the line
            let mut idx = 0;
            while idx < line.len() {
                let byte = line[idx];

                if byte == 0 {
                    // Stop at null byte
                    break;
                }

                // Track if current character is part of a search match for highlighting
                let is_highlight = is_match_line && idx >= match_start && idx < match_end;

                // Get syntax highlight color directly
                let abs_pos = line_start + idx;
                let syntax_highlight = self.highlighter.highlight_char(&self.buffer, abs_pos);

                // Apply highlighting if visible
                if chars_to_skip == 0 && screen_col < self.winsize.cols as usize {
                    // Check if the character is part of a selection
                    let is_selected = self.is_position_selected(file_line_idx, idx);

                    if is_selected {
                        // Selection highlighting takes precedence over everything
                        set_bg_color(4)?; // Blue background
                        set_fg_color(7)?; // White text
                    } else if is_highlight {
                        // Search match highlighting takes precedence after selection
                        set_bg_color(7)?;
                        set_fg_color(0)?;
                    } else if let HighlightColor::Delimiter = syntax_highlight {
                        // For delimiters, use cyan
                        set_fg_color(6)?;
                    } else {
                        // Apply regular syntax highlighting based on character type
                        match syntax_highlight {
                            HighlightColor::Default | HighlightColor::Delimiter => {} // No color change
                            HighlightColor::Comment => {
                                set_fg_color(2)?; // Green for comments
                            }
                            HighlightColor::Keyword => {
                                set_fg_color(4)?; // Blue for keywords
                                set_bold()?;
                            }
                            HighlightColor::String => {
                                set_fg_color(1)?; // Red for strings
                            }
                            HighlightColor::Number => {
                                set_fg_color(5)?; // Magenta for numbers
                            }
                        }
                    }
                }

                // Handle the character display
                if byte == b'\t' {
                    self.handle_tab_display(byte, &mut col, &mut screen_col, &mut chars_to_skip)?;
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

                // Reset colors after printing if needed
                let is_selected = self.is_position_selected(file_line_idx, idx);
                if (is_selected || is_highlight || syntax_highlight != HighlightColor::Default)
                    && chars_to_skip == 0
                    && screen_col <= self.winsize.cols as usize
                {
                    reset_colors()?;
                }

                // Move to next character
                idx += 1;
            }

            // Ensure colors are reset at the end of each line
            reset_colors()?;
        }

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

    // Set mark at current cursor position
    pub(in crate::editor) fn set_mark(&mut self) -> SysResult {
        self.mark_active = true;
        self.mark_row = self.file_row;
        self.mark_col = self.file_col;
        self.print_message("Mark set")
    }

    // Clear mark (deactivate selection)
    pub(in crate::editor) fn clear_mark(&mut self) -> SysResult {
        self.mark_active = false;
        self.draw_screen()?;
        self.print_message("Mark cleared")
    }

    // Get selected text range (ordered by position)
    fn get_selection_range(&self) -> ((usize, usize), (usize, usize)) {
        let (start_row, start_col, end_row, end_col) = if self.file_row < self.mark_row
            || (self.file_row == self.mark_row && self.file_col < self.mark_col)
        {
            (self.file_row, self.file_col, self.mark_row, self.mark_col)
        } else {
            (self.mark_row, self.mark_col, self.file_row, self.file_col)
        };

        ((start_row, start_col), (end_row, end_col))
    }

    // Calculate total selection size in bytes
    fn selection_size(&self) -> usize {
        if !self.mark_active {
            return 0;
        }

        let ((start_row, start_col), (end_row, end_col)) = self.get_selection_range();

        // If selection is within the same line
        if start_row == end_row {
            return end_col - start_col;
        }

        // For multiline selections, we need to count each line's contribution
        let mut size = 0;

        // First line: from start to end of line
        if let Some(line) = self.buffer.get_line(start_row) {
            size += line.len().saturating_sub(start_col);
            size += 1; // Include newline
        }

        // Middle lines (including whole line and newline)
        for row in (start_row + 1)..end_row {
            if let Some(line) = self.buffer.get_line(row) {
                size += line.len() + 1; // Include newline
            }
        }

        // Last line: from beginning to cursor
        if let Some(line) = self.buffer.get_line(end_row) {
            if end_col <= line.len() {
                size += end_col;
            } else {
                size += line.len();
            }
        }

        size
    }

    // Copy selected text to the kill-ring
    pub(in crate::editor) fn copy_selection(&mut self) -> SysResult {
        if !self.mark_active {
            return self.print_message("No selection (mark not active)");
        }

        let selection_size = self.selection_size();
        if selection_size == 0 {
            return self.print_message("Empty selection");
        } else if selection_size > self.kill_ring.capacity() {
            return self.print_error("Selection too large for kill-ring");
        }

        let ((start_row, start_col), (end_row, end_col)) = self.get_selection_range();

        // Create a fixed-size buffer for the selected text
        let mut selected_text = [0u8; 4096]; // Maximum size (one page)
        let mut text_len = 0;

        // Copy text from buffer to the selected_text array
        if start_row == end_row {
            // Single line selection
            if let Some(line) = self.buffer.get_line(start_row) {
                let end_idx = end_col.min(line.len());
                // Use indices as we need to control text_len
                let end = end_idx.min(line.len());
                let mut idx = start_col;
                while idx < end && text_len < selected_text.len() {
                    selected_text[text_len] = line[idx];
                    text_len += 1;
                    idx += 1;
                }
            }
        } else {
            // Multiline selection

            // First line - from start_col to end of line
            if let Some(line) = self.buffer.get_line(start_row) {
                // Use indices as we need to control text_len
                let mut idx = start_col;
                while idx < line.len() && text_len < selected_text.len() {
                    selected_text[text_len] = line[idx];
                    text_len += 1;
                    idx += 1;
                }
                if text_len < selected_text.len() {
                    selected_text[text_len] = b'\n';
                    text_len += 1;
                }
            }

            // Middle lines (complete lines)
            for row in (start_row + 1)..end_row {
                if let Some(line) = self.buffer.get_line(row) {
                    for &ch in line {
                        if text_len < selected_text.len() {
                            selected_text[text_len] = ch;
                            text_len += 1;
                        }
                    }
                    if text_len < selected_text.len() {
                        selected_text[text_len] = b'\n';
                        text_len += 1;
                    }
                }
            }

            // Last line - from beginning to end_col
            if let Some(line) = self.buffer.get_line(end_row) {
                let end_idx = end_col.min(line.len());
                // Use indices as we need to control text_len
                let end = end_idx.min(line.len());
                let mut idx = 0;
                while idx < end && text_len < selected_text.len() {
                    selected_text[text_len] = line[idx];
                    text_len += 1;
                    idx += 1;
                }
            }
        }

        // Copy to kill-ring
        match self.kill_ring.copy(&selected_text[..text_len]) {
            Ok(()) => {
                // Deactivate the mark after copying
                self.mark_active = false;
                self.draw_screen()?;
                self.print_message("Copied selection to kill-ring")
            }
            Err(KillRingError::BufferTooLarge) => {
                self.print_error("Selection too large for kill-ring")
            }
            Err(_) => self.print_error("Failed to copy selection"),
        }
    }

    // Delete selected text and copy to kill-ring
    pub(in crate::editor) fn cut_selection(&mut self) -> SysResult {
        if !self.mark_active {
            return self.print_message("No selection (mark not active)");
        }

        // First copy the selection to kill-ring
        let copy_result = self.copy_selection();
        copy_result?;

        // Now delete the selection
        let ((start_row, start_col), (end_row, end_col)) = self.get_selection_range();

        // Delete selection from buffer
        if start_row == end_row {
            // Single line selection - delete characters from start_col to end_col
            for _ in start_col..end_col {
                let result = self.buffer.delete_char(start_row, start_col);
                if result.is_err() {
                    return self.print_error("Failed to delete selection");
                }
            }
        } else {
            // Multiline selection

            // Delete all full lines between start and end
            for _ in (start_row + 1)..end_row {
                // Delete line start_row+1 (which becomes the next line after start_row repeatedly)
                // Each time we delete a line, the later lines shift up
                let Some(line_end) = self.buffer.find_line_end(start_row) else {
                    return self.print_error("Failed to find line end");
                };

                let result = self.buffer.delete_at_position(line_end);
                if result.is_err() {
                    return self.print_error("Failed to delete line in selection");
                }
            }

            // Delete partial last line (from beginning to end_col)
            for _ in 0..end_col {
                let result = self.buffer.delete_char(start_row + 1, 0);
                if result.is_err() {
                    return self.print_error("Failed to delete end of selection");
                }
            }

            // Delete partial first line (from start_col to end)
            let first_line_len = match self.buffer.get_line(start_row) {
                Some(line) => line.len(),
                None => return self.print_error("Failed to get line length"),
            };

            // Calculate how many characters to delete
            let chars_to_delete = first_line_len - start_col;
            // Delete multiple characters at the same position
            for _ in 0..chars_to_delete {
                let result = self.buffer.delete_char(start_row, start_col);
                if result.is_err() {
                    return self.print_error("Failed to delete start of selection");
                }
            }

            // Join the remaining parts of the first and last lines
            let Some(pos) = self.buffer.find_line_end(start_row) else {
                return self.print_error("Failed to find line end");
            };
            let result = self.buffer.delete_at_position(pos);

            if result.is_err() {
                return self.print_error("Failed to join lines after deleting selection");
            }
        }

        // Move cursor to start of selection and deactivate mark
        self.file_row = start_row;
        self.file_col = start_col;
        self.mark_active = false;

        // Update screen
        self.buffer.modified = true;
        self.scroll_to_cursor();
        self.draw_screen()?;

        self.print_message("Cut selection to kill-ring")
    }

    // Paste text from kill-ring at current position
    pub(in crate::editor) fn paste_from_kill_ring(&mut self) -> SysResult {
        let kill_ring_content = self.kill_ring.content();
        if kill_ring_content.is_empty() {
            return self.print_message("Kill-ring is empty");
        }

        // Insert text from kill-ring
        for &byte in kill_ring_content {
            if byte == b'\n' {
                // Insert a newline
                match self.buffer.insert_newline(self.file_row, self.file_col) {
                    Ok(()) => {
                        self.file_row += 1;
                        self.file_col = 0;
                    }
                    Err(_) => {
                        return self.print_error("Failed to insert newline during paste");
                    }
                }
            } else {
                // Insert a regular character
                match self.buffer.insert_char(self.file_row, self.file_col, byte) {
                    Ok(()) => {
                        self.file_col += 1;
                    }
                    Err(_) => {
                        return self.print_error("Failed to insert character during paste");
                    }
                }
            }
        }

        // Update buffer and screen
        self.buffer.modified = true;
        self.scroll_to_cursor();
        self.draw_screen()?;

        self.print_message("Pasted from kill-ring")
    }

    // Kill (cut) from cursor to end of line
    pub(in crate::editor) fn kill_line(&mut self) -> SysResult {
        let line_opt = self.buffer.get_line(self.file_row);

        // If no line or at empty line, just add a newline and return
        if line_opt.is_none() || line_opt.unwrap().is_empty() {
            return self.print_message("Line is empty");
        }

        let line = line_opt.unwrap();

        // If already at end of line, delete the newline
        if self.file_col >= line.len() {
            if let Some(end_pos) = self.buffer.find_line_end(self.file_row) {
                let result = self.buffer.delete_at_position(end_pos);
                if result.is_err() {
                    return self.print_error("Failed to delete newline");
                }
                return self.print_message("Killed newline");
            }
            return self.print_message("Already at end of buffer");
        }

        // Otherwise, copy text from cursor to end of line into kill-ring
        let text_to_kill = &line[self.file_col..];
        match self.kill_ring.copy(text_to_kill) {
            Ok(()) => {
                // Successfully copied to kill-ring, now delete from buffer
                let chars_to_delete = line.len() - self.file_col;
                for _ in 0..chars_to_delete {
                    let result = self.buffer.delete_char(self.file_row, self.file_col);
                    if result.is_err() {
                        return self.print_error("Failed to delete character");
                    }
                }

                // Update buffer and screen
                self.buffer.modified = true;
                self.draw_screen()?;

                self.print_message("Killed to end of line")
            }
            Err(KillRingError::BufferTooLarge) => self.print_error("Text too large for kill-ring"),
            Err(_) => self.print_error("Failed to copy to kill-ring"),
        }
    }

    // Highlight selection during drawing if mark is active
    fn is_position_selected(&self, row: usize, col: usize) -> bool {
        if !self.mark_active {
            return false;
        }

        let ((start_row, start_col), (end_row, end_col)) = self.get_selection_range();

        match row.cmp(&start_row) {
            // Before start row - not selected
            core::cmp::Ordering::Less => false,

            // On start row - check if after start column
            core::cmp::Ordering::Equal => col >= start_col && (row != end_row || col <= end_col),

            // After start row - check if before end row or on end row before or at end column
            core::cmp::Ordering::Greater => row < end_row || (row == end_row && col <= end_col),
        }
    }
}

#[cfg(test)]
pub mod tests {
    use crate::editor::file_buffer::tests::create_test_file_buffer;

    use super::*;

    #[test]
    fn test_word_movement() {
        use crate::terminal::tests::{disable_test_mode, enable_test_mode};
        // Enable test mode to prevent terminal output
        enable_test_mode();

        // Create a test buffer with simple content
        let test_content = b"word1 word2\nword3 word4";

        // Create editor state
        let mut winsize = Winsize::new();
        winsize.rows = 10;
        winsize.cols = 40;
        let mut state = EditorState::new(winsize, &[0; MAX_PATH]);
        state.buffer = create_test_file_buffer(test_content);

        // Starting position
        state.file_row = 0;
        state.file_col = 0;

        // Test forward word movement
        let orig_col = state.file_col;
        state.cursor_word_forward();
        assert!(
            state.file_col > orig_col,
            "Word forward should move cursor forward"
        );

        // Go to second line
        state.file_row = 1;
        state.file_col = 0;

        // Test backward word movement
        state.cursor_word_backward();
        assert_eq!(
            state.file_row, 0,
            "Word backward from line start should move to previous line"
        );

        // Clean up
        disable_test_mode();
    }

    #[test]
    fn test_multiple_word_backward_movements() {
        use crate::terminal::tests::{disable_test_mode, enable_test_mode};

        // Enable test mode to prevent terminal output
        enable_test_mode();

        // Create a test buffer with a line containing multiple words
        let test_content = b"first second third fourth fifth";

        // Create editor state
        let mut winsize = Winsize::new();
        winsize.rows = 10;
        winsize.cols = 40;
        let mut state = EditorState::new(winsize, &[0; MAX_PATH]);
        state.buffer = create_test_file_buffer(test_content);

        // Position cursor at the end of the line
        state.file_row = 0;
        state.file_col = test_content.len(); // Position at the end of the line

        // Get positions of each word for verification
        let text = std::str::from_utf8(test_content).unwrap();
        let fifth_pos = text.find("fifth").unwrap();
        let fourth_pos = text.find("fourth").unwrap();
        let third_pos = text.find("third").unwrap();
        let second_pos = text.find("second").unwrap();
        let first_pos = text.find("first").unwrap();

        // First backward movement should go to the beginning of "fifth"
        state.cursor_word_backward();
        assert_eq!(
            state.file_col, fifth_pos,
            "First movement should go to beginning of 'fifth'"
        );

        // Second backward movement should go to the beginning of "fourth"
        state.cursor_word_backward();
        assert_eq!(
            state.file_col, fourth_pos,
            "Second movement should go to beginning of 'fourth'"
        );

        // Third backward movement should go to the beginning of "third"
        state.cursor_word_backward();
        assert_eq!(
            state.file_col, third_pos,
            "Third movement should go to beginning of 'third'"
        );

        // Fourth backward movement should go to the beginning of "second"
        state.cursor_word_backward();
        assert_eq!(
            state.file_col, second_pos,
            "Fourth movement should go to beginning of 'second'"
        );

        // Fifth backward movement should go to the beginning of "first"
        state.cursor_word_backward();
        assert_eq!(
            state.file_col, first_pos,
            "Fifth movement should go to beginning of 'first'"
        );

        // One more backward movement at the beginning of the line should do nothing
        // since we're at the start of the line already
        state.cursor_word_backward();
        assert_eq!(
            state.file_col, 0,
            "No movement should occur at the beginning of the line"
        );

        // Clean up
        disable_test_mode();
    }

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

    #[test]
    fn test_word_movement_with_empty_lines() {
        use crate::terminal::tests::{disable_test_mode, enable_test_mode};
        use std::println; // For debugging in tests

        // Enable test mode to prevent terminal output
        enable_test_mode();

        // Create a test buffer with empty lines between content
        let test_content = b"word1 word2\n\n\nword3 word4\n\nword5";

        // Debug print the content to understand structure
        println!(
            "Test content as ASCII: {:?}",
            std::str::from_utf8(test_content).unwrap()
        );

        // Create editor state first for debugging line counts
        let mut winsize = Winsize::new();
        winsize.rows = 10;
        winsize.cols = 40;
        let mut debug_state = EditorState::new(winsize.clone(), &[0; MAX_PATH]);
        debug_state.buffer = create_test_file_buffer(test_content);

        // Debug line count and content
        let line_count = debug_state.buffer.count_lines();
        println!("Total line count: {line_count}");

        // Print content of each line for debugging
        for i in 0..line_count {
            if let Some(line) = debug_state.buffer.get_line(i) {
                println!("Line {}: '{}'", i, std::str::from_utf8(line).unwrap());
            } else {
                println!("Line {i}: <none>");
            }
        }

        // Create the real editor state for testing
        let mut winsize = Winsize::new();
        winsize.rows = 10;
        winsize.cols = 40;
        let mut state = EditorState::new(winsize, &[0; MAX_PATH]);
        state.buffer = create_test_file_buffer(test_content);

        // Test 1: Forward movement on first line
        state.file_row = 0;
        state.file_col = 0; // At beginning of "word1"

        // First movement should go to end of "word1"
        state.cursor_word_forward();
        println!(
            "After first movement: row={}, col={}",
            state.file_row, state.file_col
        );
        assert_eq!(state.file_col, 5, "Should move to end of 'word1'");

        // Second movement should move to empty line 1
        state.cursor_word_forward();
        println!(
            "After second movement: row={}, col={}",
            state.file_row, state.file_col
        );
        assert_eq!(state.file_row, 1, "Should move to the first empty line");
        assert_eq!(
            state.file_col, 0,
            "Should be at beginning of the empty line"
        );

        // Third movement should move to empty line 2
        state.cursor_word_forward();
        println!(
            "After third movement: row={}, col={}",
            state.file_row, state.file_col
        );
        assert_eq!(state.file_row, 2, "Should move to the second empty line");

        // Fourth movement should move to line with "word3"
        state.cursor_word_forward();
        assert_eq!(state.file_row, 3, "Should move to line with word3");
        assert_eq!(
            state.file_col, 0,
            "Should be at beginning of line with word3"
        );

        // Test 2: Backward movement from an empty line
        state.file_row = 2; // Position on an empty line
        state.file_col = 0;

        // Backward movement should skip to end of "word2" on first line
        state.cursor_word_backward();
        assert_eq!(state.file_row, 0, "Should move up to non-empty line");
        assert_eq!(state.file_col, 6, "Should be at the start of 'word2'");

        // Test 3: Forward movement from an empty line
        state.file_row = 4; // Position on the last empty line
        state.file_col = 0;

        // Forward movement should go to the beginning of "word5" line
        state.cursor_word_forward();
        assert_eq!(state.file_row, 5, "Should move to the last line");
        assert_eq!(
            state.file_col, 0,
            "Should be at the beginning of the word5 line"
        );

        // Clean up
        disable_test_mode();
    }
}
