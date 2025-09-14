use ansi_escapers::{interpreter::*, types::*};

/// Utilities for converting ANSI escape sequences to HTML for colored log output.
/// Provides HTML escaping and color mapping for terminal output.

/// Escapes HTML special characters to prevent XSS attacks.

///
/// Converts &, <, >, ", and ' to their HTML entity equivalents.

pub fn escape_html(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '&' => "&amp;".to_string(),

            '<' => "&lt;".to_string(),

            '>' => "&gt;".to_string(),

            '"' => "&quot;".to_string(),

            '\'' => "&#39;".to_string(),

            _ => c.to_string(),
        })
        .collect()
}

// ANSI color constants for mapping terminal colors to HTML hex codes.
const COLOR_BLACK: &str = "#000000";
const COLOR_RED: &str = "#FF0000";
const COLOR_GREEN: &str = "#00FF00";
const COLOR_YELLOW: &str = "#FFFF00";
const COLOR_BLUE: &str = "#0000FF";
const COLOR_MAGENTA: &str = "#FF00FF";
const COLOR_CYAN: &str = "#00FFFF";
const COLOR_WHITE: &str = "#FFFFFF";
const COLOR_BRIGHT_BLACK: &str = "#808080";
const COLOR_BRIGHT_RED: &str = "#FF8080";
const COLOR_BRIGHT_GREEN: &str = "#80FF80";
const COLOR_BRIGHT_YELLOW: &str = "#FFFF80";
const COLOR_BRIGHT_BLUE: &str = "#8080FF";
const COLOR_BRIGHT_MAGENTA: &str = "#FF80FF";
const COLOR_BRIGHT_CYAN: &str = "#80FFFF";
const COLOR_BRIGHT_WHITE: &str = "#FFFFFF";
const COLOR_DEFAULT: &str = "#FFFFFF";

/// Maps an ANSI color code to a CSS hex color string.
fn get_color_from_code(col: &Color) -> String {
    let basic_color_catch = match col {
        Color::Black => COLOR_BLACK.to_string(),
        Color::Red => COLOR_RED.to_string(),
        Color::Green => COLOR_GREEN.to_string(),
        Color::Yellow => COLOR_YELLOW.to_string(),
        Color::Blue => COLOR_BLUE.to_string(),
        Color::Magenta => COLOR_MAGENTA.to_string(),
        Color::Cyan => COLOR_CYAN.to_string(),
        Color::White => COLOR_WHITE.to_string(),
        Color::BrightBlack => COLOR_BRIGHT_BLACK.to_string(),
        Color::BrightRed => COLOR_BRIGHT_RED.to_string(),
        Color::BrightGreen => COLOR_BRIGHT_GREEN.to_string(),
        Color::BrightYellow => COLOR_BRIGHT_YELLOW.to_string(),
        Color::BrightBlue => COLOR_BRIGHT_BLUE.to_string(),
        Color::BrightMagenta => COLOR_BRIGHT_MAGENTA.to_string(),
        Color::BrightCyan => COLOR_BRIGHT_CYAN.to_string(),
        Color::BrightWhite => COLOR_BRIGHT_WHITE.to_string(),
        Color::Rgb24 { r, g, b } => return format!("#{:02X}{:02X}{:02X}", r, g, b),
        Color::AnsiValue(c) => match c {
            0..=15 => match c {
                0 => COLOR_BLACK.to_string(),
                1 => COLOR_RED.to_string(),
                2 => COLOR_GREEN.to_string(),
                3 => COLOR_YELLOW.to_string(),
                4 => COLOR_BLUE.to_string(),
                5 => COLOR_MAGENTA.to_string(),
                6 => COLOR_CYAN.to_string(),
                7 => COLOR_WHITE.to_string(),
                8 => COLOR_BRIGHT_BLACK.to_string(),
                9 => COLOR_BRIGHT_RED.to_string(),
                10 => COLOR_BRIGHT_GREEN.to_string(),
                11 => COLOR_BRIGHT_YELLOW.to_string(),
                12 => COLOR_BRIGHT_BLUE.to_string(),
                13 => COLOR_BRIGHT_MAGENTA.to_string(),
                14 => COLOR_BRIGHT_CYAN.to_string(),
                15 => COLOR_BRIGHT_WHITE.to_string(),
                _ => COLOR_DEFAULT.to_string(),
            },
            _ => "".to_string(),
        },
    };

    basic_color_catch
}

/// Converts a vector of SGR attributes to a CSS style string.
fn get_html_style(codes: Vec<SgrAttribute>) -> String {
    codes
        .iter()
        .map(|code| -> String {
            match code {
                SgrAttribute::Background(col) => {
                    format!("background-color: {};", get_color_from_code(col))
                }
                SgrAttribute::UnderlineColor(col) => {
                    format!("text-decoration-color: {};", get_color_from_code(col))
                }
                SgrAttribute::Foreground(col) => {
                    format!("color: {};", get_color_from_code(col))
                }
                SgrAttribute::Bold => "font-weight: bold;".to_string(),
                SgrAttribute::Faint => "opacity: 0.7;".to_string(),
                SgrAttribute::Italic => "font-style: italic;".to_string(),
                SgrAttribute::Underline => "text-decoration: underline;".to_string(),
                SgrAttribute::Reverse => "filter: invert(100%);".to_string(),
                SgrAttribute::Conceal => "color: transparent;".to_string(),
                SgrAttribute::CrossedOut => "text-decoration: line-through;".to_string(),
                SgrAttribute::Reset => "".to_string(),
                _ => "".to_string(),
            }
        })
        .filter(|s| !s.is_empty())
        .collect::<Vec<String>>()
        .join(" ")
}

/// Converts a string containing ANSI escape sequences to HTML with inline styles.
///
/// Preserves newlines and applies color and style spans for terminal output.
///
/// # Arguments
///
/// * `inp` - The input string containing ANSI escape sequences.
///
/// # Returns
///
/// A `String` containing HTML with inline styles representing the original ANSI formatting.
pub fn ansi_to_html(inp: &str) -> String {
    // Pre-process input to preserve newlines before ANSI parsing
    let inp = inp.replace("\n", "\\n").replace("\r", "\\r");

    let mut interpreter = AnsiParser::new(&inp);
    let parse_result = interpreter.parse_annotated();

    // If there are no spans, just return the escaped text
    if parse_result.spans.is_empty() {
        // Restore newlines in the output
        return escape_html(&parse_result.text)
            .replace("\\n", "<br>")
            .replace("\\r", "");
    }

    // Create styled spans
    let mut styled_spans = Vec::new();
    for span in &parse_result.spans {
        if span.end > parse_result.text.len() {
            continue;
        }

        let style = get_html_style(span.codes.clone());
        // Escape HTML and preserve newlines by converting to <br>
        let content = escape_html(&parse_result.text[span.start..span.end])
            .replace("\\n", "<br>")
            .replace("\\r", "");

        styled_spans.push((
            span.start,
            span.end,
            format!("<span style=\"{}\">{}</span>", style, content),
        ));
    }

    // Sort spans by start position
    styled_spans.sort_by_key(|&(start, _, _)| start);

    // Build the final output by replacing text with styled spans
    let mut result = String::new();
    let mut current_pos = 0;

    for (start, end, styled_span) in styled_spans {
        // Add any text before this span
        if start > current_pos {
            result.push_str(
                &escape_html(&parse_result.text[current_pos..start])
                    .replace("\\n", "<br>")
                    .replace("\\r", ""),
            );
        }

        // Add the styled span
        result.push_str(&styled_span);

        // Update current position
        current_pos = end;
    }

    // Add any remaining text after the last span
    if current_pos < parse_result.text.len() {
        result.push_str(
            &escape_html(&parse_result.text[current_pos..])
                .replace("\\n", "<br>")
                .replace("\\r", ""),
        );
    }

    result
}
