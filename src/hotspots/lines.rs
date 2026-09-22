//! Code-line counts per file, via tokei.

use std::io;
use std::path::Path;

use tokei::{Config, LanguageType};

/// Count the code lines in `file` with tokei's Rust grammar, leaving out
/// comments and blank lines.
///
/// # Errors
///
/// Returns an error if `file` cannot be read.
pub fn code_lines(file: &Path) -> io::Result<usize> {
    let source = std::fs::read_to_string(file)?;
    let stats = LanguageType::Rust.parse_from_str(source, &Config::default());
    Ok(stats.code)
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    #[test]
    fn counts_code_lines_and_skips_comments_and_blanks() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, "fn main() {{").unwrap();
        writeln!(file, "    // a comment").unwrap();
        writeln!(file, "    let x = 1;").unwrap();
        writeln!(file).unwrap();
        writeln!(file, "    // another comment").unwrap();
        writeln!(file, "}}").unwrap();

        let lines = super::code_lines(file.path()).unwrap();

        assert_eq!(lines, 3);
    }

    #[test]
    fn counts_doc_comments_and_block_comments_as_non_code() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, "/// Adds two numbers.").unwrap();
        writeln!(file, "/*").unwrap();
        writeln!(file, " * block comment").unwrap();
        writeln!(file, " */").unwrap();
        writeln!(file, "fn add(a: i32, b: i32) -> i32 {{").unwrap();
        writeln!(file, "    a + b").unwrap();
        writeln!(file, "}}").unwrap();

        let lines = super::code_lines(file.path()).unwrap();

        assert_eq!(lines, 3);
    }
}
