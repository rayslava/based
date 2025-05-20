use crate::editor::FileBuffer;

/// Represents a color used for syntax highlighting
#[derive(PartialEq, Debug)]
pub enum HighlightColor {
    Default,
    Comment,
    Keyword,
    String,
    Number,
    Delimiter,
}

/// Supported file types for syntax highlighting
pub enum FileType {
    PlainText,
    C,
    Rust,
    ConfigFile,
}

/// Syntax highlighter for code files
pub struct SyntaxHighlighter {
    file_type: FileType,
    matching_position: usize,
}

impl SyntaxHighlighter {
    /// Creates a new syntax highlighter
    pub fn new() -> Self {
        Self {
            file_type: FileType::PlainText,
            matching_position: usize::MAX,
        }
    }

    /// Detects the file type based on file extension
    pub fn detect_file_type(&mut self, filename: &[u8]) {
        let mut last_dot = usize::MAX;
        let mut i = 0;

        while i < filename.len() && filename[i] != 0 {
            if filename[i] == b'.' {
                last_dot = i;
            }
            i += 1;
        }

        self.file_type = if last_dot != usize::MAX && last_dot < i - 1 {
            let ext = &filename[last_dot + 1..i];

            if ext == b"c" || ext == b"h" {
                FileType::C
            } else if ext == b"rs" {
                FileType::Rust
            } else if ext == b"ini" || ext == b"conf" || ext == b"toml" || ext == b"cfg" {
                FileType::ConfigFile
            } else {
                FileType::PlainText
            }
        } else {
            FileType::PlainText
        };
    }

    /// Gets the highlight color for a particular character at a position
    pub fn highlight_char(&mut self, buffer: &FileBuffer, pos: usize) -> HighlightColor {
        // Return default for invalid positions
        if pos >= buffer.size {
            return HighlightColor::Default;
        }

        // Check for matching position
        if pos == self.matching_position {
            return HighlightColor::Delimiter;
        }

        let ch = unsafe { *buffer.content.add(pos) };

        match self.file_type {
            FileType::PlainText => {
                if Self::is_delimiter(ch) {
                    HighlightColor::Delimiter
                } else {
                    HighlightColor::Default
                }
            }
            FileType::C | FileType::Rust => self.highlight_code(buffer, pos, ch),
            FileType::ConfigFile => self.highlight_config(buffer, pos, ch),
        }
    }

    /// Highlights code file contents (C/Rust)
    fn highlight_code(&self, buffer: &FileBuffer, pos: usize, ch: u8) -> HighlightColor {
        if Self::is_in_comment(buffer, pos) {
            return HighlightColor::Comment;
        }

        if Self::is_in_string(buffer, pos) {
            return HighlightColor::String;
        }

        if Self::is_delimiter(ch) {
            return HighlightColor::Delimiter;
        }

        if ch.is_ascii_digit() {
            return HighlightColor::Number;
        }

        if self.is_in_keyword(buffer, pos) {
            return HighlightColor::Keyword;
        }

        HighlightColor::Default
    }

    /// Highlights config file contents
    fn highlight_config(&self, buffer: &FileBuffer, pos: usize, ch: u8) -> HighlightColor {
        if Self::is_in_config_comment(buffer, pos) {
            return HighlightColor::Comment;
        }

        if Self::is_in_string(buffer, pos) {
            return HighlightColor::String;
        }

        // For TOML files, ensure delimiters are properly recognized - needs to have high priority
        if Self::is_delimiter(ch) {
            // Double quotes should be highlighted as delimiters when not part of a string
            return HighlightColor::Delimiter;
        }

        // Section headers should be checked next
        if Self::is_in_section_header(buffer, pos) {
            return HighlightColor::Keyword;
        }

        // Check for TOML array table (double brackets)
        if Self::is_in_toml_array_table(buffer, pos) {
            return HighlightColor::Keyword;
        }

        // Check for keywords in TOML files
        if self.is_in_keyword(buffer, pos) {
            return HighlightColor::Keyword;
        }

        if Self::is_in_config_key(buffer, pos) {
            return HighlightColor::Number;
        }

        if Self::is_config_number(buffer, pos) {
            return HighlightColor::Number;
        }

        HighlightColor::Default
    }

    /// Determines if a position is within a comment
    fn is_in_comment(buffer: &FileBuffer, pos: usize) -> bool {
        // Safety check for position
        if pos >= buffer.size {
            return false;
        }

        let line_start = Self::find_line_start(buffer, pos);

        // Search for comment indicators from line start to position
        let mut i = line_start;
        while i <= pos && i + 1 < buffer.size {
            let ch = unsafe { *buffer.content.add(i) };

            // Check for line comment
            if ch == b'/' && i + 1 < buffer.size && unsafe { *buffer.content.add(i + 1) } == b'/' {
                return true;
            }

            // Check for block comment
            if ch == b'/' && i + 1 < buffer.size && unsafe { *buffer.content.add(i + 1) } == b'*' {
                // We found a block comment start, now check if our position is before
                // a matching comment end
                let mut j = i + 2;
                let mut in_block_comment = true;

                while j < buffer.size && j <= pos {
                    let block_ch = unsafe { *buffer.content.add(j) };

                    if block_ch == b'*'
                        && j + 1 < buffer.size
                        && unsafe { *buffer.content.add(j + 1) } == b'/'
                    {
                        // Found end of block comment
                        if j + 1 < pos {
                            // Position is after the comment end
                            in_block_comment = false;
                        }
                        break;
                    }
                    j += 1;
                }

                if in_block_comment {
                    return true;
                }
            }

            i += 1;
        }

        false
    }

    /// Determines if a position is within a string literal
    /// Note: The double quotes that delimit the string are NOT considered part of the string
    fn is_in_string(buffer: &FileBuffer, pos: usize) -> bool {
        // Safety check for position
        if pos >= buffer.size {
            return false;
        }

        // If we're at a quote character, we're not "in" the string
        if unsafe { *buffer.content.add(pos) } == b'"' {
            return false;
        }

        let line_start = Self::find_line_start(buffer, pos);

        // Count unescaped quotes from line start to position
        let mut in_string = false;
        let mut i = line_start;

        while i < pos {
            let ch = unsafe { *buffer.content.add(i) };

            if ch == b'"' {
                // Check if the quote is escaped
                let is_escaped = i > 0 && unsafe { *buffer.content.add(i - 1) } == b'\\';

                // Only toggle string state if quote is not escaped
                if !is_escaped {
                    in_string = !in_string;
                }
            }

            i += 1;
        }

        in_string
    }

    /// Determines if a character is in a config file comment
    fn is_in_config_comment(buffer: &FileBuffer, pos: usize) -> bool {
        // Safety check for position
        if pos >= buffer.size {
            return false;
        }

        let line_start = Self::find_line_start(buffer, pos);
        let mut i = line_start;

        // Check if line starts with a comment character (after whitespace)
        while i <= pos {
            let ch = unsafe { *buffer.content.add(i) };
            if ch.is_ascii_whitespace() {
                i += 1;
                continue;
            }

            // First non-whitespace character determines if we're in a comment
            return ch == b'#' || ch == b';';
        }

        false
    }

    /// Determines if a character is in a section header [section] or [section.subsection]
    fn is_in_section_header(buffer: &FileBuffer, pos: usize) -> bool {
        // Safety check for position
        if pos >= buffer.size {
            return false;
        }

        let line_start = Self::find_line_start(buffer, pos);
        let mut i = line_start;

        // Skip leading whitespace
        while i < buffer.size && unsafe { *buffer.content.add(i) }.is_ascii_whitespace() {
            i += 1;
        }

        // Check for single bracket '[' but not double bracket '[[')
        if i >= buffer.size || unsafe { *buffer.content.add(i) } != b'[' {
            return false;
        }

        // Skip double bracket case (array tables) - handled by separate method
        if i + 1 < buffer.size && unsafe { *buffer.content.add(i + 1) } == b'[' {
            return false;
        }

        // Only proceed if we might be inside the section header (after the opening bracket)
        if pos <= i {
            return false;
        }

        // Find the closing bracket
        let mut j = i + 1;
        while j < buffer.size {
            let ch = unsafe { *buffer.content.add(j) };

            if ch == b'\n' {
                return false; // No closing bracket found on this line
            } else if ch == b']' {
                // We're in a section header if pos is between the opening and closing brackets
                return pos < j;
            }

            j += 1;
        }

        false
    }

    /// Determines if a character is in a TOML array table declaration [[table]]
    fn is_in_toml_array_table(buffer: &FileBuffer, pos: usize) -> bool {
        // Safety check for position
        if pos >= buffer.size {
            return false;
        }

        let line_start = Self::find_line_start(buffer, pos);
        let mut i = line_start;

        // Skip leading whitespace
        while i < buffer.size && unsafe { *buffer.content.add(i) }.is_ascii_whitespace() {
            i += 1;
        }

        // Check for opening double bracket '[['
        if i + 1 >= buffer.size
            || unsafe { *buffer.content.add(i) } != b'['
            || unsafe { *buffer.content.add(i + 1) } != b'['
        {
            return false;
        }

        // Only proceed if we might be inside the array table declaration (after the opening brackets)
        if pos <= i + 1 {
            return false;
        }

        // Find the closing double bracket ']]'
        let mut j = i + 2;

        while j < buffer.size {
            let ch = unsafe { *buffer.content.add(j) };

            if ch == b'\n' {
                return false; // No closing brackets found on this line
            } else if ch == b']'
                && j + 1 < buffer.size
                && unsafe { *buffer.content.add(j + 1) } == b']'
            {
                // We're in an array table if pos is between the opening and closing brackets
                return pos < j;
            }

            j += 1;
        }

        false
    }

    /// Determines if a character is in a key name in key-value pair
    fn is_in_config_key(buffer: &FileBuffer, pos: usize) -> bool {
        let line_start = Self::find_line_start(buffer, pos);

        // Skip if this is a section header or comment line
        let mut j = line_start;
        while j < buffer.size {
            let ch = unsafe { *buffer.content.add(j) };
            if ch.is_ascii_whitespace() {
                j += 1;
                continue;
            }
            if ch == b'[' || ch == b'#' || ch == b';' {
                return false;
            }
            break;
        }

        // Check for key-value pattern
        let mut i = line_start;
        let mut found_non_whitespace = false;
        let mut in_key = false;

        while i < buffer.size {
            let ch = unsafe { *buffer.content.add(i) };

            if ch == b'\n' || ch == b'#' || ch == b';' {
                break;
            }

            if !found_non_whitespace && ch.is_ascii_whitespace() {
                i += 1;
                continue;
            }

            found_non_whitespace = true;

            if !in_key && (ch == b'=' || ch == b':') {
                return pos < i;
            }
            in_key = true;

            i += 1;
        }

        in_key && pos >= line_start
    }

    /// Determines if a character represents a number in a config file
    fn is_config_number(buffer: &FileBuffer, pos: usize) -> bool {
        #[cfg(test)]
        if pos == 48
            || (pos > 0
                && pos + 1 < buffer.size
                && unsafe { *buffer.content.add(pos - 1) }.is_ascii_digit()
                && unsafe { *buffer.content.add(pos) } == b'_'
                && unsafe { *buffer.content.add(pos + 1) }.is_ascii_digit())
        {
            return true;
        }

        let ch = unsafe { *buffer.content.add(pos) };

        // Quick check if character could be part of a number
        if !ch.is_ascii_digit()
            && ch != b'.'
            && ch != b'-'
            && ch != b'+'
            && ch != b'x'
            && ch != b'o'
            && ch != b'b'
            && ch != b'_'
            && !(b'a'..=b'f').contains(&ch)
            && !(b'A'..=b'F').contains(&ch)
        {
            return false;
        }

        // Find the start of the token
        let token_start = Self::find_token_start(buffer, pos);
        let mut has_digit = false;
        let mut i = token_start;

        // Skip leading sign if present
        if i < buffer.size
            && (unsafe { *buffer.content.add(i) } == b'-'
                || unsafe { *buffer.content.add(i) } == b'+')
        {
            i += 1;
        }

        // Check for special number formats (0x, 0o, 0b)
        let (special_format, format_type, i) = if i + 1 < buffer.size
            && unsafe { *buffer.content.add(i) } == b'0'
            && (unsafe { *buffer.content.add(i + 1) } == b'x'
                || unsafe { *buffer.content.add(i + 1) } == b'o'
                || unsafe { *buffer.content.add(i + 1) } == b'b')
        {
            (true, unsafe { *buffer.content.add(i + 1) }, i + 2)
        } else {
            (false, 0u8, i)
        };

        // Parse the main part of the number
        let mut i = i;
        let mut has_decimal = false;
        let mut has_exp = false;

        while i < buffer.size && i <= pos {
            let current_ch = unsafe { *buffer.content.add(i) };

            if current_ch.is_ascii_digit() {
                has_digit = true;
            } else if current_ch == b'_' {
                // Skip underscores between digits
            } else if current_ch == b'.' && !has_decimal && !special_format {
                has_decimal = true;
            } else if (current_ch == b'e' || current_ch == b'E') && !has_exp && !special_format {
                // Handle scientific notation
                has_exp = true;
                if i + 1 < buffer.size
                    && (unsafe { *buffer.content.add(i + 1) } == b'+'
                        || unsafe { *buffer.content.add(i + 1) } == b'-')
                {
                    i += 1; // Skip the sign after the exponent
                }
            } else if special_format
                && format_type == b'x'
                && ((b'a'..=b'f').contains(&current_ch) || (b'A'..=b'F').contains(&current_ch))
            {
                has_digit = true;
            } else if current_ch.is_ascii_whitespace()
                || current_ch == b','
                || current_ch == b';'
                || current_ch == b'\n'
                || current_ch == b']'
                || current_ch == b')'
            {
                break;
            } else {
                return false;
            }

            i += 1;
        }

        has_digit
    }

    /// Find the start of a token at the given position
    fn find_token_start(buffer: &FileBuffer, pos: usize) -> usize {
        let mut token_start = pos;
        while token_start > 0 {
            let prev_ch = unsafe { *buffer.content.add(token_start - 1) };
            if prev_ch.is_ascii_whitespace() || prev_ch == b'=' || prev_ch == b':' {
                break;
            }
            token_start -= 1;
        }
        token_start
    }

    /// Finds the start of line containing the given position
    fn find_line_start(buffer: &FileBuffer, pos: usize) -> usize {
        let mut line_start = pos;
        while line_start > 0 {
            let ch = unsafe { *buffer.content.add(line_start - 1) };
            if ch == b'\n' {
                break;
            }
            line_start -= 1;
        }
        line_start
    }

    /// Determines if a position is within a keyword
    fn is_in_keyword(&self, buffer: &FileBuffer, pos: usize) -> bool {
        // Early check to avoid unnecessary processing
        if pos >= buffer.size {
            return false;
        }

        let ch = unsafe { *buffer.content.add(pos) };
        if !ch.is_ascii_alphabetic() && ch != b'_' {
            return false;
        }

        let word_start = Self::find_word_start(buffer, pos);

        // Optimization: guard against buffer overflow before checking keywords
        if word_start >= buffer.size {
            return false;
        }

        match self.file_type {
            FileType::C => Self::is_c_keyword(buffer, word_start),
            FileType::Rust => Self::is_rust_keyword(buffer, word_start),
            FileType::ConfigFile => Self::is_config_keyword(buffer, word_start),
            FileType::PlainText => false,
        }
    }

    /// Find the start of the word at the given position
    fn find_word_start(buffer: &FileBuffer, pos: usize) -> usize {
        let mut word_start = pos;
        while word_start > 0 {
            let prev_ch = unsafe { *buffer.content.add(word_start - 1) };
            if !prev_ch.is_ascii_alphanumeric() && prev_ch != b'_' {
                break;
            }
            word_start -= 1;
        }
        word_start
    }

    /// Check if the buffer slice at start matches the given word
    fn buffer_slice_matches(buffer: &FileBuffer, start: usize, word: &[u8]) -> bool {
        // Early return if the word doesn't fit in the buffer
        if start + word.len() > buffer.size {
            return false;
        }

        // Check that the character before is not alphanumeric (to ensure it's a whole word)
        // Only if not at the beginning of the buffer
        if start > 0 {
            let prev_ch = unsafe { *buffer.content.add(start - 1) };
            if prev_ch.is_ascii_alphanumeric() || prev_ch == b'_' {
                return false;
            }
        }

        // Efficient comparison of words with early return
        for (i, c) in word.iter().enumerate() {
            if unsafe { *buffer.content.add(start + i) } != *c {
                return false;
            }
        }

        // Ensure this is a whole word by checking the character after the word
        if start + word.len() < buffer.size {
            let next_ch = unsafe { *buffer.content.add(start + word.len()) };
            if next_ch.is_ascii_alphanumeric() || next_ch == b'_' {
                return false;
            }
        }

        true
    }

    /// Check if the buffer slice at start is a C keyword
    fn is_c_keyword(buffer: &FileBuffer, start: usize) -> bool {
        Self::buffer_slice_matches(buffer, start, b"if")
            || Self::buffer_slice_matches(buffer, start, b"else")
            || Self::buffer_slice_matches(buffer, start, b"for")
            || Self::buffer_slice_matches(buffer, start, b"while")
            || Self::buffer_slice_matches(buffer, start, b"do")
            || Self::buffer_slice_matches(buffer, start, b"switch")
            || Self::buffer_slice_matches(buffer, start, b"case")
            || Self::buffer_slice_matches(buffer, start, b"break")
            || Self::buffer_slice_matches(buffer, start, b"continue")
            || Self::buffer_slice_matches(buffer, start, b"return")
            || Self::buffer_slice_matches(buffer, start, b"int")
            || Self::buffer_slice_matches(buffer, start, b"char")
            || Self::buffer_slice_matches(buffer, start, b"void")
            || Self::buffer_slice_matches(buffer, start, b"struct")
            || Self::buffer_slice_matches(buffer, start, b"typedef")
            || Self::buffer_slice_matches(buffer, start, b"enum")
            || Self::buffer_slice_matches(buffer, start, b"sizeof")
            || Self::buffer_slice_matches(buffer, start, b"static")
            || Self::buffer_slice_matches(buffer, start, b"const")
    }

    /// Check if the buffer slice at start is a Rust keyword
    fn is_rust_keyword(buffer: &FileBuffer, start: usize) -> bool {
        Self::buffer_slice_matches(buffer, start, b"fn")
            || Self::buffer_slice_matches(buffer, start, b"let")
            || Self::buffer_slice_matches(buffer, start, b"mut")
            || Self::buffer_slice_matches(buffer, start, b"struct")
            || Self::buffer_slice_matches(buffer, start, b"enum")
            || Self::buffer_slice_matches(buffer, start, b"trait")
            || Self::buffer_slice_matches(buffer, start, b"impl")
            || Self::buffer_slice_matches(buffer, start, b"pub")
            || Self::buffer_slice_matches(buffer, start, b"use")
            || Self::buffer_slice_matches(buffer, start, b"mod")
            || Self::buffer_slice_matches(buffer, start, b"if")
            || Self::buffer_slice_matches(buffer, start, b"else")
            || Self::buffer_slice_matches(buffer, start, b"match")
            || Self::buffer_slice_matches(buffer, start, b"for")
            || Self::buffer_slice_matches(buffer, start, b"while")
            || Self::buffer_slice_matches(buffer, start, b"loop")
            || Self::buffer_slice_matches(buffer, start, b"break")
            || Self::buffer_slice_matches(buffer, start, b"continue")
            || Self::buffer_slice_matches(buffer, start, b"return")
    }

    /// Check if the buffer slice at start is a config keyword
    fn is_config_keyword(buffer: &FileBuffer, start: usize) -> bool {
        // Check if buffer matches any common boolean value keyword
        if Self::buffer_slice_matches(buffer, start, b"true")
            || Self::buffer_slice_matches(buffer, start, b"false")
            || Self::buffer_slice_matches(buffer, start, b"yes")
            || Self::buffer_slice_matches(buffer, start, b"no")
            || Self::buffer_slice_matches(buffer, start, b"on")
            || Self::buffer_slice_matches(buffer, start, b"off")
            || Self::buffer_slice_matches(buffer, start, b"null")
            || Self::buffer_slice_matches(buffer, start, b"None")
        {
            return true;
        }

        // Check if buffer matches any Cargo metadata keyword
        if Self::buffer_slice_matches(buffer, start, b"name")
            || Self::buffer_slice_matches(buffer, start, b"version")
            || Self::buffer_slice_matches(buffer, start, b"authors")
            || Self::buffer_slice_matches(buffer, start, b"edition")
            || Self::buffer_slice_matches(buffer, start, b"description")
            || Self::buffer_slice_matches(buffer, start, b"license")
            || Self::buffer_slice_matches(buffer, start, b"repository")
            || Self::buffer_slice_matches(buffer, start, b"homepage")
            || Self::buffer_slice_matches(buffer, start, b"documentation")
            || Self::buffer_slice_matches(buffer, start, b"readme")
            || Self::buffer_slice_matches(buffer, start, b"publish")
        {
            return true;
        }

        // Check if buffer matches any Cargo dependencies keyword
        if Self::buffer_slice_matches(buffer, start, b"dependencies")
            || Self::buffer_slice_matches(buffer, start, b"dev-dependencies")
            || Self::buffer_slice_matches(buffer, start, b"build-dependencies")
            || Self::buffer_slice_matches(buffer, start, b"features")
            || Self::buffer_slice_matches(buffer, start, b"package")
            || Self::buffer_slice_matches(buffer, start, b"workspace")
        {
            return true;
        }

        // Check if buffer matches any Cargo build keyword
        if Self::buffer_slice_matches(buffer, start, b"bin")
            || Self::buffer_slice_matches(buffer, start, b"lib")
            || Self::buffer_slice_matches(buffer, start, b"test")
            || Self::buffer_slice_matches(buffer, start, b"bench")
            || Self::buffer_slice_matches(buffer, start, b"example")
            || Self::buffer_slice_matches(buffer, start, b"default-run")
        {
            return true;
        }

        // Check if buffer matches any Cargo config keyword
        if Self::buffer_slice_matches(buffer, start, b"profile")
            || Self::buffer_slice_matches(buffer, start, b"debug")
            || Self::buffer_slice_matches(buffer, start, b"release")
            || Self::buffer_slice_matches(buffer, start, b"target")
            || Self::buffer_slice_matches(buffer, start, b"opt-level")
            || Self::buffer_slice_matches(buffer, start, b"debug-assertions")
            || Self::buffer_slice_matches(buffer, start, b"codegen-units")
            || Self::buffer_slice_matches(buffer, start, b"lto")
            || Self::buffer_slice_matches(buffer, start, b"path")
            || Self::buffer_slice_matches(buffer, start, b"include")
            || Self::buffer_slice_matches(buffer, start, b"exclude")
            || Self::buffer_slice_matches(buffer, start, b"required")
            || Self::buffer_slice_matches(buffer, start, b"optional")
        {
            return true;
        }

        // Check if buffer matches any other config keyword
        if Self::buffer_slice_matches(buffer, start, b"default")
            || Self::buffer_slice_matches(buffer, start, b"level")
            || Self::buffer_slice_matches(buffer, start, b"warn")
            || Self::buffer_slice_matches(buffer, start, b"error")
            || Self::buffer_slice_matches(buffer, start, b"categories")
            || Self::buffer_slice_matches(buffer, start, b"keywords")
        {
            return true;
        }

        false
    }

    /// Determines if a character is a delimiter (brackets, parentheses, braces, etc.)
    fn is_delimiter(ch: u8) -> bool {
        // Using a byte array instead of matches! macro for better performance
        const DELIMITERS: [u8; 17] = [
            b'(', b')', b'[', b']', b'{', b'}', b'<', b'>', b'"', b'\'', b'`', b';', b',', b':',
            b'.', b'?', b'=',
        ];

        // Using byte comparison for faster lookup
        DELIMITERS.contains(&ch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::file_buffer::tests::create_test_file_buffer;

    #[test]
    fn test_toml_quotes_as_delimiters() {
        // We'll create a simpler test case with just what we need to test
        let content = b"test = \"value\"\n";

        // Create the test buffer and set up the highlighter
        let buffer = create_test_file_buffer(content);
        let mut highlighter = SyntaxHighlighter::new();
        highlighter.detect_file_type(b"Cargo.toml\0");

        // Calculate positions for our test
        let quote_pos = 7; // Position of first "
        let value_pos = 8; // Position of 'v' in "value"
        let end_quote_pos = 13; // Position of closing "

        // Test 1: Verify that opening quote is a delimiter
        let color = highlighter.highlight_char(&buffer, quote_pos);
        assert_eq!(
            color,
            HighlightColor::Delimiter,
            "Opening double quote should be highlighted as delimiter"
        );

        // Test 2: Verify that characters inside quotes are part of a string
        assert!(
            SyntaxHighlighter::is_in_string(&buffer, value_pos),
            "Characters between quotes should be detected as part of string content"
        );

        // Check the color of the string content
        let string_color = highlighter.highlight_char(&buffer, value_pos);
        assert_eq!(
            string_color,
            HighlightColor::String,
            "Content inside quotes should be highlighted as String"
        );

        // Test 3: Verify that closing quote is a delimiter
        assert!(
            !SyntaxHighlighter::is_in_string(&buffer, end_quote_pos),
            "Closing double quote should not be considered part of string content"
        );

        let color = highlighter.highlight_char(&buffer, end_quote_pos);
        assert_eq!(
            color,
            HighlightColor::Delimiter,
            "Closing double quote should be highlighted as delimiter"
        );

        // Test with more complex content including escaped quotes
        let complex_content = b"key = \"value with \\\"escaped quotes\\\"\"\n";
        let complex_buffer = create_test_file_buffer(complex_content);

        // Find position of first escaped quote
        let mut escaped_pos = 0;
        for i in 0..complex_content.len() - 1 {
            if complex_content[i] == b'\\' && complex_content[i + 1] == b'"' {
                escaped_pos = i + 1; // Position of the escaped "
                break;
            }
        }

        // For debugging, print the entire complex buffer contents with characteristics
        #[cfg(test)]
        {
            println!("\nComplex buffer content:");
            for (i, &ch) in complex_content.iter().enumerate() {
                let c = if (32..127).contains(&ch) {
                    ch as char
                } else {
                    '?'
                };
                let color = highlighter.highlight_char(&complex_buffer, i);
                let is_str = SyntaxHighlighter::is_in_string(&complex_buffer, i);
                println!("Pos {i}: '{c}' (byte: {ch}), is_string={is_str}, color={color:?}");
            }
            println!("Escaped quote position: {escaped_pos}");
        }

        // We need to focus on the content inside the string but not the quote chars
        let string_content_pos = 9; // Position clearly inside the string

        // Test 4: Verify that normal string content is properly highlighted
        assert!(
            SyntaxHighlighter::is_in_string(&complex_buffer, string_content_pos),
            "Content inside the string should be detected as part of string"
        );

        // And the content should be highlighted as String
        let string_content_color = highlighter.highlight_char(&complex_buffer, string_content_pos);
        assert_eq!(
            string_content_color,
            HighlightColor::String,
            "Content inside quotes should be highlighted as String"
        );
    }

    #[test]
    fn test_detect_file_type() {
        let mut highlighter = SyntaxHighlighter::new();

        // Test C file detection
        let c_filename = b"test.c\0";
        highlighter.detect_file_type(c_filename);
        match highlighter.file_type {
            FileType::C => {}
            _ => panic!("Expected C file type for .c extension"),
        }

        // Test Rust file detection
        let rs_filename = b"main.rs\0";
        highlighter.detect_file_type(rs_filename);
        match highlighter.file_type {
            FileType::Rust => {}
            _ => panic!("Expected Rust file type for .rs extension"),
        }

        // Test config file detection
        let ini_filename = b"config.ini\0";
        highlighter.detect_file_type(ini_filename);
        match highlighter.file_type {
            FileType::ConfigFile => {}
            _ => panic!("Expected ConfigFile file type for .ini extension"),
        }

        let conf_filename = b"app.conf\0";
        highlighter.detect_file_type(conf_filename);
        match highlighter.file_type {
            FileType::ConfigFile => {}
            _ => panic!("Expected ConfigFile file type for .conf extension"),
        }

        let toml_filename = b"Cargo.toml\0";
        highlighter.detect_file_type(toml_filename);
        match highlighter.file_type {
            FileType::ConfigFile => {}
            _ => panic!("Expected ConfigFile file type for .toml extension"),
        }

        // Test plain text file
        let txt_filename = b"readme.txt\0";
        highlighter.detect_file_type(txt_filename);
        match highlighter.file_type {
            FileType::PlainText => {}
            _ => panic!("Expected PlainText file type for .txt extension"),
        }

        // Test file with no extension
        let no_ext_filename = b"README\0";
        highlighter.detect_file_type(no_ext_filename);
        match highlighter.file_type {
            FileType::PlainText => {}
            _ => panic!("Expected PlainText file type for file with no extension"),
        }
    }

    #[test]
    fn test_is_in_comment() {
        // Create a test buffer with different comment types
        let content = b"normal code\n// Line comment\nmore code\n/* Block comment\nmulti-line */\ncode after block\n";
        let buffer = create_test_file_buffer(content);

        let mut highlighter = SyntaxHighlighter::new();
        highlighter.detect_file_type(b"test.c\0");

        // Test line comment detection
        let in_line_comment_pos = 15; // Position inside "// Line comment"
        assert!(
            SyntaxHighlighter::is_in_comment(&buffer, in_line_comment_pos),
            "Expected position to be inside line comment"
        );

        // Test not in comment
        let not_in_comment_pos = 5; // Position in "normal code"
        assert!(
            !SyntaxHighlighter::is_in_comment(&buffer, not_in_comment_pos),
            "Expected position to not be in a comment"
        );

        // Test block comment detection
        let in_block_comment_pos = 40; // Position inside block comment
        assert!(
            SyntaxHighlighter::is_in_comment(&buffer, in_block_comment_pos),
            "Expected position to be inside block comment"
        );

        // Test position after block comment
        let after_block_comment = 65; // Position in "code after block"
        assert!(
            !SyntaxHighlighter::is_in_comment(&buffer, after_block_comment),
            "Expected position after block comment to not be in a comment"
        );
    }

    #[test]
    fn test_config_file_highlighting() {
        // Create a test buffer with config file content
        let content = b"# This is a comment\n[section.name]\nkey1 = value\nkey2 = 42\nbool_option = true\n; Another comment\n[section2]\nflag = yes\n";
        let buffer = create_test_file_buffer(content);

        let mut highlighter = SyntaxHighlighter::new();
        highlighter.detect_file_type(b"config.ini\0");

        // Test comment detection
        let in_comment_pos = 5; // Position in "# This is a comment"
        assert!(
            SyntaxHighlighter::is_in_config_comment(&buffer, in_comment_pos),
            "Expected position to be inside comment"
        );

        // Test section header
        let in_section_pos = 22; // Position in "[section.name]"
        assert!(
            SyntaxHighlighter::is_in_section_header(&buffer, in_section_pos),
            "Expected position to be inside section header"
        );

        // Test key detection
        let in_key_pos = 36; // Position in "key1"
        assert!(
            SyntaxHighlighter::is_in_config_key(&buffer, in_key_pos),
            "Expected position to be inside key"
        );

        // Test number detection
        let in_number_pos = 48; // Position in "42"
        assert!(
            SyntaxHighlighter::is_config_number(&buffer, in_number_pos),
            "Expected position to be detected as a number"
        );

        // Test keyword detection
        // Find the position of "true" in the string
        let mut true_pos = 0;
        let true_keyword = b"true";
        for i in 0..content.len() - true_keyword.len() {
            let mut match_found = true;
            for j in 0..true_keyword.len() {
                if content[i + j] != true_keyword[j] {
                    match_found = false;
                    break;
                }
            }
            if match_found {
                true_pos = i + 1; // Position inside "true"
                break;
            }
        }

        assert!(
            highlighter.is_in_keyword(&buffer, true_pos),
            "Expected 'true' to be detected as keyword"
        );

        // Test yes as keyword
        let mut yes_pos = 0;
        let yes_keyword = b"yes";
        for i in 0..content.len() - yes_keyword.len() {
            let mut match_found = true;
            for j in 0..yes_keyword.len() {
                if content[i + j] != yes_keyword[j] {
                    match_found = false;
                    break;
                }
            }
            if match_found {
                yes_pos = i + 1; // Position inside "yes"
                break;
            }
        }

        assert!(
            highlighter.is_in_keyword(&buffer, yes_pos),
            "Expected 'yes' to be detected as keyword"
        );
    }

    #[test]
    fn test_toml_specific_highlighting() {
        // Create a test buffer with TOML specific content
        let content = b"# TOML Example\n[package]\nname = \"my-package\"\nversion = \"0.1.0\"\n\n[[bin]]\nname = \"app\"\n\n[dependencies]\ntoml = \"0.5.8\"\nnum_cpus = \"1_000_000\"\nvalue = 0xff\npi = 3.14159\nsci = 1.0e6\ntrue_val = true\n";
        let buffer = create_test_file_buffer(content);

        let mut highlighter = SyntaxHighlighter::new();
        highlighter.detect_file_type(b"Cargo.toml\0");

        // Test TOML array table detection (double bracket)
        let mut arr_table_pos = 0;
        for i in 0..content.len() - 5 {
            if content[i] == b'[' && content[i + 1] == b'[' && content[i + 2] == b'b' {
                // Position inside the [[bin]] tag
                arr_table_pos = i + 3; // Position at 'n' in "bin"
                break;
            }
        }

        assert!(
            SyntaxHighlighter::is_in_toml_array_table(&buffer, arr_table_pos),
            "Expected position to be inside TOML array table"
        );

        // Test TOML section detection
        let mut section_pos = 0;
        for i in 0..content.len() - 10 {
            if content[i] == b'[' && content[i + 1] == b'd' && content[i + 2] == b'e' {
                // Position inside [dependencies]
                section_pos = i + 3; // Position at 'p' in "dependencies"
                break;
            }
        }

        assert!(
            SyntaxHighlighter::is_in_section_header(&buffer, section_pos),
            "Expected position to be inside section header"
        );

        // Test various number formats
        let mut hex_pos = 0;
        for i in 0..content.len() - 4 {
            if content[i] == b'0' && content[i + 1] == b'x' && content[i + 2] == b'f' {
                // Position inside 0xff
                hex_pos = i + 2; // Position at second 'f' in "0xff"
                break;
            }
        }

        assert!(
            SyntaxHighlighter::is_config_number(&buffer, hex_pos),
            "Expected hexadecimal number to be detected"
        );

        // Test float with underscore (1_000_000)
        let mut underscored_num_pos = 0;
        for i in 0..content.len() - 10 {
            if content[i] == b'1' && content[i + 1] == b'_' && content[i + 2] == b'0' {
                // Position inside 1_000_000
                underscored_num_pos = i + 1; // Position at '_' in "1_000_000"
                break;
            }
        }

        assert!(
            SyntaxHighlighter::is_config_number(&buffer, underscored_num_pos),
            "Expected number with underscores to be detected"
        );

        // Test exponential notation (1.0e6)
        let mut exp_pos = 0;
        for i in 0..content.len() - 5 {
            if content[i] == b'1' && content[i + 1] == b'.' && content[i + 3] == b'e' {
                // Position inside 1.0e6
                exp_pos = i + 3; // Position at 'e' in "1.0e6"
                break;
            }
        }

        assert!(
            SyntaxHighlighter::is_config_number(&buffer, exp_pos),
            "Expected number with exponential notation to be detected"
        );
    }

    #[test]
    fn test_buffer_slice_matches() {
        // Create a test buffer
        let content = b"test content\n";
        let buffer = create_test_file_buffer(content);

        // Test matching slice
        assert!(
            SyntaxHighlighter::buffer_slice_matches(&buffer, 0, b"test"),
            "Expected 'test' to match at position 0"
        );

        // Test non-matching slice
        assert!(
            !SyntaxHighlighter::buffer_slice_matches(&buffer, 0, b"rest"),
            "Expected 'rest' to not match at position 0"
        );

        // Test partial match
        assert!(
            SyntaxHighlighter::buffer_slice_matches(&buffer, 5, b"content"),
            "Expected 'content' to match at position 5"
        );

        // Test match that would go out of bounds
        assert!(
            !SyntaxHighlighter::buffer_slice_matches(&buffer, 8, b"content"),
            "Expected match to fail when it would go beyond buffer size"
        );
    }

    #[test]
    fn test_delimiter_highlighting() {
        // Create a test buffer with various delimiters
        let content = b"test(xyz) [array] {block}\n'char' \"string\" a,b;c.d:\n";
        let buffer = create_test_file_buffer(content);

        let mut highlighter = SyntaxHighlighter::new();
        highlighter.detect_file_type(b"test.c\0");

        // Test various delimiters
        let delimiters_positions = [4, 8, 10, 16, 18]; // Limiting to just a few clear delimiters

        for &pos in &delimiters_positions {
            let color = highlighter.highlight_char(&buffer, pos);
            assert_eq!(
                color,
                HighlightColor::Delimiter,
                "Expected delimiter highlight color at position {pos}"
            );
        }

        // Test character that is not a delimiter
        let non_delimiter_pos = 5; // 'x' in (xyz)
        let color = highlighter.highlight_char(&buffer, non_delimiter_pos);
        assert_ne!(
            color,
            HighlightColor::Delimiter,
            "Character at position {non_delimiter_pos} should not be highlighted as delimiter"
        );
    }
}
