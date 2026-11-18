//! Preferences that only the desktop reader has (typeface, text size, its
//! own theme choice). They live in `reader.toml` beside the terminal app's
//! `config.yaml`, which this module reads but never writes.

use bookokrat::settings::{YamlTheme, preferred_config_dir};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

const PREFS_FILE: &str = "reader.toml";
const TERMINAL_CONFIG_FILE: &str = "config.yaml";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ReaderPrefs {
    /// Unset until the user picks one here; the terminal app's theme is used meanwhile.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,
    pub font: String,
    pub font_size: f32,
    pub line_spacing: f32,
    pub measure: u32,
    pub justify: bool,
    pub drop_caps: bool,
    pub nav_width: u32,
    /// How the library is ordered: by `name`, most `recent` first, or by `type`.
    pub library_sort: String,
    /// PDF pages side by side instead of one under another.
    pub pdf_spread: bool,
    /// PDF zoom, as a multiple of the size pages are first shown at.
    pub pdf_zoom: f32,
    /// One PDF page (or pair) at a time instead of a continuous scroll.
    pub pdf_paged: bool,
    /// Whether the page keeps its margin and rounded corners in zen mode.
    pub zen_border: bool,
}

impl Default for ReaderPrefs {
    fn default() -> Self {
        Self {
            theme: None,
            font: "Literata".to_string(),
            font_size: 18.0,
            line_spacing: 1.7,
            measure: 68,
            justify: false,
            drop_caps: true,
            nav_width: 280,
            library_sort: "name".to_string(),
            pdf_spread: false,
            pdf_zoom: 1.0,
            pdf_paged: false,
            zen_border: false,
        }
    }
}

#[derive(Serialize)]
pub struct ThemeDto {
    pub name: String,
    pub colors: Vec<String>,
}

#[derive(Serialize)]
pub struct Appearance {
    pub prefs: ReaderPrefs,
    pub terminal_theme: Option<String>,
    pub custom_themes: Vec<ThemeDto>,
    /// Problems reading either file, worded for the status line.
    pub warnings: Vec<String>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct TerminalConfig {
    theme: Option<String>,
    custom_themes: Vec<YamlTheme>,
    synctex_editor: Option<String>,
}

/// The command that opens the TeX source at a line, from `config.yaml`.
pub fn synctex_editor() -> Option<String> {
    let path = config_file(TERMINAL_CONFIG_FILE).filter(|p| p.exists())?;
    fs::read_to_string(&path)
        .map_err(|e| e.to_string())
        .and_then(|text| serde_yaml::from_str::<TerminalConfig>(&text).map_err(|e| e.to_string()))
        .inspect_err(|e| log::error!("Failed to load {path:?}: {e}"))
        .ok()?
        .synctex_editor
}

fn config_file(name: &str) -> Option<PathBuf> {
    preferred_config_dir().map(|dir| dir.join(name))
}

pub fn load() -> Appearance {
    let mut warnings = Vec::new();

    let prefs = match config_file(PREFS_FILE).filter(|p| p.exists()) {
        Some(path) => fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|text| toml::from_str::<ReaderPrefs>(&text).map_err(|e| e.to_string()))
            .unwrap_or_else(|e| {
                log::error!("Failed to load {path:?}: {e}");
                warnings.push(format!("Could not read {PREFS_FILE}; using defaults"));
                ReaderPrefs::default()
            }),
        None => ReaderPrefs::default(),
    };

    let terminal = match config_file(TERMINAL_CONFIG_FILE).filter(|p| p.exists()) {
        Some(path) => fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|text| {
                serde_yaml::from_str::<TerminalConfig>(&text).map_err(|e| e.to_string())
            })
            .unwrap_or_else(|e| {
                log::error!("Failed to load {path:?}: {e}");
                warnings.push(format!(
                    "Could not read {TERMINAL_CONFIG_FILE}; custom themes are unavailable"
                ));
                TerminalConfig::default()
            }),
        None => TerminalConfig::default(),
    };

    Appearance {
        prefs,
        terminal_theme: terminal.theme,
        custom_themes: terminal.custom_themes.iter().map(theme_dto).collect(),
        warnings,
    }
}

pub fn save(prefs: &ReaderPrefs) -> Result<(), String> {
    let path = config_file(PREFS_FILE).ok_or("Could not determine the config directory")?;
    let text = toml::to_string_pretty(prefs).map_err(|e| e.to_string())?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("Failed to create {dir:?}: {e}"))?;
    }
    fs::write(&path, text).map_err(|e| format!("Failed to write {path:?}: {e}"))
}

fn theme_dto(theme: &YamlTheme) -> ThemeDto {
    let colors = [
        &theme.base00,
        &theme.base01,
        &theme.base02,
        &theme.base03,
        &theme.base04,
        &theme.base05,
        &theme.base06,
        &theme.base07,
        &theme.base08,
        &theme.base09,
        &theme.base0a,
        &theme.base0b,
        &theme.base0c,
        &theme.base0d,
        &theme.base0e,
        &theme.base0f,
    ];
    ThemeDto {
        name: theme.scheme.clone(),
        colors: colors
            .iter()
            .map(|c| format!("#{}", c.trim().trim_start_matches('#').to_lowercase()))
            .collect(),
    }
}

/// Families installed on this computer, for the "any installed font" picker.
pub fn system_fonts() -> Vec<String> {
    let mut db = fontdb::Database::new();
    db.load_system_fonts();
    let mut families: Vec<String> = db
        .faces()
        .filter_map(|face| face.families.first().map(|(name, _)| name.clone()))
        .filter(|name| !name.starts_with('.'))
        .collect();
    families.sort_by_key(|name| name.to_lowercase());
    families.dedup();
    families
}
