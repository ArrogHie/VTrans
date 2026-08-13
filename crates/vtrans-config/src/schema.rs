//! Application configuration schema.
//!
//! [`AppConfig`] is the root configuration structure. It is serialized to
//! and from JSON. Every field carries a `serde` default so that config
//! files with missing fields deserialize successfully with sensible values;
//! see [`crate::defaults`] for the canonical default values.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use vtrans_core::{Language, ScreenRegion};

/// The current config file format version.
///
/// Bump this constant when the schema changes and add a corresponding
/// migration step in [`crate::migration`].
pub const CURRENT_CONFIG_VERSION: u32 = 6;

/// Root application configuration.
///
/// All sub-structures default to sensible values when absent from the JSON
/// file, so a minimal config file only needs to override what differs.
///
/// # Example
///
/// ```
/// use vtrans_config::AppConfig;
///
/// let config = AppConfig::default();
/// let json = serde_json::to_string_pretty(&config).unwrap();
/// let back: AppConfig = serde_json::from_str(&json).unwrap();
/// assert_eq!(back.version, config.version);
/// assert_eq!(back.capture.interval_ms, config.capture.interval_ms);
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppConfig {
    /// Screen capture settings.
    #[serde(default = "CaptureConfig::default")]
    pub capture: CaptureConfig,

    /// OCR recognition settings.
    #[serde(default = "OcrConfig::default")]
    pub ocr: OcrConfig,

    /// Translation engine settings.
    #[serde(default = "TranslationConfig::default")]
    pub translation: TranslationConfig,

    /// Result window display settings.
    #[serde(default = "ResultWindowConfig::default")]
    pub result_window: ResultWindowConfig,

    /// Floating-ball (quick-launch bubble) settings.
    #[serde(default = "FloatingBallConfig::default")]
    pub floating_ball: FloatingBallConfig,

    /// Global hotkey bindings.
    #[serde(default = "HotkeyConfig::default")]
    pub hotkeys: HotkeyConfig,

    /// Log level filter (e.g. `"info"`, `"debug"`). Defaults to `"info"`.
    #[serde(default = "crate::defaults::default_log_level")]
    pub log_level: String,

    /// Override directory for model files; `None` means the default path.
    #[serde(default)]
    pub model_dir: Option<PathBuf>,

    /// Translation boxes for multi-box live translation mode.
    ///
    /// An empty list (the default) means multi-box mode is not configured.
    /// Each box carries its own screen region and display color and is
    /// persisted across restarts.
    #[serde(default = "crate::defaults::default_translation_boxes")]
    pub translation_boxes: Vec<TranslationBoxConfig>,

    /// Maximum number of concurrent translation boxes allowed. Defaults to
    /// `8`. Must be within `1..=32`.
    #[serde(default = "crate::defaults::default_max_boxes")]
    pub max_boxes: u32,

    /// Number of active boxes at which the UI should warn the user about
    /// potential performance impact. Defaults to `4`. Must be within
    /// `0..=max_boxes`; `0` disables the warning.
    #[serde(default = "crate::defaults::default_warning_threshold")]
    pub warning_threshold: u32,

    /// Config file format version, used by [`crate::migration`].
    #[serde(default = "crate::defaults::default_version")]
    pub version: u32,
}

/// Screen capture settings for live translation mode.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CaptureConfig {
    /// Capture interval in milliseconds. Must be within `250..=2000`.
    #[serde(default = "crate::defaults::default_interval_ms")]
    pub interval_ms: u32,

    /// Frame-difference threshold in `0.0..=1.0`; changes below this are
    /// ignored by the live pipeline.
    #[serde(default = "crate::defaults::default_difference_threshold")]
    pub difference_threshold: f32,
}

/// OCR recognition settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OcrConfig {
    /// Target language for recognition. Defaults to [`Language::Auto`].
    #[serde(default = "crate::defaults::default_language_auto")]
    pub language: Language,

    /// Minimum recognition confidence in `0.0..=1.0`; lines below this are
    /// discarded by the OCR provider.
    #[serde(default = "crate::defaults::default_min_confidence")]
    pub min_confidence: f32,
}

/// Translation engine settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TranslationConfig {
    /// Provider identifier: `"openai"`, `"deepl"`, `"google"`, `"azure"`,
    /// `"baidu"`, or `"local"`.
    #[serde(default = "crate::defaults::default_provider")]
    pub provider: String,

    /// Azure Translator region (e.g. `"eastasia"`), used only by the
    /// `"azure"` provider. Not sensitive; `None` means the provider omits
    /// the region header.
    #[serde(default = "crate::defaults::default_region")]
    pub region: Option<String>,

    /// Baidu Translate APP ID, used only by the `"baidu"` provider. Not
    /// sensitive (the matching Secret lives in the credential store, see
    /// `vtrans-security`); `None` means the provider cannot authenticate.
    #[serde(default = "crate::defaults::default_app_id")]
    pub app_id: Option<String>,

    /// Translation quality preset: `"fast"` or `"balanced"`. Defaults to
    /// `"fast"`. The value is consumed by the local translation provider
    /// (e.g. beam size); see `docs/modules/02-config.md`.
    #[serde(default = "crate::defaults::default_translation_quality")]
    pub quality: String,

    /// Source language; [`Language::Auto`] enables auto-detection.
    #[serde(default = "crate::defaults::default_source_language")]
    pub source_language: Language,

    /// Target language; must not be [`Language::Auto`].
    #[serde(default = "crate::defaults::default_target_language")]
    pub target_language: Language,

    /// Per-request timeout in seconds. Must be within `1..=3600`.
    #[serde(default = "crate::defaults::default_timeout_seconds")]
    pub timeout_seconds: u32,

    /// HTTP(S) endpoint used by the remote providers (`"openai"`,
    /// `"deepl"`, `"google"`, `"azure"`, `"baidu"`).
    #[serde(default = "crate::defaults::default_api_endpoint")]
    pub api_endpoint: String,

    /// Model identifier used by the `"openai"` provider; optional for
    /// `"deepl"` / `"google"` and ignored by `"azure"` / `"baidu"`.
    #[serde(default = "crate::defaults::default_api_model")]
    pub api_model: String,

    /// Number of retry attempts for transient API failures. Must be within
    /// `0..=10`.
    #[serde(default = "crate::defaults::default_max_retries")]
    pub max_retries: u32,
}

/// Result window display settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResultWindowConfig {
    /// Keep the result window above other windows.
    #[serde(default = "crate::defaults::default_always_on_top")]
    pub always_on_top: bool,

    /// Window opacity, `0.3..=1.0` (`1.0` = fully opaque).
    #[serde(default = "crate::defaults::default_opacity")]
    pub opacity: f64,

    /// Base font size for result text, in pixels. Must be within `12..=24`.
    #[serde(default = "crate::defaults::default_font_size_px")]
    pub font_size_px: u32,
}

/// Floating-ball (quick-launch bubble) settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FloatingBallConfig {
    /// Whether the floating ball is shown. Defaults to `false` (hidden).
    #[serde(default = "crate::defaults::default_floating_ball_enabled")]
    pub enabled: bool,

    /// Floating-ball opacity, `0.3..=1.0` (`1.0` = fully opaque).
    #[serde(default = "crate::defaults::default_floating_ball_opacity")]
    pub opacity: f64,

    /// Floating-ball diameter in pixels. Must be within `32..=72`.
    #[serde(default = "crate::defaults::default_floating_ball_size_px")]
    pub size_px: u32,
}

/// Global hotkey bindings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HotkeyConfig {
    /// Hotkey for manual region select and translate.
    #[serde(default = "crate::defaults::default_select_and_translate")]
    pub select_and_translate: String,

    /// Hotkey for starting live region translation.
    #[serde(default = "crate::defaults::default_live_translate")]
    pub live_translate: String,

    /// Hotkey for stopping live translation.
    #[serde(default = "crate::defaults::default_stop_live")]
    pub stop_live: String,
}

/// Default color palette for translation boxes (8 high-contrast colors).
///
/// When a new box is added, [`AppConfig::next_box_color`] assigns the first
/// unused color from this list. Once all colors are in use the palette
/// cycles from the beginning.
pub const BOX_COLOR_PALETTE: &[&str] = &[
    "#FF6B6B", // coral red
    "#4ECDC4", // teal
    "#45B7D1", // sky blue
    "#FFA07A", // light salmon
    "#98D8C8", // mint
    "#F7DC6F", // yellow
    "#BB8FCE", // purple
    "#85C1E9", // light blue
];

/// Configuration for a single translation box in multi-box live mode.
///
/// Each box has a unique `id`, a screen `region` to capture, and a display
/// `color` (hex string like `"#FF6B6B"`) used by the frontend to visually
/// distinguish boxes.
///
/// # Example
///
/// ```
/// use vtrans_config::TranslationBoxConfig;
/// use vtrans_core::ScreenRegion;
///
/// let box_config = TranslationBoxConfig::new(
///     0,
///     ScreenRegion::new("monitor0", 100, 200, 300, 400),
///     "#FF6B6B",
/// );
/// assert_eq!(box_config.id, 0);
/// assert_eq!(box_config.color, "#FF6B6B");
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranslationBoxConfig {
    /// Unique identifier for this translation box.
    pub id: u32,

    /// Screen region captured and translated for this box.
    pub region: ScreenRegion,

    /// Display color as a hex string (e.g. `"#FF6B6B"`).
    pub color: String,
}

impl TranslationBoxConfig {
    /// Creates a new translation box configuration.
    ///
    /// # Arguments
    ///
    /// * `id` - Unique identifier for the box.
    /// * `region` - Screen region to capture.
    /// * `color` - Hex color string (e.g. `"#FF6B6B"`).
    #[must_use]
    pub fn new(id: u32, region: ScreenRegion, color: impl Into<String>) -> Self {
        Self {
            id,
            region,
            color: color.into(),
        }
    }
}

// `ScreenRegion` in `vtrans-core` does not derive `PartialEq`, so a manual
// implementation is needed to support `AppConfig`'s derived `PartialEq`.
impl PartialEq for TranslationBoxConfig {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
            && self.color == other.color
            && self.region.monitor_id == other.region.monitor_id
            && self.region.x == other.region.x
            && self.region.y == other.region.y
            && self.region.width == other.region.width
            && self.region.height == other.region.height
    }
}

impl AppConfig {
    /// Returns the next available color from [`BOX_COLOR_PALETTE`] that is
    /// not already used by an existing translation box.
    ///
    /// When all palette colors are in use, the palette cycles from the
    /// beginning based on the current box count.
    ///
    /// # Example
    ///
    /// ```
    /// use vtrans_config::AppConfig;
    ///
    /// let config = AppConfig::default();
    /// assert_eq!(config.next_box_color(), "#FF6B6B");
    /// ```
    #[must_use]
    pub fn next_box_color(&self) -> &'static str {
        for &color in BOX_COLOR_PALETTE {
            if !self.translation_boxes.iter().any(|b| b.color == color) {
                return color;
            }
        }
        // All palette colors are in use — cycle from the beginning.
        let index = self.translation_boxes.len() % BOX_COLOR_PALETTE.len();
        BOX_COLOR_PALETTE[index]
    }

    /// Returns the next available box id (`max_id + 1`, or `0` if no boxes
    /// exist yet).
    ///
    /// # Example
    ///
    /// ```
    /// use vtrans_config::AppConfig;
    ///
    /// let config = AppConfig::default();
    /// assert_eq!(config.next_box_id(), 0);
    /// ```
    #[must_use]
    pub fn next_box_id(&self) -> u32 {
        self.translation_boxes
            .iter()
            .map(|b| b.id)
            .max()
            .map_or(0, |max_id| max_id + 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_config_serde_round_trip() {
        let config = AppConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let back: AppConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config, back);
    }

    #[test]
    fn missing_sections_are_filled_with_defaults() {
        let json = r#"{"version":6}"#;
        let config: AppConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config, AppConfig::default());
    }

    #[test]
    fn missing_version_defaults_to_current() {
        let json = "{}";
        let config: AppConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.version, CURRENT_CONFIG_VERSION);
    }

    #[test]
    fn unknown_fields_are_ignored() {
        let json = r#"{"version":6,"unknown_field":123}"#;
        let config: AppConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config, AppConfig::default());
    }

    #[test]
    fn partial_section_keeps_present_fields() {
        let json = r#"{"capture":{"interval_ms":1000},"version":6}"#;
        let config: AppConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.capture.interval_ms, 1000);
        assert!((config.capture.difference_threshold - 0.03).abs() < f32::EPSILON);
    }

    #[test]
    fn language_codes_round_trip() {
        let config = AppConfig {
            ocr: OcrConfig {
                language: Language::Japanese,
                ..Default::default()
            },
            ..Default::default()
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains(r#""language":"ja""#));
        let back: AppConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back.ocr.language, Language::Japanese);
    }

    #[test]
    fn invalid_language_code_is_rejected() {
        let json = r#"{"ocr":{"language":"klingon"},"version":5}"#;
        assert!(serde_json::from_str::<AppConfig>(json).is_err());
    }

    #[test]
    fn model_dir_round_trip() {
        let config = AppConfig {
            model_dir: Some(PathBuf::from(r"C:\models")),
            ..Default::default()
        };
        let json = serde_json::to_string(&config).unwrap();
        let back: AppConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back.model_dir, Some(PathBuf::from(r"C:\models")));
    }

    #[test]
    fn nested_defaults_apply_independently() {
        let json = r#"{
            "capture": {"difference_threshold": 0.5},
            "translation": {"target_language": "en"},
            "version": 6
        }"#;
        let config: AppConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.capture.interval_ms, 500);
        assert!((config.capture.difference_threshold - 0.5).abs() < f32::EPSILON);
        assert_eq!(config.translation.target_language, Language::English);
        assert_eq!(config.translation.quality, "fast");
        assert_eq!(config.translation.provider, "openai");
        assert_eq!(config.translation.region, None);
        assert_eq!(config.translation.app_id, None);
        assert_eq!(config.hotkeys.select_and_translate, "Alt+Shift+A");
    }

    #[test]
    fn translation_quality_defaults_to_fast() {
        let json = r#"{"version":6,"translation":{"provider":"local"}}"#;
        let config: AppConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.translation.quality, "fast");
    }

    #[test]
    fn translation_quality_round_trip() {
        let config = AppConfig {
            translation: TranslationConfig {
                quality: "balanced".to_string(),
                ..Default::default()
            },
            ..Default::default()
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains(r#""quality":"balanced""#));
        let back: AppConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back.translation.quality, "balanced");
    }

    #[test]
    fn region_and_app_id_default_to_none() {
        let json = r#"{"version":6}"#;
        let config: AppConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.translation.region, None);
        assert_eq!(config.translation.app_id, None);
    }

    #[test]
    fn region_and_app_id_round_trip() {
        let config = AppConfig {
            translation: TranslationConfig {
                region: Some("eastasia".to_string()),
                app_id: Some("2026081000000000".to_string()),
                ..Default::default()
            },
            ..Default::default()
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains(r#""region":"eastasia""#));
        assert!(json.contains(r#""app_id":"2026081000000000""#));
        let back: AppConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back.translation.region.as_deref(), Some("eastasia"));
        assert_eq!(back.translation.app_id.as_deref(), Some("2026081000000000"));
    }

    #[test]
    fn result_window_appearance_round_trip() {
        let config = AppConfig {
            result_window: ResultWindowConfig {
                always_on_top: false,
                opacity: 0.8,
                font_size_px: 18,
            },
            ..Default::default()
        };
        let json = serde_json::to_string(&config).unwrap();
        let back: AppConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back.result_window, config.result_window);
    }

    #[test]
    fn floating_ball_round_trip() {
        let config = AppConfig {
            floating_ball: FloatingBallConfig {
                enabled: true,
                opacity: 0.9,
                size_px: 56,
            },
            ..Default::default()
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains(r#""floating_ball":{"enabled":true,"opacity":0.9,"size_px":56}"#));
        let back: AppConfig = serde_json::from_str(&json).unwrap();
        assert!(back.floating_ball.enabled);
        assert!((back.floating_ball.opacity - 0.9).abs() < f64::EPSILON);
        assert_eq!(back.floating_ball.size_px, 56);
    }

    #[test]
    fn missing_result_window_fields_are_filled_with_defaults() {
        let json = r#"{"result_window":{"always_on_top":false},"version":6}"#;
        let config: AppConfig = serde_json::from_str(json).unwrap();
        assert!(!config.result_window.always_on_top);
        assert!((config.result_window.opacity - 0.95).abs() < f64::EPSILON);
        assert_eq!(config.result_window.font_size_px, 14);
        assert!(!config.floating_ball.enabled);
    }

    #[test]
    fn missing_floating_ball_fields_are_filled_with_defaults() {
        let json = r#"{"floating_ball":{"enabled":true},"version":6}"#;
        let config: AppConfig = serde_json::from_str(json).unwrap();
        assert!(config.floating_ball.enabled);
        assert!((config.floating_ball.opacity - 1.0).abs() < f64::EPSILON);
        assert_eq!(config.floating_ball.size_px, 48);
    }

    // ── Multi-box config ──

    #[test]
    fn translation_box_config_serde_round_trip() {
        let region = ScreenRegion::new("monitor0", 10, 20, 300, 400);
        let box_config = TranslationBoxConfig::new(0, region.clone(), "#FF6B6B");
        let json = serde_json::to_string(&box_config).unwrap();
        let back: TranslationBoxConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back, box_config);
        assert_eq!(back.region.monitor_id, region.monitor_id);
        assert_eq!(back.region.width, region.width);
    }

    #[test]
    fn app_config_with_translation_boxes_round_trip() {
        let region = ScreenRegion::new("m0", 100, 200, 640, 480);
        let config = AppConfig {
            translation_boxes: vec![TranslationBoxConfig::new(0, region, "#FF6B6B")],
            max_boxes: 16,
            warning_threshold: 8,
            ..Default::default()
        };
        let json = serde_json::to_string(&config).unwrap();
        let back: AppConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back, config);
        assert_eq!(back.max_boxes, 16);
        assert_eq!(back.warning_threshold, 8);
        assert_eq!(back.translation_boxes.len(), 1);
        assert_eq!(back.translation_boxes[0].id, 0);
        assert_eq!(back.translation_boxes[0].color, "#FF6B6B");
    }

    #[test]
    fn missing_translation_boxes_defaults_to_empty() {
        let json = r#"{"version":6}"#;
        let config: AppConfig = serde_json::from_str(json).unwrap();
        assert!(config.translation_boxes.is_empty());
        assert_eq!(config.max_boxes, 8);
        assert_eq!(config.warning_threshold, 4);
    }

    #[test]
    fn box_color_palette_has_at_least_eight_colors() {
        assert!(BOX_COLOR_PALETTE.len() >= 8);
        // All palette entries must be distinct.
        let mut sorted = BOX_COLOR_PALETTE.to_vec();
        sorted.sort_unstable();
        let before = sorted.len();
        sorted.dedup();
        assert_eq!(sorted.len(), before, "palette has duplicate colors");
    }

    #[test]
    fn next_box_color_returns_first_unused() {
        let config = AppConfig::default();
        assert_eq!(config.next_box_color(), BOX_COLOR_PALETTE[0]);

        // With the first color in use, the next should be the second.
        let config = AppConfig {
            translation_boxes: vec![TranslationBoxConfig::new(
                0,
                ScreenRegion::new("m", 0, 0, 100, 100),
                BOX_COLOR_PALETTE[0],
            )],
            ..Default::default()
        };
        assert_eq!(config.next_box_color(), BOX_COLOR_PALETTE[1]);
    }

    #[test]
    fn next_box_color_cycles_after_palette_exhaustion() {
        let boxes: Vec<TranslationBoxConfig> = (0..BOX_COLOR_PALETTE.len())
            .map(|i| {
                TranslationBoxConfig::new(
                    u32::try_from(i).unwrap(),
                    ScreenRegion::new("m", 0, 0, 100, 100),
                    BOX_COLOR_PALETTE[i],
                )
            })
            .collect();
        let config = AppConfig {
            translation_boxes: boxes,
            ..Default::default()
        };
        // All colors in use → cycle back to palette[0].
        assert_eq!(config.next_box_color(), BOX_COLOR_PALETTE[0]);

        // With one more box than palette size, cycle continues to palette[1].
        let mut boxes = config.translation_boxes.clone();
        boxes.push(TranslationBoxConfig::new(
            99,
            ScreenRegion::new("m", 0, 0, 100, 100),
            config.next_box_color(),
        ));
        let config = AppConfig {
            translation_boxes: boxes,
            ..Default::default()
        };
        assert_eq!(config.next_box_color(), BOX_COLOR_PALETTE[1]);
    }

    #[test]
    fn next_box_id_returns_zero_when_empty() {
        let config = AppConfig::default();
        assert_eq!(config.next_box_id(), 0);
    }

    #[test]
    fn next_box_id_returns_max_plus_one() {
        let config = AppConfig {
            translation_boxes: vec![
                TranslationBoxConfig::new(2, ScreenRegion::new("m", 0, 0, 10, 10), "#FF6B6B"),
                TranslationBoxConfig::new(5, ScreenRegion::new("m", 0, 0, 10, 10), "#4ECDC4"),
                TranslationBoxConfig::new(1, ScreenRegion::new("m", 0, 0, 10, 10), "#45B7D1"),
            ],
            ..Default::default()
        };
        assert_eq!(config.next_box_id(), 6);
    }

    #[test]
    fn translation_box_config_partial_eq() {
        let r1 = ScreenRegion::new("m", 1, 2, 3, 4);
        let r2 = ScreenRegion::new("m", 1, 2, 3, 4);
        let r3 = ScreenRegion::new("m", 1, 2, 3, 5);

        let original = TranslationBoxConfig::new(0, r1.clone(), "#FF6B6B");
        let identical = TranslationBoxConfig::new(0, r2.clone(), "#FF6B6B");
        let different_id = TranslationBoxConfig::new(1, r1, "#FF6B6B");
        let different_region = TranslationBoxConfig::new(0, r3, "#FF6B6B");
        let different_color = TranslationBoxConfig::new(0, r2, "#4ECDC4");

        assert_eq!(original, identical);
        assert_ne!(original, different_id);
        assert_ne!(original, different_region);
        assert_ne!(original, different_color);
    }
}
