/// Main entry point and initialization logic for the Rust Server Controller.
///
/// This module sets up the application state, loads configuration, ensures the themes directory exists,
/// and starts the appropriate async tasks for master or slave mode.
mod ansi_to_html;
mod app_state;
mod configuration;
mod controlled_program;
mod files;
mod logging;
mod macros;
mod master;
mod messages;
mod servers;
mod slave;
mod specializations;
mod theme;
mod webserver;
mod websocket;

use files::*;
use std::{fs, path::Path, process::exit};
use tokio::{spawn, sync::broadcast};
use tracing::*;

use crate::{
    master::create_slave_connections,
    servers::start_servers,
    slave::start_slave,
    theme::{oklch, Theme},
    webserver::start_web_server,
};

/// Main async entry point for the application.
///
/// Loads configuration, initializes logging, ensures the themes directory exists,
/// and starts the appropriate async tasks for master or slave mode.
/// Handles graceful shutdown on Ctrl+C or T key.
#[tokio::main]
async fn main() -> Result<(), String> {
    let config = load_json("config.json");
    let slave: bool = config.slave;
    logging::init_logging();

    // Ensure themes directory exists with default themes
    ensure_themes_directory(&config);
    let (tx, _rx) = broadcast::channel(100);
    let specialization_registry = specializations::init_builtin_registry();
    let mut app_state = app_state::AppState::new(tx, config, specialization_registry);
    let handles: Vec<tokio::task::JoinHandle<()>> = if slave {
        spawn_tasks!(app_state.clone(), start_servers, start_slave)
    } else {
        spawn_tasks!(
            app_state.clone(),
            start_web_server,
            start_servers,
            create_slave_connections
        )
    };
    {
        info!("Starting {} tasks", handles.len());
    }
    // Spawn shutdown handler to kill all child processes on exit
    let app_state_clone = app_state.clone();
    let shutdown = async move |reason: &str| {
        info!(
            "Shutdown signal received ({}), terminating all child processes...",
            reason
        );
        let mut servers = app_state_clone.servers.lock().await;
        for server in servers.iter_mut() {
            let _ = server.stop().await;
        }
        info!("All child processes terminated.");
        std::process::exit(0);
    };

    // Ctrl+C handler
    #[allow(unused_variables)]
    let app_state_clone_ctrlc = app_state.clone();
    let _app_state_clone_ctrlc = app_state.clone();
    tokio::spawn({
        let shutdown = shutdown.clone();
        async move {
            use tokio::signal;
            signal::ctrl_c().await.expect("Failed to listen for ctrl_c");
            shutdown("ctrl_c").await;
        }
    });

    // T key handler
    #[allow(unused_variables)]
    let app_state_clone_t = app_state.clone();
    let _app_state_clone_t = app_state.clone();
    tokio::spawn({
        let shutdown = shutdown.clone();
        async move {
            use crossterm::event::{poll, read, Event, KeyCode};
            use tokio::task::yield_now;
            loop {
                yield_now().await;
                if poll(std::time::Duration::from_millis(25)).expect("Failed to poll for events") {
                    if let Event::Key(key_event) = read().expect("Failed to read event") {
                        if key_event.code == KeyCode::Char('t') {
                            shutdown("T key").await;
                        }
                    }
                }
            }
        }
    });

    let _ = tokio::spawn(async_listener!("t", app_state)).await;
    exit(0);
}

// Ensures that the themes directory exists and contains at least default themes.
/// Ensures that the themes directory exists and contains at least default themes.
///
/// If the directory does not exist or is empty, creates default dark, light, and high contrast themes,
/// a README with instructions, and a sample custom theme.
///
/// # Arguments
/// * `config` - The application configuration containing the themes folder path.
fn ensure_themes_directory(config: &crate::configuration::Config) {
    let themes_dir = config
        .themes_folder
        .clone()
        .unwrap_or_else(|| "themes".to_string());
    let themes_path = Path::new(&themes_dir);

    // Check if themes directory exists, create it if not
    if !themes_path.exists() {
        info!("Creating themes directory: {}", themes_dir);
        if let Err(e) = fs::create_dir_all(themes_path) {
            error!("Failed to create themes directory: {}", e);
            return;
        }
    }

    // Check if the directory is empty
    let is_empty = match fs::read_dir(themes_path) {
        Ok(entries) => entries.count() == 0,
        Err(_) => true, // Treat errors as if directory is empty
    };

    // Create default themes if directory is empty
    if is_empty {
        info!("Creating default themes in directory: {}", themes_dir);

        // Create default dark theme
        let dark_theme = Theme::default(); // Default is already dark theme
        let dark_theme_path = themes_path.join("default_dark.json");
        if let Err(e) = dark_theme.save_to_file(&dark_theme_path) {
            error!("Failed to save default dark theme: {}", e);
        } else {
            info!("Created default dark theme at: {:?}", dark_theme_path);
        }

        // Create light theme
        let light_theme = Theme::light();
        let light_theme_path = themes_path.join("default_light.json");
        if let Err(e) = light_theme.save_to_file(&light_theme_path) {
            error!("Failed to save default light theme: {}", e);
        } else {
            info!("Created default light theme at: {:?}", light_theme_path);
        }

        // Create high contrast theme
        let high_contrast_theme = Theme::high_contrast();
        let high_contrast_path = themes_path.join("high_contrast.json");
        if let Err(e) = high_contrast_theme.save_to_file(&high_contrast_path) {
            error!("Failed to save high contrast theme: {}", e);
        } else {
            info!("Created high contrast theme at: {:?}", high_contrast_path);
        }

        // Create a README.md file with theme creation instructions
        let readme_path = themes_path.join("README.md");
        let readme_content = r#"# Theme Creation Guide

This directory contains theme files for the Rust Server Controller application. You can create your own custom themes by creating new JSON files in this directory.

## Theme File Format

Theme files are JSON files that define colors for the UI. Each theme should have the following structure:

```json
{
  "name": "My Custom Theme",
  "color_space": "Oklch",
  "bg_dark": { "l": 0.1, "c": 0.05, "h": 290.0, "a": 1.0 },
  "bg": { "l": 0.15, "c": 0.05, "h": 290.0, "a": 1.0 },
  "bg_light": { "l": 0.2, "c": 0.05, "h": 290.0, "a": 1.0 },
  "text": { "l": 0.96, "c": 0.02, "h": 290.0, "a": 1.0 },
  "text_muted": { "l": 0.76, "c": 0.02, "h": 290.0, "a": 1.0 },
  "highlight": { "l": 0.5, "c": 0.2, "h": 290.0, "a": 1.0 },
  "border": { "l": 0.4, "c": 0.1, "h": 290.0, "a": 1.0 },
  "border_muted": { "l": 0.3, "c": 0.05, "h": 290.0, "a": 1.0 },
  "primary": { "l": 0.7, "c": 0.25, "h": 290.0, "a": 1.0 },
  "secondary": { "l": 0.7, "c": 0.2, "h": 250.0, "a": 1.0 },
  "danger": { "l": 0.7, "c": 0.2, "h": 30.0, "a": 1.0 },
  "warning": { "l": 0.7, "c": 0.2, "h": 100.0, "a": 1.0 },
  "success": { "l": 0.7, "c": 0.2, "h": 140.0, "a": 1.0 },
  "info": { "l": 0.7, "c": 0.2, "h": 260.0, "a": 1.0 }
}
```

## Color Properties

- `name`: The display name of the theme (shown in the theme selector)
- `color_space`: The color space used (Oklch is recommended)
- Color values use the following properties:
  - `l`: Lightness (0 to 1)
  - `c`: Chroma/saturation (0 to 0.4 is a good range)
  - `h`: Hue angle in degrees (0 to 360)
  - `a`: Alpha/opacity (0 to 1)

## Color Meanings

- `bg_dark`, `bg`, `bg_light`: Background colors (dark to light)
- `text`, `text_muted`: Text colors (primary and secondary)
- `highlight`: Used for UI highlights
- `border`, `border_muted`: Border colors
- `primary`, `secondary`: Primary and secondary brand colors
- `danger`, `warning`, `success`, `info`: Semantic colors for feedback

## Tips

- Study the examples in this directory to understand how colors work together
- The default themes are good starting points
- Try small changes first to understand how they affect the UI
- Use Oklch color space for better perceptual uniformity
- Keep contrast ratios high for better accessibility
"#;

        if let Err(e) = fs::write(&readme_path, readme_content) {
            error!("Failed to create themes README file: {}", e);
        } else {
            info!("Created themes README at: {:?}", readme_path);
        }

        // Create custom theme example (purple-based theme)
        let mut custom_theme = Theme {
            name: "Purple Dream".to_string(),
            ..Theme::default()
        };

        // Use public oklch function to create colors
        custom_theme.primary = oklch(0.7, 0.25, 290.0); // Vibrant purple
        custom_theme.secondary = oklch(0.7, 0.2, 250.0); // Blue-purple
        custom_theme.bg_dark = oklch(0.1, 0.05, 290.0); // Dark purple-tinted
        custom_theme.bg = oklch(0.15, 0.05, 290.0); // Dark purple-tinted
        custom_theme.bg_light = oklch(0.2, 0.05, 290.0); // Medium purple-tinted
        custom_theme.highlight = oklch(0.7, 0.2, 290.0); // Purple highlight
        custom_theme.success = oklch(0.7, 0.2, 140.0); // Green
        custom_theme.danger = oklch(0.7, 0.25, 30.0); // Red-orange
        custom_theme.warning = oklch(0.7, 0.2, 80.0); // Amber
        custom_theme.info = oklch(0.7, 0.2, 210.0); // Blue
        custom_theme.text = oklch(0.96, 0.02, 290.0); // Light text with slight purple tint
        custom_theme.text_muted = oklch(0.76, 0.02, 290.0); // Medium light text with slight purple tint

        let custom_theme_path = themes_path.join("purple_dream.json");
        if let Err(e) = custom_theme.save_to_file(&custom_theme_path) {
            error!("Failed to save custom purple theme: {}", e);
        } else {
            info!("Created custom purple theme at: {:?}", custom_theme_path);
        }
    }
}
