pub(crate) mod input;
pub mod squeezer;

pub use input::*;

use std::io::{self, Read, Write};

use ansi_term::Color;
use ansi_term::Color::Fixed;

use crate::squeezer::{SqueezeAction, Squeezer};

const BUFFER_SIZE: usize = 256;

const COLOR_NULL: Color = Fixed(242); // grey
const COLOR_OFFSET: Color = Fixed(242); // grey
const COLOR_ASCII_PRINTABLE: Color = Color::Cyan;
const COLOR_ASCII_WHITESPACE: Color = Color::Green;
const COLOR_ASCII_OTHER: Color = Color::Purple;
const COLOR_NONASCII: Color = Color::Yellow;

pub enum ByteCategory {
    Null,
    AsciiPrintable,
    AsciiWhitespace,
    AsciiOther,
    NonAscii,
}

#[derive(Copy, Clone)]
struct Byte(u8);

impl Byte {
    fn category(self) -> ByteCategory {
        if self.0 == 0x00 {
            ByteCategory::Null
        } else if self.0.is_ascii_alphanumeric()
            || self.0.is_ascii_punctuation()
            || self.0.is_ascii_graphic()
        {
            ByteCategory::AsciiPrintable
        } else if self.0.is_ascii_whitespace() {
            ByteCategory::AsciiWhitespace
        } else if self.0.is_ascii() {
            ByteCategory::AsciiOther
        } else {
            ByteCategory::NonAscii
        }
    }

    fn color(self) -> &'static Color {
        use crate::ByteCategory::*;

        match self.category() {
            Null => &COLOR_NULL,
            AsciiPrintable => &COLOR_ASCII_PRINTABLE,
            AsciiWhitespace => &COLOR_ASCII_WHITESPACE,
            AsciiOther => &COLOR_ASCII_OTHER,
            NonAscii => &COLOR_NONASCII,
        }
    }

    fn as_char(self) -> char {
        use crate::ByteCategory::*;

        match self.category() {
            Null => '0',
            AsciiPrintable => self.0 as char,
            AsciiWhitespace if self.0 == 0x20 => ' ',
            AsciiWhitespace => '_',
            AsciiOther => '•',
            NonAscii => '×',
        }
    }
}

struct BorderElements {
    left_corner: char,
    horizontal_line: char,
    column_separator: char,
    right_corner: char,
}

pub enum BorderStyle {
    Unicode,
    Ascii,
    None,
}

impl BorderStyle {
    fn header_elems(&self) -> Option<BorderElements> {
        match self {
            BorderStyle::Unicode => Some(BorderElements {
                left_corner: '┌',
                horizontal_line: '─',
                column_separator: '┬',
                right_corner: '┐',
            }),
            BorderStyle::Ascii => Some(BorderElements {
                left_corner: '+',
                horizontal_line: '-',
                column_separator: '+',
                right_corner: '+',
            }),
            BorderStyle::None => None,
        }
    }

    fn footer_elems(&self) -> Option<BorderElements> {
        match self {
            BorderStyle::Unicode => Some(BorderElements {
                left_corner: '└',
                horizontal_line: '─',
                column_separator: '┴',
                right_corner: '┘',
            }),
            BorderStyle::Ascii => Some(BorderElements {
                left_corner: '+',
                horizontal_line: '-',
                column_separator: '+',
                right_corner: '+',
            }),
            BorderStyle::None => None,
        }
    }

    fn outer_sep(&self) -> char {
        match self {
            BorderStyle::Unicode => '│',
            BorderStyle::Ascii => '|',
            BorderStyle::None => ' ',
        }
    }

    fn inner_sep(&self) -> char {
        match self {
            BorderStyle::Unicode => '┊',
            BorderStyle::Ascii => '|',
            BorderStyle::None => ' ',
        }
    }
}

pub struct Printer<'a, Writer: Write> {
    idx: usize,
    /// The raw bytes used as input for the current line.
    raw_line: Vec<u8>,
    /// The buffered line built with each byte, ready to print to writer.
    buffer_line: Vec<u8>,
    writer: &'a mut Writer,
    show_color: bool,
    border_style: BorderStyle,
    header_was_printed: bool,
    byte_hex_table: Vec<String>,
    byte_char_table: Vec<String>,
    squeezer: Squeezer,
    display_offset: usize,
}

impl<'a, Writer: Write> Printer<'a, Writer> {
    pub fn new(
        writer: &'a mut Writer,
        show_color: bool,
        border_style: BorderStyle,
        use_squeeze: bool,
    ) -> Printer<'a, Writer> {
        Printer {
            idx: 1,
            raw_line: vec![],
            buffer_line: vec![],
            writer,
            show_color,
            border_style,
            header_was_printed: false,
            byte_hex_table: (0u8..=u8::max_value())
                .map(|i| {
                    let byte_hex = format!("{:02x} ", i);
                    if show_color {
                        Byte(i).color().paint(byte_hex).to_string()
                    } else {
                        byte_hex
                    }
                })
                .collect(),
            byte_char_table: (0u8..=u8::max_value())
                .map(|i| {
                    let byte_char = format!("{}", Byte(i).as_char());
                    if show_color {
                        Byte(i).color().paint(byte_char).to_string()
                    } else {
                        byte_char
                    }
                })
                .collect(),
            squeezer: Squeezer::new(use_squeeze),
            display_offset: 0,
        }
    }

    pub fn display_offset(&mut self, display_offset: usize) -> &mut Self {
        self.display_offset = display_offset;
        self
    }

    pub fn header(&mut self) {
        if let Some(border_elements) = self.border_style.header_elems() {
            let h = border_elements.horizontal_line;
            let h8 = h.to_string().repeat(8);
            let h25 = h.to_string().repeat(25);

            writeln!(
                self.writer,
                "{l}{h8}{c}{h25}{c}{h25}{c}{h8}{c}{h8}{r}",
                l = border_elements.left_corner,
                c = border_elements.column_separator,
                r = border_elements.right_corner,
                h8 = h8,
                h25 = h25
            )
            .ok();
        }
    }

    pub fn footer(&mut self) {
        if let Some(border_elements) = self.border_style.footer_elems() {
            let h = border_elements.horizontal_line;
            let h8 = h.to_string().repeat(8);
            let h25 = h.to_string().repeat(25);

            writeln!(
                self.writer,
                "{l}{h8}{c}{h25}{c}{h25}{c}{h8}{c}{h8}{r}",
                l = border_elements.left_corner,
                c = border_elements.column_separator,
                r = border_elements.right_corner,
                h8 = h8,
                h25 = h25
            )
            .ok();
        }
    }

    fn print_position_indicator(&mut self) {
        if !self.header_was_printed {
            self.header();
            self.header_was_printed = true;
        }

        let style = COLOR_OFFSET.normal();
        let byte_index = format!("{:08x}", (self.idx - 1) + self.display_offset);
        let formatted_string = if self.show_color {
            format!("{}", style.paint(byte_index))
        } else {
            byte_index
        };
        let _ = write!(
            &mut self.buffer_line,
            "{}{}{} ",
            self.border_style.outer_sep(),
            formatted_string,
            self.border_style.outer_sep()
        );
    }

    pub fn print_byte(&mut self, b: u8) -> io::Result<()> {
        if self.idx % 16 == 1 {
            self.print_position_indicator();
        }

        write!(&mut self.buffer_line, "{}", self.byte_hex_table[b as usize])?;
        self.raw_line.push(b);

        self.squeezer.process(b, self.idx);

        match self.idx % 16 {
            8 => {
                let _ = write!(&mut self.buffer_line, "{} ", self.border_style.inner_sep());
            }
            0 => {
                self.print_textline()?;
            }
            _ => {}
        }

        self.idx += 1;

        Ok(())
    }

    pub fn print_textline(&mut self) -> io::Result<()> {
        let len = self.raw_line.len();

        if len == 0 {
            if self.squeezer.active() {
                self.print_position_indicator();
                let _ = writeln!(
                    &mut self.buffer_line,
                    "{0:1$}{4}{0:2$}{5}{0:3$}{4}{0:3$}{5}",
                    "",
                    24,
                    25,
                    8,
                    self.border_style.inner_sep(),
                    self.border_style.outer_sep(),
                );
                self.writer.write_all(&self.buffer_line)?;
            }
            return Ok(());
        }

        let squeeze_action = self.squeezer.action();

        if squeeze_action != SqueezeAction::Delete {
            if len < 8 {
                let _ = write!(
                    &mut self.buffer_line,
                    "{0:1$}{3}{0:2$}{4}",
                    "",
                    3 * (8 - len),
                    1 + 3 * 8,
                    self.border_style.inner_sep(),
                    self.border_style.outer_sep(),
                );
            } else {
                let _ = write!(
                    &mut self.buffer_line,
                    "{0:1$}{2}",
                    "",
                    3 * (16 - len),
                    self.border_style.outer_sep()
                );
            }

            let mut idx = 1;
            for &b in self.raw_line.iter() {
                let _ = write!(
                    &mut self.buffer_line,
                    "{}",
                    self.byte_char_table[b as usize]
                );

                if idx == 8 {
                    let _ = write!(&mut self.buffer_line, "{}", self.border_style.inner_sep());
                }

                idx += 1;
            }

            if len < 8 {
                let _ = writeln!(
                    &mut self.buffer_line,
                    "{0:1$}{3}{0:2$}{4}",
                    "",
                    8 - len,
                    8,
                    self.border_style.inner_sep(),
                    self.border_style.outer_sep(),
                );
            } else {
                let _ = writeln!(
                    &mut self.buffer_line,
                    "{0:1$}{2}",
                    "",
                    16 - len,
                    self.border_style.outer_sep()
                );
            }
        }

        match squeeze_action {
            SqueezeAction::Print => {
                self.buffer_line.clear();
                let style = COLOR_OFFSET.normal();
                let asterisk = if self.show_color {
                    format!("{}", style.paint("*"))
                } else {
                    String::from("*")
                };
                let _ = writeln!(
                    &mut self.buffer_line,
                    "{5}{0}{1:2$}{5}{1:3$}{6}{1:3$}{5}{1:4$}{6}{1:4$}{5}",
                    asterisk,
                    "",
                    7,
                    25,
                    8,
                    self.border_style.outer_sep(),
                    self.border_style.inner_sep(),
                );
            }
            SqueezeAction::Delete => self.buffer_line.clear(),
            SqueezeAction::Ignore => (),
        }

        self.writer.write_all(&self.buffer_line)?;

        self.raw_line.clear();
        self.buffer_line.clear();

        self.squeezer.advance();

        Ok(())
    }

    pub fn header_was_printed(&self) -> bool {
        self.header_was_printed
    }

    /// Loop through the given `Reader`, printing until the `Reader` buffer
    /// is exhausted.
    pub fn print_all<Reader: Read>(
        &mut self,
        mut reader: Reader,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut buffer = [0; BUFFER_SIZE];
        'mainloop: loop {
            let size = reader.read(&mut buffer)?;
            if size == 0 {
                break;
            }

            for b in &buffer[..size] {
                let res = self.print_byte(*b);

                if res.is_err() {
                    // Broken pipe
                    break 'mainloop;
                }
            }
        }

        // Finish last line
        self.print_textline().ok();
        if !self.header_was_printed() {
            self.header();
        }
        self.footer();

        Ok(())
    }
}

/// Parse a byte count integer.
/// Accepts two forms, either a hex number prefixed by "0x", or a decimal number optionally
/// followed by a suffix. Suffixes can be 'b' or 'B' for byte (1); kb, KB, MB, GB, TB for
/// SI powers of 10 (1000, 1000000, etc); or k/K/KiB, M/MiB, G/GiB, T/TiB for binary
/// power of 2 (1024, 1024*1024, etc).
pub fn parse_byte_count(input: &str) -> Result<u64, String> {
    // handle hex strings first, they have a prefix of '0x' and no suffixes are allowed
    if input.starts_with("0x") {
        return u64::from_str_radix(input.trim_start_matches("0x"), 16)
            .map_err(|e| format!("failed to parse hex byte count '{}': {}", input, e));
    }

    // Manually match and split approximately /([0-9]+)([A-Za-z]*)/ without using regex.
    // The suffix is found by splitting the string at the first ASCII alpha char,
    // everything before that is the "number".  This doesn't strictly match that regex,
    // but the u64 conversion validates the number and the suffix is matched against
    // a fixed list, so it works here.
    let (num_str, suffix) = input
        .find(|c: char| c.is_ascii_alphabetic())
        .map(|suffix_start| input.split_at(suffix_start)) // match found, split the string
        .unwrap_or((input, "")); // or no match, suffix is empty and num is the whole input

    // parse the number into a u64, this will check that it's actually a vaild number
    let num = u64::from_str_radix(num_str, 10)
        .map_err(|e| format!("failed to parse byte count '{}': {}", num_str, e))?;

    let multiplier: u64 = match suffix {
        // a single byte
        "" | "b" | "B" => 1,

        // powers of 10 SI suffixes
        "kB" | "KB" => 1000,
        "MB" => 1_000_000,
        "GB" => 1_000_000_000,
        "TB" => 1_000_000_000_000,

        // powers of 2 binary suffixes
        "k" | "K" | "KiB" => 1 << 10,
        "M" | "MiB" => 1 << 20,
        "G" | "GiB" => 1 << 30,
        "T" | "TiB" => 1 << 40,

        _ => {
            return Err(format!(
                "failed to parse byte count '{}': invalid suffix '{}'",
                input, suffix
            ));
        }
    };

    // make sure that multiplication doesn't overflow
    num.checked_mul(multiplier)
        .ok_or_else(|| format!("byte count '{}' size calculation overflowed", input))
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::str;

    use super::*;

    fn assert_print_all_output<Reader: Read>(input: Reader, expected_string: String) -> () {
        let mut output = vec![];
        let mut printer = Printer::new(&mut output, false, BorderStyle::Unicode, true);

        printer.print_all(input).unwrap();

        let actual_string: &str = str::from_utf8(&output).unwrap();
        assert_eq!(actual_string, expected_string)
    }

    #[test]
    fn empty_file_passes() {
        let input = io::empty();
        let expected_string =
            "┌────────┬─────────────────────────┬─────────────────────────┬────────┬────────┐
└────────┴─────────────────────────┴─────────────────────────┴────────┴────────┘
"
            .to_owned();
        assert_print_all_output(input, expected_string);
    }

    #[test]
    fn short_input_passes() {
        let input = io::Cursor::new(b"spam");
        let expected_string =
            "┌────────┬─────────────────────────┬─────────────────────────┬────────┬────────┐
│00000000│ 73 70 61 6d             ┊                         │spam    ┊        │
└────────┴─────────────────────────┴─────────────────────────┴────────┴────────┘
"
            .to_owned();
        assert_print_all_output(input, expected_string);
    }

    #[test]
    fn display_offset() {
        let input = io::Cursor::new(b"spamspamspamspamspam");
        let expected_string =
            "┌────────┬─────────────────────────┬─────────────────────────┬────────┬────────┐
│deadbeef│ 73 70 61 6d 73 70 61 6d ┊ 73 70 61 6d 73 70 61 6d │spamspam┊spamspam│
│deadbeff│ 73 70 61 6d             ┊                         │spam    ┊        │
└────────┴─────────────────────────┴─────────────────────────┴────────┴────────┘
"
            .to_owned();

        let mut output = vec![];
        let mut printer: Printer<Vec<u8>> =
            Printer::new(&mut output, false, BorderStyle::Unicode, true);
        printer.display_offset(0xdeadbeef);

        printer.print_all(input).unwrap();

        let actual_string: &str = str::from_utf8(&output).unwrap();
        assert_eq!(actual_string, expected_string)
    }

    #[test]
    fn parse_byte_count() {
        use super::parse_byte_count as pbc; // alias for brevity

        assert_eq!(pbc("0"), Ok(0));
        assert_eq!(pbc("1"), Ok(1));
        assert_eq!(pbc("100"), Ok(100));
        assert_eq!(pbc("1KB"), Ok(1000));
        assert_eq!(pbc("2MB"), Ok(2000000));
        assert_eq!(pbc("3GB"), Ok(3000000000));
        assert_eq!(pbc("4TB"), Ok(4000000000000));
        assert_eq!(pbc("1k"), Ok(1024));
        assert_eq!(pbc("10K"), Ok(10240));
        assert_eq!(pbc("1M"), Ok(1048576));
        assert_eq!(pbc("1G"), Ok(1073741824));
        assert_eq!(pbc("1GiB"), Ok(1073741824));
        assert_eq!(pbc("2TiB"), Ok(2199023255552));
        assert_eq!(pbc("0xff"), Ok(255));
        assert_eq!(pbc("0xEE"), Ok(238));

        // empty string is invalid
        assert!(pbc("").is_err());
        // leading/trailing space is invalid
        assert!(pbc(" 0").is_err());
        assert!(pbc("0 ").is_err());
        // invalid suffix
        assert!(pbc("1234asdf").is_err());
        // bad numbers
        assert!(pbc("asdf1234").is_err());
        assert!(pbc("a1s2d3f4").is_err());
        // multiplication overflows u64
        assert!(pbc("20000000TiB").is_err());
    }
}
