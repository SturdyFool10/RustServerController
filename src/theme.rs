use colorlab::colorspaces::{
    color::Color, colorspace::ColorSpace, hsl::Hsl, lch::Lch, oklab::Oklab, oklch::Oklch,
    srgb::Srgb,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Represents the available color spaces that can be used for themes
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub enum ThemeColorSpace {
    #[default]
    Oklch,
    Oklab,
    Lch,
    Srgb,
    DisplayP3,
    AdobeRgb,
    Rec2020,
    Hsl,
    Hsv,
    Hwb,
    Lab,
    Luv,
    Xyz,
}

/// A struct representing a complete theme for the application
/// Contains all CSS variables needed to style the UI
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Theme {
    // Base colors
    pub bg_dark: Color,
    pub bg: Color,
    pub bg_light: Color,
    pub text: Color,
    pub text_muted: Color,
    pub highlight: Color,
    pub border: Color,
    pub border_muted: Color,

    // Semantic colors
    pub primary: Color,
    pub secondary: Color,
    pub danger: Color,
    pub warning: Color,
    pub success: Color,
    pub info: Color,

    // Theme metadata
    pub name: String,
    pub color_space: ThemeColorSpace,
}

impl Default for Theme {
    fn default() -> Self {
        // Create a dark theme using Oklch
        Self {
            // Base colors
            bg_dark: oklch_to_color(0.1, 0.01, 256.0, 1.0),
            bg: oklch_to_color(0.15, 0.01, 256.0, 1.0),
            bg_light: oklch_to_color(0.2, 0.01, 256.0, 1.0),
            text: oklch_to_color(0.96, 0.02, 256.0, 1.0),
            text_muted: oklch_to_color(0.76, 0.02, 256.0, 1.0),
            highlight: oklch_to_color(0.5, 0.02, 256.0, 1.0),
            border: oklch_to_color(0.4, 0.02, 256.0, 1.0),
            border_muted: oklch_to_color(0.3, 0.02, 256.0, 1.0),

            // Semantic colors
            primary: oklch_to_color(0.76, 0.2, 256.0, 1.0),
            secondary: oklch_to_color(0.76, 0.2, 76.0, 1.0),
            danger: oklch_to_color(0.7, 0.2, 30.0, 1.0),
            warning: oklch_to_color(0.7, 0.2, 100.0, 1.0),
            success: oklch_to_color(0.7, 0.2, 160.0, 1.0),
            info: oklch_to_color(0.7, 0.2, 260.0, 1.0),

            // Theme name
            name: "Default Dark".to_string(),
            color_space: ThemeColorSpace::Oklch,
        }
    }
}

// Helper function to convert Oklch values to a Color
fn oklch_to_color(l: f64, c: f64, h: f64, alpha: f64) -> Color {
    Oklch { l, c, h, alpha }.to_color()
}

// Helper function to convert Oklab values to a Color
#[allow(unused)]
fn oklab_to_color(l: f64, a: f64, b: f64, alpha: f64) -> Color {
    Oklab { l, a, b, alpha }.to_color()
}

#[allow(dead_code)]
impl Theme {
    /// Creates a new theme with the specified name
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            ..Self::default()
        }
    }

    /// Create a light theme variant
    pub fn light() -> Self {
        Self {
            // Base colors - inverted from dark theme
            bg_dark: oklch_to_color(0.92, 0.055, 264.0, 1.0),
            bg: oklch_to_color(0.96, 0.055, 264.0, 1.0),
            bg_light: oklch_to_color(1.0, 0.055, 264.0, 1.0),
            text: oklch_to_color(0.15, 0.11, 264.0, 1.0),
            text_muted: oklch_to_color(0.4, 0.11, 264.0, 1.0),
            highlight: oklch_to_color(1.0, 0.11, 264.0, 1.0),
            border: oklch_to_color(0.6, 0.11, 264.0, 1.0),
            border_muted: oklch_to_color(0.7, 0.11, 264.0, 1.0),

            // Semantic colors - specifically set for light theme
            primary: oklch_to_color(0.4, 0.2, 264.0, 1.0),
            secondary: oklch_to_color(0.4, 0.2, 84.0, 1.0),
            danger: oklch_to_color(0.5, 0.2, 30.0, 1.0),
            warning: oklch_to_color(0.5, 0.2, 100.0, 1.0),
            success: oklch_to_color(0.5, 0.2, 160.0, 1.0),
            info: oklch_to_color(0.5, 0.2, 260.0, 1.0),

            // Theme metadata
            name: "Default Light".to_string(),
            color_space: ThemeColorSpace::Oklch,
        }
    }

    /// Create a high contrast theme for accessibility
    pub fn high_contrast() -> Self {
        Self {
            bg_dark: oklch_to_color(0.05, 0.01, 256.0, 1.0),
            bg: oklch_to_color(0.08, 0.01, 256.0, 1.0),
            bg_light: oklch_to_color(0.12, 0.01, 256.0, 1.0),
            text: oklch_to_color(0.99, 0.03, 256.0, 1.0),
            text_muted: oklch_to_color(0.90, 0.03, 256.0, 1.0),
            highlight: oklch_to_color(0.7, 0.03, 256.0, 1.0),
            border: oklch_to_color(0.6, 0.03, 256.0, 1.0),
            border_muted: oklch_to_color(0.5, 0.03, 256.0, 1.0),

            primary: oklch_to_color(0.85, 0.25, 256.0, 1.0),
            secondary: oklch_to_color(0.85, 0.25, 76.0, 1.0),
            danger: oklch_to_color(0.8, 0.25, 30.0, 1.0),
            warning: oklch_to_color(0.8, 0.25, 100.0, 1.0),
            success: oklch_to_color(0.8, 0.25, 160.0, 1.0),
            info: oklch_to_color(0.8, 0.25, 260.0, 1.0),

            name: "High Contrast".to_string(),
            color_space: ThemeColorSpace::Oklch,
        }
    }

    /// Convert a color to a CSS string based on the selected color space
    fn color_to_css_string(&self, color: &Color) -> String {
        match self.color_space {
            ThemeColorSpace::Oklch => {
                let oklch = Oklch::from_color(color);
                format!("oklch({} {} {})", oklch.l, oklch.c, oklch.h)
            }
            ThemeColorSpace::Oklab => {
                let oklab = Oklab::from_color(color);
                format!("oklab({} {} {})", oklab.l, oklab.a, oklab.b)
            }
            ThemeColorSpace::Lch => {
                let lch = Lch::from_color(color);
                format!("lch({} {} {})", lch.l, lch.c, lch.h)
            }
            ThemeColorSpace::Srgb => {
                let srgb = Srgb::from_color(color);
                format!(
                    "rgb({}, {}, {})",
                    (srgb.r * 255.0).round() as u8,
                    (srgb.g * 255.0).round() as u8,
                    (srgb.b * 255.0).round() as u8
                )
            }
            ThemeColorSpace::Hsl => {
                let hsl = Hsl::from_color(color);
                format!(
                    "hsl({}deg {}% {}%)",
                    hsl.h,
                    (hsl.s * 100.0).round(),
                    (hsl.l * 100.0).round()
                )
            }
            _ => {
                // Convert to Oklch for all other color spaces
                // This provides better color fidelity than sRGB
                let oklch = Oklch::from_color(color);
                format!("oklch({} {} {})", oklch.l, oklch.c, oklch.h)
            }
        }
    }

    /// Generate CSS custom properties (variables) for this theme
    pub fn to_css(&self) -> String {
        let mut css = String::from(":root {\n");

        // Add each color as a CSS variable
        css.push_str(&format!(
            "    --bg-dark: {};\n",
            self.color_to_css_string(&self.bg_dark)
        ));
        css.push_str(&format!(
            "    --bg: {};\n",
            self.color_to_css_string(&self.bg)
        ));
        css.push_str(&format!(
            "    --bg-light: {};\n",
            self.color_to_css_string(&self.bg_light)
        ));
        css.push_str(&format!(
            "    --text: {};\n",
            self.color_to_css_string(&self.text)
        ));
        css.push_str(&format!(
            "    --text-muted: {};\n",
            self.color_to_css_string(&self.text_muted)
        ));
        css.push_str(&format!(
            "    --highlight: {};\n",
            self.color_to_css_string(&self.highlight)
        ));
        css.push_str(&format!(
            "    --border: {};\n",
            self.color_to_css_string(&self.border)
        ));
        css.push_str(&format!(
            "    --border-muted: {};\n",
            self.color_to_css_string(&self.border_muted)
        ));

        css.push_str(&format!(
            "    --primary: {};\n",
            self.color_to_css_string(&self.primary)
        ));
        css.push_str(&format!(
            "    --secondary: {};\n",
            self.color_to_css_string(&self.secondary)
        ));
        css.push_str(&format!(
            "    --danger: {};\n",
            self.color_to_css_string(&self.danger)
        ));
        css.push_str(&format!(
            "    --warning: {};\n",
            self.color_to_css_string(&self.warning)
        ));
        css.push_str(&format!(
            "    --success: {};\n",
            self.color_to_css_string(&self.success)
        ));
        css.push_str(&format!(
            "    --info: {};\n",
            self.color_to_css_string(&self.info)
        ));

        // Add legacy variable mapping for backward compatibility
        css.push_str("\n    /* Legacy variable mapping for compatibility */\n");
        css.push_str("    --pageBG: var(--bg-dark);\n");
        css.push_str("    --textCol: var(--text);\n");
        css.push_str("    --menuBG: var(--bg);\n");
        css.push_str("    --highlightCol: var(--primary);\n");
        css.push_str("    --altHighlightCol: var(--secondary);\n");

        css.push_str("}\n");
        css
    }

    /// Set the color space for the theme
    pub fn set_color_space(&mut self, color_space: ThemeColorSpace) -> &mut Self {
        self.color_space = color_space;
        self
    }

    /// Save the theme to a file in JSON format
    pub fn save_to_file<P: AsRef<Path>>(&self, path: P) -> io::Result<()> {
        let json = serde_json::to_string_pretty(self)?;
        let mut file = fs::File::create(path)?;
        file.write_all(json.as_bytes())?;
        Ok(())
    }

    /// Load a theme from a JSON file
    pub fn load_from_file<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        let json = fs::read_to_string(path)?;
        let theme: Self = serde_json::from_str(&json)?;
        Ok(theme)
    }

    /// Find a theme file by name in a directory
    pub fn find_in_directory<P: AsRef<Path>>(dir_path: P, theme_name: &str) -> Option<PathBuf> {
        let dir_path = dir_path.as_ref();
        if !dir_path.exists() || !dir_path.is_dir() {
            return None;
        }

        if let Ok(entries) = fs::read_dir(dir_path) {
            for entry in entries.flatten() {
                let path = entry.path();

                // Only check JSON files
                if path.extension().is_some_and(|ext| ext == "json") {
                    if let Ok(json) = fs::read_to_string(&path) {
                        if let Ok(theme) = serde_json::from_str::<Theme>(&json) {
                            if theme.name == theme_name {
                                return Some(path);
                            }
                        }
                    }
                }
            }
        }

        None
    }

    /// Convert the Theme to a HashMap for easier manipulation
    pub fn to_map(&self) -> HashMap<String, Color> {
        let mut map = HashMap::new();
        map.insert("bg-dark".to_string(), self.bg_dark);
        map.insert("bg".to_string(), self.bg);
        map.insert("bg-light".to_string(), self.bg_light);
        map.insert("text".to_string(), self.text);
        map.insert("text-muted".to_string(), self.text_muted);
        map.insert("highlight".to_string(), self.highlight);
        map.insert("border".to_string(), self.border);
        map.insert("border-muted".to_string(), self.border_muted);
        map.insert("primary".to_string(), self.primary);
        map.insert("secondary".to_string(), self.secondary);
        map.insert("danger".to_string(), self.danger);
        map.insert("warning".to_string(), self.warning);
        map.insert("success".to_string(), self.success);
        map.insert("info".to_string(), self.info);
        map
    }

    /// Create a Theme from a HashMap
    pub fn from_map(
        map: &HashMap<String, Color>,
        name: &str,
        color_space: ThemeColorSpace,
    ) -> Self {
        Self {
            bg_dark: map
                .get("bg-dark")
                .cloned()
                .unwrap_or_else(|| oklch_to_color(0.1, 0.01, 256.0, 1.0)),
            bg: map
                .get("bg")
                .cloned()
                .unwrap_or_else(|| oklch_to_color(0.15, 0.01, 256.0, 1.0)),
            bg_light: map
                .get("bg-light")
                .cloned()
                .unwrap_or_else(|| oklch_to_color(0.2, 0.01, 256.0, 1.0)),
            text: map
                .get("text")
                .cloned()
                .unwrap_or_else(|| oklch_to_color(0.96, 0.02, 256.0, 1.0)),
            text_muted: map
                .get("text-muted")
                .cloned()
                .unwrap_or_else(|| oklch_to_color(0.76, 0.02, 256.0, 1.0)),
            highlight: map
                .get("highlight")
                .cloned()
                .unwrap_or_else(|| oklch_to_color(0.5, 0.02, 256.0, 1.0)),
            border: map
                .get("border")
                .cloned()
                .unwrap_or_else(|| oklch_to_color(0.4, 0.02, 256.0, 1.0)),
            border_muted: map
                .get("border-muted")
                .cloned()
                .unwrap_or_else(|| oklch_to_color(0.3, 0.02, 256.0, 1.0)),
            primary: map
                .get("primary")
                .cloned()
                .unwrap_or_else(|| oklch_to_color(0.76, 0.2, 256.0, 1.0)),
            secondary: map
                .get("secondary")
                .cloned()
                .unwrap_or_else(|| oklch_to_color(0.76, 0.2, 76.0, 1.0)),
            danger: map
                .get("danger")
                .cloned()
                .unwrap_or_else(|| oklch_to_color(0.7, 0.2, 30.0, 1.0)),
            warning: map
                .get("warning")
                .cloned()
                .unwrap_or_else(|| oklch_to_color(0.7, 0.2, 100.0, 1.0)),
            success: map
                .get("success")
                .cloned()
                .unwrap_or_else(|| oklch_to_color(0.7, 0.2, 160.0, 1.0)),
            info: map
                .get("info")
                .cloned()
                .unwrap_or_else(|| oklch_to_color(0.7, 0.2, 260.0, 1.0)),
            name: name.to_string(),
            color_space,
        }
    }
}

/// A collection of themes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemeCollection {
    pub themes: Vec<Theme>,
    pub current_theme: String,
}

impl Default for ThemeCollection {
    fn default() -> Self {
        Self {
            themes: vec![Theme::default(), Theme::light(), Theme::high_contrast()],
            current_theme: "Default Dark".to_string(),
        }
    }
}

#[allow(dead_code)]
impl ThemeCollection {
    /// Create a new theme collection
    pub fn new() -> Self {
        Self::default()
    }

    /// Get the current active theme
    pub fn current(&self) -> Option<&Theme> {
        self.themes.iter().find(|t| t.name == self.current_theme)
    }

    /// Set the current theme by name
    pub fn set_current(&mut self, name: &str) -> bool {
        if self.themes.iter().any(|t| t.name == name) {
            self.current_theme = name.to_string();
            true
        } else {
            false
        }
    }

    /// Add a new theme to the collection
    pub fn add_theme(&mut self, theme: Theme) {
        // Remove any existing theme with the same name
        self.themes.retain(|t| t.name != theme.name);
        self.themes.push(theme);
    }

    /// Remove a theme by name
    pub fn remove_theme(&mut self, name: &str) -> bool {
        // Don't remove the current theme
        if name == self.current_theme {
            return false;
        }

        let initial_len = self.themes.len();
        self.themes.retain(|t| t.name != name);

        initial_len != self.themes.len()
    }

    /// Save all themes to a JSON file
    pub fn save_to_file<P: AsRef<Path>>(&self, path: P) -> io::Result<()> {
        let json = serde_json::to_string_pretty(self)?;
        let mut file = fs::File::create(path)?;
        file.write_all(json.as_bytes())?;
        Ok(())
    }

    /// Load themes from a JSON file
    pub fn load_from_file<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        let json = fs::read_to_string(path)?;
        let collection: Self = serde_json::from_str(&json)?;
        Ok(collection)
    }

    /// Load all themes from a directory
    pub fn load_from_directory<P: AsRef<Path>>(dir_path: P) -> io::Result<Self> {
        let dir_path = dir_path.as_ref();

        // Check if directory exists
        if !dir_path.exists() || !dir_path.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("Theme directory not found: {:?}", dir_path),
            ));
        }

        let mut collection = ThemeCollection::default();

        // Read all JSON files in the directory
        for entry in fs::read_dir(dir_path)? {
            let entry = entry?;
            let path = entry.path();

            // Only process JSON files
            if path.extension().is_some_and(|ext| ext == "json") {
                match Theme::load_from_file(&path) {
                    Ok(theme) => {
                        collection.add_theme(theme);
                    }
                    Err(e) => {
                        eprintln!("Error loading theme from {:?}: {}", path, e);
                        // Continue with other themes
                    }
                }
            }
        }

        Ok(collection)
    }

    /// Generate CSS for the current theme
    pub fn current_theme_css(&self) -> Option<String> {
        self.current().map(|theme| theme.to_css())
    }

    /// Load themes from the configuration's themes_folder
    /// If the folder doesn't exist or cannot be read, returns the default theme collection
    pub fn load_from_config(config: &crate::configuration::Config) -> Self {
        if let Some(themes_dir) = &config.themes_folder {
            match Self::load_from_directory(themes_dir) {
                Ok(collection) if !collection.themes.is_empty() => return collection,
                Ok(_) => eprintln!("No themes found in directory: {}", themes_dir),
                Err(e) => eprintln!("Error loading themes: {}", e),
            }
        }
        // Return default theme collection if no themes could be loaded
        Self::default()
    }

    /// Set the color space for all themes
    pub fn set_color_space_for_all(&mut self, color_space: ThemeColorSpace) -> &mut Self {
        for theme in &mut self.themes {
            theme.color_space = color_space;
        }
        self
    }

    /// Save all themes to individual files in a directory
    pub fn save_to_directory<P: AsRef<Path>>(&self, dir_path: P) -> io::Result<()> {
        let dir_path = dir_path.as_ref();

        // Create directory if it doesn't exist
        if !dir_path.exists() {
            fs::create_dir_all(dir_path)?;
        }

        for theme in &self.themes {
            let file_name = format!("{}.json", theme.name.replace(" ", "_"));
            let file_path = dir_path.join(file_name);
            theme.save_to_file(&file_path)?;
        }

        // Save the collection metadata (current theme name)
        let collection_meta = serde_json::json!({
            "current_theme": self.current_theme
        });
        let meta_path = dir_path.join("_collection_meta.json");
        let mut file = fs::File::create(meta_path)?;
        file.write_all(
            serde_json::to_string_pretty(&collection_meta)
                .unwrap()
                .as_bytes(),
        )?;

        Ok(())
    }
}

// Helper function to convert a Color to RGB hex string
#[allow(unused)]
pub fn color_to_hex(color: &Color) -> String {
    let srgb = Srgb::from_color(color);
    let r = (srgb.r * 255.0).round() as u8;
    let g = (srgb.g * 255.0).round() as u8;
    let b = (srgb.b * 255.0).round() as u8;
    format!("#{:02X}{:02X}{:02X}", r, g, b)
}

#[allow(unused)]
pub fn rgb(r: u8, g: u8, b: u8) -> Color {
    Srgb {
        r: r as f64 / 255.0,
        g: g as f64 / 255.0,
        b: b as f64 / 255.0,
        a: 1.0,
    }
    .to_color()
}

#[allow(unused)]
pub fn rgba(r: u8, g: u8, b: u8, a: f64) -> Color {
    Srgb {
        r: r as f64 / 255.0,
        g: g as f64 / 255.0,
        b: b as f64 / 255.0,
        a,
    }
    .to_color()
}

#[allow(unused)]
pub fn hsl(h: f64, s: f64, l: f64) -> Color {
    Hsl { h, s, l, a: 1.0 }.to_color()
}

#[allow(unused)]
pub fn hsla(h: f64, s: f64, l: f64, a: f64) -> Color {
    Hsl { h, s, l, a }.to_color()
}

#[allow(unused)]
pub fn oklab(l: f64, a: f64, b: f64) -> Color {
    Oklab {
        l,
        a,
        b,
        alpha: 1.0,
    }
    .to_color()
}

#[allow(unused)]
pub fn oklaba(l: f64, a: f64, b: f64, alpha: f64) -> Color {
    Oklab { l, a, b, alpha }.to_color()
}

#[allow(unused)]
pub fn oklch(l: f64, c: f64, h: f64) -> Color {
    Oklch {
        l,
        c,
        h,
        alpha: 1.0,
    }
    .to_color()
}

#[allow(unused)]
pub fn oklcha(l: f64, c: f64, h: f64, alpha: f64) -> Color {
    Oklch { l, c, h, alpha }.to_color()
}
