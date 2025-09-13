use chrono::{Local, LocalResult, NaiveDate, NaiveTime, TimeZone};
use colored::Colorize;
use once_cell::sync::OnceCell;
use regex::Regex;
use std::{
    backtrace::Backtrace,
    fs::{self, OpenOptions},
    io::{self, Write},
    marker::Send,
    path::{Path, PathBuf},
};
use tracing_subscriber::{
    fmt::{format::Writer, writer::MakeWriter},
    EnvFilter,
};

const LOGS_PATH: &str = "./logs/";

/// MultiWriter writes logs to both stdout and a file, stripping ANSI codes for the file.
pub struct MultiWriter {
    pub log_path: PathBuf,
}

impl<'a> MakeWriter<'a> for MultiWriter {
    type Writer = MultiWriterHandle;

    fn make_writer(&'a self) -> Self::Writer {
        let file = match OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_path)
        {
            Ok(f) => Some(f),
            Err(e) => {
                eprintln!(
                    "Failed to create or open log file {:?}: {}",
                    self.log_path, e
                );
                None
            }
        };
        MultiWriterHandle { file }
    }
}

pub struct MultiWriterHandle {
    file: Option<std::fs::File>,
}

// SAFETY: MultiWriterHandle only contains a File, which is Send + 'static.
unsafe impl Send for MultiWriterHandle {}
unsafe impl Sync for MultiWriterHandle {}

impl Write for MultiWriterHandle {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        // Write original buffer to stdout
        if let Err(e) = io::stdout().write_all(buf) {
            eprintln!("Error writing to stdout: {}", e);
            return Err(e);
        }

        // Write ANSI-stripped text to file
        if let Some(f) = &mut self.file {
            let s = std::str::from_utf8(buf).unwrap_or("");
            let mut parser = ansi_escapers::interpreter::AnsiParser::new(s);
            let text = parser.parse_annotated().text;
            if let Err(e) = f.write_all(text.as_bytes()) {
                eprintln!("Error writing to log file: {}", e);
                return Err(e);
            }
        }

        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        if let Err(e) = io::stdout().flush() {
            eprintln!("Error flushing stdout: {}", e);
            return Err(e);
        }
        if let Some(f) = &mut self.file {
            if let Err(e) = f.flush() {
                eprintln!("Error flushing log file: {}", e);
                return Err(e);
            }
        }
        Ok(())
    }
}
/// Custom timer for 12-hour time format with AM/PM for tracing_subscriber log output.
struct Custom12HourTimer;

impl tracing_subscriber::fmt::time::FormatTime for Custom12HourTimer {
    fn format_time(&self, w: &mut Writer<'_>) -> std::fmt::Result {
        let now = chrono::Local::now();
        write!(w, "{}", now.format("[%I:%M:%S %p]"))
    }
}

pub fn init_logging() {
    // Set warn for all dependencies by default
    // Only allow trace for the local crate, block trace for all external crates
    let mut filter = EnvFilter::builder()
        .with_default_directive(tracing::Level::WARN.into())
        .from_env_lossy();

    // Only allow trace for the local crate (assume crate name is "rust_server_controller")
    // You may need to adjust the crate name if it differs
    #[cfg(debug_assertions)]
    {
        filter = filter
            .add_directive("rust_server_controller=trace".parse().unwrap())
            .add_directive("tokio=warn".parse().unwrap())
            .add_directive("hyper=warn".parse().unwrap())
            .add_directive("tracing=warn".parse().unwrap())
            .add_directive("tower=warn".parse().unwrap())
            .add_directive("warp=warn".parse().unwrap())
            .add_directive("serde=warn".parse().unwrap())
            .add_directive("reqwest=warn".parse().unwrap())
            .add_directive("axum=warn".parse().unwrap())
            .add_directive("sqlx=warn".parse().unwrap())
            .add_directive("mio=warn".parse().unwrap())
            .add_directive("tokio_util=warn".parse().unwrap());
    }

    #[cfg(not(debug_assertions))]
    {
        filter = filter
            .add_directive("rust_server_controller=info".parse().unwrap())
            .add_directive("tokio=warn".parse().unwrap())
            .add_directive("hyper=warn".parse().unwrap())
            .add_directive("tracing=warn".parse().unwrap())
            .add_directive("tower=warn".parse().unwrap())
            .add_directive("warp=warn".parse().unwrap())
            .add_directive("serde=warn".parse().unwrap())
            .add_directive("reqwest=warn".parse().unwrap())
            .add_directive("axum=warn".parse().unwrap())
            .add_directive("sqlx=warn".parse().unwrap())
            .add_directive("mio=warn".parse().unwrap())
            .add_directive("tokio_util=warn".parse().unwrap());
    }

    // Use only the subcrate name as the log file name, with .log extension.
    // Get the current date and time at initialization
    let now = chrono::Local::now();
    let date_str = now.format("%m-%d-%Y").to_string();
    let time_str = now.format("%I-%M-%S_%p").to_string();

    static LOG_FILE_PATH: OnceCell<PathBuf> = OnceCell::new();
    let log_path = {
        let mut path = PathBuf::from(LOGS_PATH);
        // Use CARGO_PKG_NAME for subcrate name, and include date/time for uniqueness
        let subcrate = env!("CARGO_PKG_NAME");
        path.push(format!("{subcrate}_{date_str}_{time_str}.log"));
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        // Store the log path in the static for panic hook access
        let _ = LOG_FILE_PATH.set(path.clone());
        path
    };

    // Write the first line: "Logs start on {date} at {time}"
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
    {
        let _ = writeln!(
            file,
            "Logs start on {} at {}",
            now.format("%m-%d-%Y"),
            now.format("%I:%M:%S %p")
        );
    }
    let writer = MultiWriter { log_path };

    if let Err(e) = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(writer)
        .with_timer(Custom12HourTimer)
        .try_init()
    {
        eprintln!("Failed to set tracing subscriber: {}", e);
    }

    /// Set a panic hook that logs panics using tracing::error! and [FATAL] prefix, including stacktrace.
    pub fn set_panic_hook() {
        let default_hook = std::panic::take_hook();
        static CRATE_NAME: &str = env!("CARGO_PKG_NAME");
        std::panic::set_hook(Box::new(move |panic_info| {
            let thread_name = std::thread::current()
                .name()
                .map(|s| s.to_string())
                .unwrap_or_else(|| "<unnamed>".to_string());
            let panic_msg = match panic_info.payload().downcast_ref::<&str>() {
                Some(s) => *s,
                None => match panic_info.payload().downcast_ref::<String>() {
                    Some(s) => s.as_str(),
                    None => "Box<Any>",
                },
            };
            let location = panic_info.location();
            let msg = match location {
                Some(loc) => format!(
                    "Panic occurred in thread '{}': {}\nAt {}:{}",
                    thread_name,
                    panic_msg,
                    loc.file(),
                    loc.line()
                ),
                None => format!("Panic occurred in thread '{}': {}", thread_name, panic_msg),
            };

            // Format time as [HH:MM:SS AM/PM]
            let now = Local::now();
            let time_str = now.format("[%I:%M:%S %p]").to_string();

            // Color for FATAL (bright red) using colored crate
            let fatal_color = "FATAL".red().bold();
            let faded_time = time_str.dimmed();
            let faded_crate = CRATE_NAME.dimmed();
            let faded_colon = ":".dimmed();

            // Print the main panic message with faded time, crate, and colon, reset color for message
            eprintln!(
                "{} {} {}{} {}",
                faded_time, fatal_color, faded_crate, faded_colon, msg
            );

            // Print the stacktrace, each line as FATAL, faded time/crate/colon, reset for line
            let backtrace = Backtrace::force_capture();
            let backtrace_str = format!("{}", backtrace);
            for line in backtrace_str.lines() {
                eprintln!(
                    "{} {} {}{} {}",
                    faded_time, fatal_color, faded_crate, faded_colon, line
                );
            }

            // Also append the colorless version to the main log file for post-mortem visibility
            if let Some(log_path) = LOG_FILE_PATH.get() {
                if let Ok(mut file) = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(log_path)
                {
                    use std::io::Write;
                    let _ = writeln!(file, "{} FATAL {}: {}", time_str, CRATE_NAME, msg);
                    for line in backtrace_str.lines() {
                        let _ = writeln!(file, "{} FATAL {}: {}", time_str, CRATE_NAME, line);
                    }
                }
            }

            // Optionally call the default hook to also print to stderr (for default panic output)
            default_hook(panic_info);
        }));
    }

    set_panic_hook();
}

/// Function to deliberately cause a panic for testing the panic hook and logging.
#[allow(dead_code)]
pub fn test_panic() {
    panic!("This is a test panic from logging::test_panic()");
}
#[allow(dead_code)]
pub fn cleanup_old_logs<P: AsRef<Path>>(logs_dir: P, keep_for: std::time::Duration) {
    let logs_dir = logs_dir.as_ref();
    let now = Local::now();

    // Regex for schema: {subcrate}_{MM-DD-YYYY}_{HH-MM-SS_AMPM}.log
    // Example: calendar_server_04-27-2024_09-15-23_PM.log
    let re = Regex::new(r"^[^_]+_(\d{2})-(\d{2})-(\d{4})_(\d{2})-(\d{2})-(\d{2})_(AM|PM)\.log$")
        .expect("Failed to compile regex for log file schema");

    if let Ok(entries) = fs::read_dir(logs_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            if let Some(fname) = path.file_name().and_then(|n| n.to_str()) {
                if let Some(caps) = re.captures(fname) {
                    // Parse date and time from filename
                    let month = caps[1].parse::<u32>().ok();
                    let day = caps[2].parse::<u32>().ok();
                    let year = caps[3].parse::<i32>().ok();
                    let hour = caps[4].parse::<u32>().ok();
                    let minute = caps[5].parse::<u32>().ok();
                    let second = caps[6].parse::<u32>().ok();
                    let ampm = &caps[7];

                    if let (
                        Some(month),
                        Some(day),
                        Some(year),
                        Some(mut hour),
                        Some(minute),
                        Some(second),
                    ) = (month, day, year, hour, minute, second)
                    {
                        // Convert to 24-hour time
                        if ampm == "PM" && hour != 12 {
                            hour += 12;
                        }
                        if ampm == "AM" && hour == 12 {
                            hour = 0;
                        }
                        let date = NaiveDate::from_ymd_opt(year, month, day);
                        let time = NaiveTime::from_hms_opt(hour, minute, second);
                        if let (Some(date), Some(time)) = (date, time) {
                            let naive_dt = date.and_time(time);
                            match Local.from_local_datetime(&naive_dt) {
                                LocalResult::Single(file_dt) => {
                                    let age = now
                                        .signed_duration_since(file_dt)
                                        .to_std()
                                        .unwrap_or_default();
                                    if age > keep_for {
                                        let _ = fs::remove_file(&path);
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
    }
}
