use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// Main config structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub style: StyleConfig,
    pub segments: Vec<SegmentConfig>,
    pub theme: String,
    /// Top-level statusline behavior — global knobs that don't belong on any
    /// single segment. Defaults via `#[serde(default)]` for back-compat with
    /// pre-T07 config files.
    #[serde(default)]
    pub statusline: StatuslineConfig,
}

/// Top-level statusline configuration (T07).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StatuslineConfig {
    /// When true, ccline exits with code 1 on `warn` level or 2 on `critical`
    /// level after rendering. The status line still prints normally — the
    /// exit code is just a side-channel signal that Claude Code hooks may
    /// interpret. Default false (most users should leave this off).
    #[serde(default)]
    pub exit_code_on_threshold: bool,
}

// Default implementation moved to ui/themes/presets.rs

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StyleConfig {
    pub mode: StyleMode,
    pub separator: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StyleMode {
    Plain,
    NerdFont,
    Powerline,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SegmentConfig {
    pub id: SegmentId,
    pub enabled: bool,
    pub icon: IconConfig,
    pub colors: ColorConfig,
    pub styles: TextStyleConfig,
    pub options: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IconConfig {
    pub plain: String,
    pub nerd_font: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColorConfig {
    pub icon: Option<AnsiColor>,
    pub text: Option<AnsiColor>,
    pub background: Option<AnsiColor>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TextStyleConfig {
    pub text_bold: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AnsiColor {
    Color16 { c16: u8 },
    Color256 { c256: u8 },
    Rgb { r: u8, g: u8, b: u8 },
}

/// Color threshold for percent-bearing segments.
///
/// Stored inside `SegmentConfig.options` under the `"thresholds"` key as an
/// array of `{at, color}` objects. The renderer picks the highest `at` that
/// is `<= percent` (per `Threshold::pick`) and uses its color to override the
/// segment's primary text color.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Threshold {
    /// Percent threshold (0..=100). Inclusive lower bound.
    pub at: u8,
    /// Color applied to the segment's primary text when the percent reaches
    /// `at` and no higher threshold matches.
    pub color: AnsiColor,
}

impl Threshold {
    /// Pull `Vec<Threshold>` out of a segment's `options` map. Returns empty
    /// when the key is absent or malformed — color thresholds are an opt-in
    /// feature and bad config should be ignored, not propagated as errors.
    pub fn list_from_options(options: &HashMap<String, serde_json::Value>) -> Vec<Threshold> {
        options
            .get("thresholds")
            .and_then(|v| serde_json::from_value::<Vec<Threshold>>(v.clone()).ok())
            .unwrap_or_default()
    }

    /// Pick the threshold whose `at` is the highest value `<= percent`.
    /// Returns `None` when `percent` is below every threshold's `at`,
    /// signalling "no color override".
    pub fn pick(thresholds: &[Threshold], percent: f64) -> Option<&Threshold> {
        thresholds
            .iter()
            .filter(|t| (t.at as f64) <= percent)
            .max_by_key(|t| t.at)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SegmentId {
    Model,
    Directory,
    Git,
    ContextWindow,
    Usage,
    WeeklyUsage,
    BurnRate,
    ProjectedExhaust,
    Cost,
    Session,
    OutputStyle,
    Update,
}

// Legacy compatibility structure
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SegmentsConfig {
    pub directory: bool,
    pub git: bool,
    pub model: bool,
    // pub usage: bool,
}

// Data structures compatible with existing main.rs
#[derive(Deserialize)]
pub struct Model {
    pub id: String,
    pub display_name: String,
}

#[derive(Deserialize)]
pub struct Workspace {
    pub current_dir: String,
}

#[derive(Deserialize)]
pub struct Cost {
    pub total_cost_usd: Option<f64>,
    pub total_duration_ms: Option<u64>,
    pub total_api_duration_ms: Option<u64>,
    pub total_lines_added: Option<u32>,
    pub total_lines_removed: Option<u32>,
}

#[derive(Deserialize)]
pub struct OutputStyle {
    pub name: String,
}

#[derive(Deserialize)]
pub struct InputData {
    pub model: Model,
    pub workspace: Workspace,
    pub transcript_path: String,
    pub cost: Option<Cost>,
    pub output_style: Option<OutputStyle>,
}

// OpenAI-style nested token details
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct PromptTokensDetails {
    #[serde(default)]
    pub cached_tokens: Option<u32>,
    #[serde(default)]
    pub audio_tokens: Option<u32>,
}

// Raw usage data from different LLM providers (flexible parsing)
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct RawUsage {
    // Anthropic-style input tokens
    #[serde(default)]
    pub input_tokens: Option<u32>,

    // OpenAI-style input tokens (separate field to handle both formats)
    #[serde(default)]
    pub prompt_tokens: Option<u32>,

    // Anthropic-style output tokens
    #[serde(default)]
    pub output_tokens: Option<u32>,

    // OpenAI-style output tokens (separate field to handle both formats)
    #[serde(default)]
    pub completion_tokens: Option<u32>,

    // Total tokens (some providers only provide this)
    #[serde(default)]
    pub total_tokens: Option<u32>,

    // Anthropic-style cache fields
    #[serde(default)]
    pub cache_creation_input_tokens: Option<u32>,

    #[serde(default)]
    pub cache_read_input_tokens: Option<u32>,

    // OpenAI-style cache fields (separate fields to handle both formats)
    #[serde(default)]
    pub cache_creation_prompt_tokens: Option<u32>,

    #[serde(default)]
    pub cache_read_prompt_tokens: Option<u32>,

    #[serde(default)]
    pub cached_tokens: Option<u32>,

    // OpenAI-style nested details
    #[serde(default)]
    pub prompt_tokens_details: Option<PromptTokensDetails>,

    // Completion token details (OpenAI)
    #[serde(default)]
    pub completion_tokens_details: Option<HashMap<String, u32>>,

    // Catch unknown fields for future compatibility and debugging
    #[serde(flatten, skip_serializing)]
    pub extra: HashMap<String, serde_json::Value>,
}

// Normalized internal representation after processing
#[derive(Debug, Clone, Serialize, Default, PartialEq)]
pub struct NormalizedUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub total_tokens: u32,
    pub cache_creation_input_tokens: u32,
    pub cache_read_input_tokens: u32,

    // Metadata for debugging and analysis
    pub calculation_source: String,
    pub raw_data_available: Vec<String>,
}

impl NormalizedUsage {
    /// Get tokens that count toward context window
    /// This includes all tokens that consume context window space
    /// Output tokens from this turn will become input tokens in the next turn
    pub fn context_tokens(&self) -> u32 {
        self.input_tokens
            + self.cache_creation_input_tokens
            + self.cache_read_input_tokens
            + self.output_tokens
    }

    /// Get total tokens for cost calculation
    /// Priority: use total_tokens if available, otherwise sum all components
    pub fn total_for_cost(&self) -> u32 {
        if self.total_tokens > 0 {
            self.total_tokens
        } else {
            self.input_tokens
                + self.output_tokens
                + self.cache_creation_input_tokens
                + self.cache_read_input_tokens
        }
    }

    /// Get the most appropriate token count for general display
    /// For OpenAI format: use total_tokens directly
    /// For Anthropic format: use context_tokens (input + cache)
    pub fn display_tokens(&self) -> u32 {
        // For Claude/Anthropic format: prefer input-related tokens for context window display
        let context = self.context_tokens();
        if context > 0 {
            return context;
        }

        // For OpenAI format: use total_tokens when no input breakdown available
        if self.total_tokens > 0 {
            return self.total_tokens;
        }

        // Fallback to any available tokens
        self.input_tokens.max(self.output_tokens)
    }
}

impl Config {
    /// Check if current config matches the specified theme preset
    pub fn matches_theme(&self, theme_name: &str) -> bool {
        let theme_preset = crate::ui::themes::ThemePresets::get_theme(theme_name);

        // Compare style config
        if self.style.mode != theme_preset.style.mode
            || self.style.separator != theme_preset.style.separator
        {
            return false;
        }

        // Compare segments count and order
        if self.segments.len() != theme_preset.segments.len() {
            return false;
        }

        // Compare each segment config
        for (current, preset) in self.segments.iter().zip(theme_preset.segments.iter()) {
            if !self.segment_matches(current, preset) {
                return false;
            }
        }

        true
    }

    /// Check if current config has been modified from the selected theme
    pub fn is_modified_from_theme(&self) -> bool {
        !self.matches_theme(&self.theme)
    }

    /// Compare two segment configs for equality
    fn segment_matches(&self, current: &SegmentConfig, preset: &SegmentConfig) -> bool {
        current.id == preset.id
            && current.enabled == preset.enabled
            && current.icon.plain == preset.icon.plain
            && current.icon.nerd_font == preset.icon.nerd_font
            && self.color_matches(&current.colors.icon, &preset.colors.icon)
            && self.color_matches(&current.colors.text, &preset.colors.text)
            && self.color_matches(&current.colors.background, &preset.colors.background)
            && current.styles.text_bold == preset.styles.text_bold
            && current.options == preset.options
    }

    /// Compare two optional colors for equality
    fn color_matches(&self, current: &Option<AnsiColor>, preset: &Option<AnsiColor>) -> bool {
        match (current, preset) {
            (None, None) => true,
            (Some(c1), Some(c2)) => c1 == c2,
            _ => false,
        }
    }
}

impl PartialEq for AnsiColor {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (AnsiColor::Color16 { c16: a }, AnsiColor::Color16 { c16: b }) => a == b,
            (AnsiColor::Color256 { c256: a }, AnsiColor::Color256 { c256: b }) => a == b,
            (
                AnsiColor::Rgb {
                    r: r1,
                    g: g1,
                    b: b1,
                },
                AnsiColor::Rgb {
                    r: r2,
                    g: g2,
                    b: b2,
                },
            ) => r1 == r2 && g1 == g2 && b1 == b2,
            _ => false,
        }
    }
}

impl RawUsage {
    /// Convert raw usage data to normalized format with intelligent token inference
    pub fn normalize(self) -> NormalizedUsage {
        let mut result = NormalizedUsage::default();
        let mut sources = Vec::new();

        // Collect available raw data fields and merge tokens with Anthropic priority
        let mut available_fields = Vec::new();

        // Merge input tokens (priority: input_tokens > prompt_tokens)
        let input = self.input_tokens.or(self.prompt_tokens).unwrap_or(0);
        if input > 0 {
            available_fields.push("input_tokens".to_string());
        }

        // Merge output tokens (priority: output_tokens > completion_tokens)
        let output = self.output_tokens.or(self.completion_tokens).unwrap_or(0);
        if output > 0 {
            available_fields.push("output_tokens".to_string());
        }

        let total = self.total_tokens.unwrap_or(0);
        if total > 0 {
            available_fields.push("total_tokens".to_string());
        }

        // Merge cache creation tokens (priority: Anthropic > OpenAI)
        let cache_creation = self
            .cache_creation_input_tokens
            .or(self.cache_creation_prompt_tokens)
            .unwrap_or(0);
        if cache_creation > 0 {
            available_fields.push("cache_creation".to_string());
        }

        // Merge cache read tokens (priority: Anthropic > OpenAI > nested format)
        let cache_read = self
            .cache_read_input_tokens
            .or(self.cache_read_prompt_tokens)
            .or(self.cached_tokens)
            .or_else(|| {
                // Fallback to OpenAI nested format
                self.prompt_tokens_details
                    .as_ref()
                    .and_then(|d| d.cached_tokens)
            })
            .unwrap_or(0);
        if cache_read > 0 {
            available_fields.push("cache_read".to_string());
        }

        result.raw_data_available = available_fields;

        // Use merged cache values (already calculated above with Anthropic priority)

        // Token calculation logic - prioritize total_tokens for OpenAI format
        let total_value = if total > 0 {
            sources.push("total_tokens_direct".to_string());
            total
        } else if input > 0 || output > 0 || cache_read > 0 || cache_creation > 0 {
            let calculated = input + output + cache_read + cache_creation;
            sources.push("total_from_components".to_string());
            calculated
        } else {
            0
        };

        // Assignment
        result.input_tokens = input;
        result.output_tokens = output;
        result.total_tokens = total_value;
        result.cache_creation_input_tokens = cache_creation;
        result.cache_read_input_tokens = cache_read;
        result.calculation_source = sources.join("+");

        result
    }
}

// Legacy alias for backward compatibility
pub type Usage = RawUsage;

#[derive(Deserialize)]
pub struct Message {
    pub usage: Option<Usage>,
}

#[derive(Deserialize)]
pub struct TranscriptEntry {
    pub r#type: Option<String>,
    pub message: Option<Message>,
    #[serde(rename = "leafUuid")]
    pub leaf_uuid: Option<String>,
    pub uuid: Option<String>,
    #[serde(rename = "parentUuid")]
    pub parent_uuid: Option<String>,
    pub summary: Option<String>,
    /// RFC3339 wall-clock timestamp Claude Code records for each turn.
    /// Optional because pre-T04 transcripts and synthetic entries may not
    /// carry it.
    #[serde(default)]
    pub timestamp: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------- RawUsage::normalize ----------

    #[test]
    fn normalize_empty_returns_zeros() {
        let n = RawUsage::default().normalize();
        assert_eq!(n.input_tokens, 0);
        assert_eq!(n.output_tokens, 0);
        assert_eq!(n.total_tokens, 0);
        assert_eq!(n.cache_creation_input_tokens, 0);
        assert_eq!(n.cache_read_input_tokens, 0);
        assert_eq!(n.context_tokens(), 0);
        assert_eq!(n.total_for_cost(), 0);
        assert_eq!(n.display_tokens(), 0);
    }

    #[test]
    fn normalize_anthropic_only() {
        let raw = RawUsage {
            input_tokens: Some(100),
            output_tokens: Some(50),
            cache_creation_input_tokens: Some(30),
            cache_read_input_tokens: Some(20),
            ..Default::default()
        };
        let n = raw.normalize();
        assert_eq!(n.input_tokens, 100);
        assert_eq!(n.output_tokens, 50);
        assert_eq!(n.cache_creation_input_tokens, 30);
        assert_eq!(n.cache_read_input_tokens, 20);
        assert_eq!(n.context_tokens(), 200);
        assert_eq!(n.total_for_cost(), 200);
        assert!(n.calculation_source.contains("total_from_components"));
    }

    #[test]
    fn normalize_openai_total_priority() {
        let raw = RawUsage {
            prompt_tokens: Some(100),
            completion_tokens: Some(50),
            total_tokens: Some(999),
            ..Default::default()
        };
        let n = raw.normalize();
        assert_eq!(n.input_tokens, 100);
        assert_eq!(n.output_tokens, 50);
        // When total is provided directly, it is preferred over the sum.
        assert_eq!(n.total_tokens, 999);
        assert!(n.calculation_source.contains("total_tokens_direct"));
        assert_eq!(n.total_for_cost(), 999);
    }

    #[test]
    fn normalize_anthropic_priority_over_openai() {
        let raw = RawUsage {
            input_tokens: Some(100),      // Anthropic
            prompt_tokens: Some(999),     // OpenAI — should be ignored
            output_tokens: Some(50),      // Anthropic
            completion_tokens: Some(888), // OpenAI — should be ignored
            cache_creation_input_tokens: Some(30),
            cache_creation_prompt_tokens: Some(777),
            cache_read_input_tokens: Some(20),
            cache_read_prompt_tokens: Some(666),
            ..Default::default()
        };
        let n = raw.normalize();
        assert_eq!(n.input_tokens, 100);
        assert_eq!(n.output_tokens, 50);
        assert_eq!(n.cache_creation_input_tokens, 30);
        assert_eq!(n.cache_read_input_tokens, 20);
    }

    #[test]
    fn normalize_cached_tokens_fallback_chain() {
        // Only the deepest fallback — nested prompt_tokens_details.cached_tokens
        let raw = RawUsage {
            prompt_tokens: Some(100),
            completion_tokens: Some(50),
            prompt_tokens_details: Some(PromptTokensDetails {
                cached_tokens: Some(42),
                audio_tokens: None,
            }),
            ..Default::default()
        };
        let n = raw.normalize();
        assert_eq!(n.cache_read_input_tokens, 42);
    }

    #[test]
    fn normalize_cached_tokens_flat_field_wins_over_nested() {
        let raw = RawUsage {
            cached_tokens: Some(100),
            prompt_tokens_details: Some(PromptTokensDetails {
                cached_tokens: Some(42),
                audio_tokens: None,
            }),
            ..Default::default()
        };
        let n = raw.normalize();
        // cached_tokens (flat) should beat the nested path.
        assert_eq!(n.cache_read_input_tokens, 100);
    }

    #[test]
    fn normalize_records_available_fields() {
        let raw = RawUsage {
            input_tokens: Some(10),
            output_tokens: Some(5),
            ..Default::default()
        };
        let n = raw.normalize();
        assert!(n.raw_data_available.contains(&"input_tokens".to_string()));
        assert!(n.raw_data_available.contains(&"output_tokens".to_string()));
    }

    // ---------- Config::matches_theme ----------

    #[test]
    fn default_config_segments_match_default_preset_builtin() {
        // Compares the structure of Config::default() directly against the
        // built-in `get_default()`. Bypasses `matches_theme()` because that
        // method loads `~/.claude/ccline/themes/default.toml` first, which can
        // be stale on user machines that bootstrapped theme files before new
        // segments (like WeeklyUsage) were added.
        let cfg = Config::default();
        let built_in = crate::ui::themes::ThemePresets::get_default();

        assert_eq!(cfg.theme, "default");
        assert_eq!(cfg.theme, built_in.theme);
        assert_eq!(cfg.style.mode, built_in.style.mode);
        assert_eq!(cfg.style.separator, built_in.style.separator);

        let cfg_ids: Vec<_> = cfg.segments.iter().map(|s| s.id).collect();
        let built_in_ids: Vec<_> = built_in.segments.iter().map(|s| s.id).collect();
        assert_eq!(cfg_ids, built_in_ids);
    }

    // ---------- Threshold (T03) ----------

    #[test]
    fn threshold_list_from_options_reads_array() {
        let json = serde_json::json!({
            "thresholds": [
                { "at": 60, "color": { "c16": 3 } },
                { "at": 85, "color": { "r": 220, "g": 60, "b": 60 } }
            ]
        });
        let opts: HashMap<String, serde_json::Value> = serde_json::from_value(json).unwrap();
        let thresholds = Threshold::list_from_options(&opts);
        assert_eq!(thresholds.len(), 2);
        assert_eq!(thresholds[0].at, 60);
        assert_eq!(thresholds[1].at, 85);
        assert!(matches!(thresholds[0].color, AnsiColor::Color16 { c16: 3 }));
        assert!(matches!(
            thresholds[1].color,
            AnsiColor::Rgb {
                r: 220,
                g: 60,
                b: 60
            }
        ));
    }

    #[test]
    fn threshold_list_from_options_missing_returns_empty() {
        let empty: HashMap<String, serde_json::Value> = HashMap::new();
        assert!(Threshold::list_from_options(&empty).is_empty());
    }

    #[test]
    fn threshold_list_from_options_garbage_returns_empty() {
        let mut bogus: HashMap<String, serde_json::Value> = HashMap::new();
        bogus.insert("thresholds".to_string(), serde_json::json!("not an array"));
        assert!(Threshold::list_from_options(&bogus).is_empty());
    }

    #[test]
    fn threshold_pick_below_all_returns_none() {
        let ts = vec![
            Threshold {
                at: 60,
                color: AnsiColor::Color16 { c16: 3 },
            },
            Threshold {
                at: 85,
                color: AnsiColor::Color16 { c16: 1 },
            },
        ];
        assert!(Threshold::pick(&ts, 30.0).is_none());
        assert!(Threshold::pick(&ts, 59.99).is_none());
    }

    #[test]
    fn threshold_pick_returns_highest_at_or_below() {
        let ts = vec![
            Threshold {
                at: 60,
                color: AnsiColor::Color16 { c16: 3 },
            },
            Threshold {
                at: 85,
                color: AnsiColor::Color16 { c16: 1 },
            },
        ];
        assert_eq!(Threshold::pick(&ts, 60.0).map(|t| t.at), Some(60));
        assert_eq!(Threshold::pick(&ts, 70.0).map(|t| t.at), Some(60));
        assert_eq!(Threshold::pick(&ts, 85.0).map(|t| t.at), Some(85));
        assert_eq!(Threshold::pick(&ts, 99.9).map(|t| t.at), Some(85));
    }

    #[test]
    fn every_builtin_preset_ships_with_default_thresholds_on_usage() {
        use crate::ui::themes::ThemePresets;
        let configs = [
            ("cometix", ThemePresets::get_cometix()),
            ("default", ThemePresets::get_default()),
            ("minimal", ThemePresets::get_minimal()),
            ("gruvbox", ThemePresets::get_gruvbox()),
            ("nord", ThemePresets::get_nord()),
            ("powerline-dark", ThemePresets::get_powerline_dark()),
            ("powerline-light", ThemePresets::get_powerline_light()),
            (
                "powerline-rose-pine",
                ThemePresets::get_powerline_rose_pine(),
            ),
            (
                "powerline-tokyo-night",
                ThemePresets::get_powerline_tokyo_night(),
            ),
        ];
        for (name, cfg) in &configs {
            let usage = cfg
                .segments
                .iter()
                .find(|s| s.id == SegmentId::Usage)
                .expect("usage segment missing");
            let ts = Threshold::list_from_options(&usage.options);
            assert_eq!(ts.len(), 2, "theme {} should ship 60/85 thresholds", name);
            let ats: Vec<u8> = ts.iter().map(|t| t.at).collect();
            assert!(
                ats.contains(&60) && ats.contains(&85),
                "theme {} thresholds not 60/85: {:?}",
                name,
                ats
            );
        }
    }

    #[test]
    fn weekly_usage_inherits_thresholds_from_usage_via_wrapper() {
        use crate::ui::themes::ThemePresets;
        // WeeklyUsage segments are built as `{ let mut s = usage_segment(); ... }`
        // so they share the options HashMap and inherit thresholds for free.
        let cfg = ThemePresets::get_default();
        let weekly = cfg
            .segments
            .iter()
            .find(|s| s.id == SegmentId::WeeklyUsage)
            .expect("WeeklyUsage missing");
        let ts = Threshold::list_from_options(&weekly.options);
        assert_eq!(
            ts.len(),
            2,
            "weekly_usage didn't inherit thresholds: options keys = {:?}",
            weekly.options.keys().collect::<Vec<_>>()
        );
    }

    #[test]
    fn threshold_pick_unsorted_input_still_returns_max() {
        // The helper must tolerate user-provided thresholds in any order.
        let ts = vec![
            Threshold {
                at: 85,
                color: AnsiColor::Color16 { c16: 1 },
            },
            Threshold {
                at: 60,
                color: AnsiColor::Color16 { c16: 3 },
            },
        ];
        assert_eq!(Threshold::pick(&ts, 90.0).map(|t| t.at), Some(85));
    }

    #[test]
    fn cometix_preset_structurally_differs_from_default() {
        // Direct structural comparison instead of going through matches_theme()
        // (which would hit on-disk theme cache).
        let default_cfg = crate::ui::themes::ThemePresets::get_default();
        let cometix_cfg = crate::ui::themes::ThemePresets::get_cometix();
        assert_ne!(default_cfg.style.mode, cometix_cfg.style.mode);
    }

    #[test]
    fn config_with_dropped_segment_does_not_match() {
        let mut cfg = Config::default();
        cfg.segments.pop();
        assert!(!cfg.matches_theme("default"));
    }

    #[test]
    fn config_with_mutated_style_mode_does_not_match() {
        let mut cfg = Config::default();
        cfg.style.mode = StyleMode::Powerline;
        assert!(!cfg.matches_theme("default"));
        assert!(cfg.is_modified_from_theme());
    }

    #[test]
    fn config_with_mutated_separator_does_not_match() {
        let mut cfg = Config::default();
        cfg.style.separator = "###".to_string();
        assert!(!cfg.matches_theme("default"));
    }

    // ---------- SegmentId::WeeklyUsage (T02) ----------

    #[test]
    fn segment_id_weekly_usage_deserializes_from_snake_case() {
        let id: SegmentId =
            serde_json::from_str(r#""weekly_usage""#).expect("snake_case form must deserialize");
        assert!(matches!(id, SegmentId::WeeklyUsage));
    }

    #[test]
    fn segment_id_weekly_usage_serializes_to_snake_case() {
        let s = serde_json::to_string(&SegmentId::WeeklyUsage).expect("serialize");
        assert_eq!(s, r#""weekly_usage""#);
    }

    #[test]
    fn every_builtin_preset_includes_weekly_usage_segment() {
        use crate::ui::themes::ThemePresets;
        // Call the per-theme builders directly to bypass the on-disk theme
        // cache, which may carry pre-WeeklyUsage configs from older versions.
        let configs = [
            ("cometix", ThemePresets::get_cometix()),
            ("default", ThemePresets::get_default()),
            ("minimal", ThemePresets::get_minimal()),
            ("gruvbox", ThemePresets::get_gruvbox()),
            ("nord", ThemePresets::get_nord()),
            ("powerline-dark", ThemePresets::get_powerline_dark()),
            ("powerline-light", ThemePresets::get_powerline_light()),
            (
                "powerline-rose-pine",
                ThemePresets::get_powerline_rose_pine(),
            ),
            (
                "powerline-tokyo-night",
                ThemePresets::get_powerline_tokyo_night(),
            ),
        ];
        for (name, cfg) in &configs {
            assert!(
                cfg.segments.iter().any(|s| s.id == SegmentId::WeeklyUsage),
                "built-in theme {} is missing the WeeklyUsage segment",
                name
            );
        }
    }

    #[test]
    fn every_builtin_preset_includes_projected_exhaust_segment() {
        use crate::ui::themes::ThemePresets;
        let configs = [
            ThemePresets::get_cometix(),
            ThemePresets::get_default(),
            ThemePresets::get_minimal(),
            ThemePresets::get_gruvbox(),
            ThemePresets::get_nord(),
            ThemePresets::get_powerline_dark(),
            ThemePresets::get_powerline_light(),
            ThemePresets::get_powerline_rose_pine(),
            ThemePresets::get_powerline_tokyo_night(),
        ];
        for cfg in &configs {
            let seg = cfg
                .segments
                .iter()
                .find(|s| s.id == SegmentId::ProjectedExhaust)
                .unwrap_or_else(|| panic!("theme {} missing ProjectedExhaust", cfg.theme));
            assert!(
                !seg.enabled,
                "theme {} has ProjectedExhaust enabled",
                cfg.theme
            );
        }
    }

    #[test]
    fn every_builtin_preset_includes_burn_rate_segment() {
        use crate::ui::themes::ThemePresets;
        let configs = [
            ("cometix", ThemePresets::get_cometix()),
            ("default", ThemePresets::get_default()),
            ("minimal", ThemePresets::get_minimal()),
            ("gruvbox", ThemePresets::get_gruvbox()),
            ("nord", ThemePresets::get_nord()),
            ("powerline-dark", ThemePresets::get_powerline_dark()),
            ("powerline-light", ThemePresets::get_powerline_light()),
            (
                "powerline-rose-pine",
                ThemePresets::get_powerline_rose_pine(),
            ),
            (
                "powerline-tokyo-night",
                ThemePresets::get_powerline_tokyo_night(),
            ),
        ];
        for (name, cfg) in &configs {
            let burn = cfg
                .segments
                .iter()
                .find(|s| s.id == SegmentId::BurnRate)
                .unwrap_or_else(|| panic!("theme {} missing BurnRate", name));
            assert!(!burn.enabled, "theme {} has BurnRate enabled", name);
        }
    }

    #[test]
    fn weekly_usage_disabled_by_default_in_every_builtin_preset() {
        use crate::ui::themes::ThemePresets;
        let configs = [
            ("cometix", ThemePresets::get_cometix()),
            ("default", ThemePresets::get_default()),
            ("minimal", ThemePresets::get_minimal()),
            ("gruvbox", ThemePresets::get_gruvbox()),
            ("nord", ThemePresets::get_nord()),
            ("powerline-dark", ThemePresets::get_powerline_dark()),
            ("powerline-light", ThemePresets::get_powerline_light()),
            (
                "powerline-rose-pine",
                ThemePresets::get_powerline_rose_pine(),
            ),
            (
                "powerline-tokyo-night",
                ThemePresets::get_powerline_tokyo_night(),
            ),
        ];
        for (name, cfg) in &configs {
            let weekly = cfg
                .segments
                .iter()
                .find(|s| s.id == SegmentId::WeeklyUsage)
                .unwrap_or_else(|| panic!("theme {} missing WeeklyUsage", name));
            assert!(
                !weekly.enabled,
                "theme {} ships with WeeklyUsage enabled — must be opt-in",
                name
            );
        }
    }

    #[test]
    fn config_with_disabled_segment_does_not_match() {
        let mut cfg = Config::default();
        if let Some(first) = cfg.segments.first_mut() {
            first.enabled = !first.enabled;
        }
        assert!(!cfg.matches_theme("default"));
    }
}
