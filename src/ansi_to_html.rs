use ansi_escapers::{interpreter::*, types::*};

// ANSI color constants
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

pub fn ansi_to_html(inp: &str) -> String {
    let mut interpreter = AnsiParser::new(inp);
    let parse_result = interpreter.parse_annotated();
    parse_result
        .spans
        .iter()
        .map(|span| -> String {
            if span.end > parse_result.text.len() {
                return "".to_string();
            }
            let mut res: String = String::new();
            res += format!(
                "<span style=\"{}\">{}</span>",
                get_html_style(span.codes.clone()),
                &parse_result.text[span.start..span.end]
            )
            .as_str();

            res
        })
        .filter(|x| !x.is_empty())
        //.map(|x| x.clone())
        .collect::<Vec<String>>()
        .join("")
}
