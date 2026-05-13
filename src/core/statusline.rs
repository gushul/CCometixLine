use crate::config::{AnsiColor, Config, SegmentConfig, StyleMode, Threshold};
use crate::core::segments::SegmentData;

/// Strip ANSI escape sequences and return visible text length
fn visible_width(text: &str) -> usize {
    let mut visible = String::new();
    let mut in_escape = false;
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            // Start of ANSI escape sequence
            in_escape = true;
            // Skip the [ character
            if chars.peek() == Some(&'[') {
                chars.next();
            }
        } else if in_escape {
            // Skip until we find the end of the escape sequence (letter)
            if ch.is_alphabetic() {
                in_escape = false;
            }
        } else {
            // Regular character
            visible.push(ch);
        }
    }

    visible.chars().count()
}

pub struct StatusLineGenerator {
    config: Config,
}

impl StatusLineGenerator {
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    pub fn generate(&self, segments: Vec<(SegmentConfig, SegmentData)>) -> String {
        let mut output = Vec::new();
        let enabled_segments: Vec<_> = segments
            .into_iter()
            .filter(|(config, _)| config.enabled)
            .collect();

        for (config, data) in enabled_segments.iter() {
            let rendered = self.render_segment(config, data);
            if !rendered.is_empty() {
                output.push(rendered);
            }
        }

        if output.is_empty() {
            return String::new();
        }

        // Handle Powerline arrow separators with color transition
        if self.config.style.separator == "\u{e0b0}" {
            self.join_with_powerline_arrows(&output, &enabled_segments)
        } else {
            // For all other separators, use white color and simple join
            self.join_with_white_separators(&output)
        }
    }

    /// Generate statusline for TUI preview with proper width calculation
    /// This method handles ANSI escape sequences properly for ratatui rendering
    pub fn generate_for_tui(
        &self,
        segments: Vec<(SegmentConfig, SegmentData)>,
    ) -> ratatui::text::Line<'static> {
        use ansi_to_tui::IntoText;
        use ratatui::text::{Line, Span};

        // Use the same generate method and convert to TUI
        let full_output = self.generate(segments);

        if let Ok(text) = full_output.into_text() {
            if let Some(line) = text.lines.into_iter().next() {
                return line;
            }
        }

        // Fallback to raw text
        Line::from(vec![Span::raw(full_output)])
    }

    /// Generate TUI-optimized text with intelligent wrapping by segment for preview
    pub fn generate_for_tui_preview(
        &self,
        segments: Vec<(SegmentConfig, SegmentData)>,
        max_width: u16,
    ) -> ratatui::text::Text<'_> {
        use ansi_to_tui::IntoText;
        use ratatui::text::{Line, Span, Text};

        let enabled_segments: Vec<_> = segments
            .into_iter()
            .filter(|(config, _)| config.enabled)
            .collect();

        if enabled_segments.is_empty() {
            return Text::from(vec![Line::default()]);
        }

        // Render each segment individually
        let mut rendered_segments = Vec::new();
        let mut segment_configs = Vec::new();

        for (config, data) in &enabled_segments {
            let rendered = self.render_segment(config, data);
            if !rendered.is_empty() {
                rendered_segments.push(rendered);
                segment_configs.push(config.clone());
            }
        }

        if rendered_segments.is_empty() {
            return Text::from(vec![Line::default()]);
        }

        // Pre-calculate separators between segments
        let mut separators = Vec::new();
        for i in 0..rendered_segments.len().saturating_sub(1) {
            let separator = if self.config.style.separator == "\u{e0b0}" {
                // Powerline arrows with color transition
                let prev_bg = segment_configs
                    .get(i)
                    .and_then(|config| config.colors.background.as_ref());
                let curr_bg = segment_configs
                    .get(i + 1)
                    .and_then(|config| config.colors.background.as_ref());
                self.create_powerline_arrow(prev_bg, curr_bg)
            } else {
                // Regular separators with white color
                format!("\x1b[37m{}\x1b[0m", self.config.style.separator)
            };
            separators.push(separator);
        }

        // Intelligent line wrapping by segment
        let mut lines: Vec<String> = Vec::new();
        let mut current_line = String::new();
        let mut current_width = 0usize;
        let max_w = max_width as usize;

        for i in 0..rendered_segments.len() {
            let segment = &rendered_segments[i];
            let segment_width = visible_width(segment);

            // Check if adding this segment would exceed max_width
            if current_width > 0 && current_width + segment_width > max_w {
                // Current line would overflow, start a new line
                lines.push(current_line.clone());
                current_line.clear();
                current_width = 0;
            }

            // Add the segment to current line
            current_line.push_str(segment);
            current_width += segment_width;

            // Handle separator if not the last segment
            if i < separators.len() {
                let separator = &separators[i];
                let separator_width = visible_width(separator);

                // Check if next segment exists
                if i + 1 < rendered_segments.len() {
                    let next_segment = &rendered_segments[i + 1];
                    let next_width = visible_width(next_segment);

                    // Check if separator AND next segment both fit
                    if current_width + separator_width + next_width <= max_w {
                        // Both fit, add separator and continue on same line
                        current_line.push_str(separator);
                        current_width += separator_width;
                    } else {
                        // Separator and/or next segment don't fit
                        // Don't add separator, just break line
                        lines.push(current_line.clone());
                        current_line.clear();
                        current_width = 0;
                    }
                }
            }
        }

        // Add the last line if it's not empty
        if !current_line.is_empty() {
            lines.push(current_line);
        }

        // Convert string lines to ratatui Text
        let mut tui_lines = Vec::new();
        for line in lines {
            if let Ok(text) = line.into_text() {
                for tui_line in text.lines {
                    tui_lines.push(tui_line);
                }
            } else {
                tui_lines.push(Line::from(vec![Span::raw(line)]));
            }
        }

        // Ensure we have at least one line
        if tui_lines.is_empty() {
            tui_lines.push(Line::default());
        }

        Text::from(tui_lines)
    }

    fn render_segment(&self, config: &SegmentConfig, data: &SegmentData) -> String {
        let icon = if let Some(dynamic_icon) = data.metadata.get("dynamic_icon") {
            dynamic_icon.clone()
        } else {
            self.get_icon(config)
        };

        // Threshold-derived color overrides `colors.text` for the primary
        // string only; secondary text + icon keep their configured colors so
        // themes stay recognizable.
        let primary_color = self.effective_primary_color(config, data);

        // Apply background color to the entire segment if set
        if let Some(bg_color) = &config.colors.background {
            let bg_code = self.apply_background_color(bg_color);

            // Build the entire segment content first
            let icon_colored = if let Some(icon_color) = &config.colors.icon {
                self.apply_color(&icon, Some(icon_color))
                    .replace("\x1b[0m", "")
            } else {
                icon.clone()
            };

            let text_styled = self
                .apply_style(
                    &data.primary,
                    primary_color.as_ref(),
                    config.styles.text_bold,
                )
                .replace("\x1b[0m", "");

            let mut segment_content = format!(" {} {} ", icon_colored, text_styled);

            if !data.secondary.is_empty() {
                let secondary_styled = self
                    .apply_style(
                        &data.secondary,
                        config.colors.text.as_ref(),
                        config.styles.text_bold,
                    )
                    .replace("\x1b[0m", "");
                segment_content.push_str(&format!("{} ", secondary_styled));
            }

            // Apply background to the entire content and reset at the end
            format!("{}{}\x1b[49m", bg_code, segment_content)
        } else {
            // No background color, use original logic
            let icon_colored = self.apply_color(&icon, config.colors.icon.as_ref());
            let text_styled = self.apply_style(
                &data.primary,
                primary_color.as_ref(),
                config.styles.text_bold,
            );

            let mut segment = format!("{} {}", icon_colored, text_styled);

            if !data.secondary.is_empty() {
                segment.push_str(&format!(
                    " {}",
                    self.apply_style(
                        &data.secondary,
                        config.colors.text.as_ref(),
                        config.styles.text_bold
                    )
                ));
            }

            segment
        }
    }

    /// Returns the color to apply to the segment's primary text. Picks a
    /// threshold-derived color when the segment publishes a `percent`
    /// metadata key and a configured threshold matches; otherwise falls back
    /// to the segment's configured `colors.text`.
    fn effective_primary_color(
        &self,
        config: &SegmentConfig,
        data: &SegmentData,
    ) -> Option<AnsiColor> {
        let thresholds = Threshold::list_from_options(&config.options);
        if !thresholds.is_empty() {
            if let Some(pct_str) = data.metadata.get("percent") {
                if let Ok(percent) = pct_str.parse::<f64>() {
                    if let Some(t) = Threshold::pick(&thresholds, percent) {
                        return Some(t.color.clone());
                    }
                }
            }
        }
        config.colors.text.clone()
    }

    fn get_icon(&self, config: &SegmentConfig) -> String {
        match self.config.style.mode {
            StyleMode::Plain => config.icon.plain.clone(),
            StyleMode::NerdFont => config.icon.nerd_font.clone(),
            StyleMode::Powerline => config.icon.nerd_font.clone(), // Future: use Powerline icons
        }
    }

    fn apply_color(&self, text: &str, color: Option<&AnsiColor>) -> String {
        match color {
            Some(AnsiColor::Color16 { c16 }) => {
                let code = if *c16 < 8 { 30 + c16 } else { 90 + (c16 - 8) };
                format!("\x1b[{}m{}\x1b[0m", code, text)
            }
            Some(AnsiColor::Color256 { c256 }) => {
                format!("\x1b[38;5;{}m{}\x1b[0m", c256, text)
            }
            Some(AnsiColor::Rgb { r, g, b }) => {
                format!("\x1b[38;2;{};{};{}m{}\x1b[0m", r, g, b, text)
            }
            None => text.to_string(),
        }
    }

    fn apply_style(&self, text: &str, color: Option<&AnsiColor>, bold: bool) -> String {
        let mut codes = Vec::new();

        // Add style codes
        if bold {
            codes.push("1".to_string()); // Bold: \x1b[1m
        }

        // Add color codes
        match color {
            Some(AnsiColor::Color16 { c16 }) => {
                let color_code = if *c16 < 8 { 30 + c16 } else { 90 + (c16 - 8) };
                codes.push(color_code.to_string());
            }
            Some(AnsiColor::Color256 { c256 }) => {
                codes.push("38".to_string());
                codes.push("5".to_string());
                codes.push(c256.to_string());
            }
            Some(AnsiColor::Rgb { r, g, b }) => {
                codes.push("38".to_string());
                codes.push("2".to_string());
                codes.push(r.to_string());
                codes.push(g.to_string());
                codes.push(b.to_string());
            }
            None => {}
        }

        if codes.is_empty() {
            text.to_string()
        } else {
            format!("\x1b[{}m{}\x1b[0m", codes.join(";"), text)
        }
    }

    fn apply_background_color(&self, color: &AnsiColor) -> String {
        match color {
            AnsiColor::Color16 { c16 } => {
                let code = if *c16 < 8 { 40 + c16 } else { 100 + (c16 - 8) };
                format!("\x1b[{}m", code)
            }
            AnsiColor::Color256 { c256 } => {
                format!("\x1b[48;5;{}m", c256)
            }
            AnsiColor::Rgb { r, g, b } => {
                format!("\x1b[48;2;{};{};{}m", r, g, b)
            }
        }
    }

    /// Join segments with white separators (non-Powerline)
    fn join_with_white_separators(&self, rendered_segments: &[String]) -> String {
        if rendered_segments.is_empty() {
            return String::new();
        }

        // Use white color for separator
        let white_separator = format!("\x1b[37m{}\x1b[0m", self.config.style.separator);
        rendered_segments.join(&white_separator)
    }

    /// Join segments with Powerline arrow separators with proper color transitions
    fn join_with_powerline_arrows(
        &self,
        rendered_segments: &[String],
        segment_configs: &[(SegmentConfig, SegmentData)],
    ) -> String {
        if rendered_segments.is_empty() {
            return String::new();
        }

        if rendered_segments.len() == 1 {
            return rendered_segments[0].clone();
        }

        let mut result = rendered_segments[0].clone();

        for (i, _) in rendered_segments.iter().enumerate().skip(1) {
            let prev_bg = segment_configs
                .get(i - 1)
                .and_then(|(config, _)| config.colors.background.as_ref());
            let curr_bg = segment_configs
                .get(i)
                .and_then(|(config, _)| config.colors.background.as_ref());

            // Create Powerline arrow with color transition
            let arrow = self.create_powerline_arrow(prev_bg, curr_bg);

            result.push_str(&arrow);
            result.push_str(&rendered_segments[i]);
        }

        // Reset colors at the end
        result.push_str("\x1b[0m");
        result
    }

    /// Create a Powerline arrow with proper color transition
    fn create_powerline_arrow(
        &self,
        prev_bg: Option<&AnsiColor>,
        curr_bg: Option<&AnsiColor>,
    ) -> String {
        let arrow_char = "\u{e0b0}";

        match (prev_bg, curr_bg) {
            (Some(prev), Some(curr)) => {
                // Arrow foreground = previous segment's background
                // Arrow background = current segment's background
                let fg_code = self.color_to_foreground_code(prev);
                let bg_code = self.apply_background_color(curr);
                format!("{}{}{}\x1b[0m", bg_code, fg_code, arrow_char)
            }
            (Some(prev), None) => {
                // Previous segment has background, current doesn't
                let fg_code = self.color_to_foreground_code(prev);
                format!("{}{}\x1b[0m", fg_code, arrow_char)
            }
            (None, Some(curr)) => {
                // Current segment has background, previous doesn't
                let bg_code = self.apply_background_color(curr);
                format!("{}{}\x1b[0m", bg_code, arrow_char)
            }
            (None, None) => {
                // Neither segment has background color
                arrow_char.to_string()
            }
        }
    }

    /// Convert AnsiColor to foreground color code
    fn color_to_foreground_code(&self, color: &AnsiColor) -> String {
        match color {
            AnsiColor::Color16 { c16 } => {
                let code = if *c16 < 8 { 30 + c16 } else { 90 + (c16 - 8) };
                format!("\x1b[{}m", code)
            }
            AnsiColor::Color256 { c256 } => {
                format!("\x1b[38;5;{}m", c256)
            }
            AnsiColor::Rgb { r, g, b } => {
                format!("\x1b[38;2;{};{};{}m", r, g, b)
            }
        }
    }
}

pub fn collect_all_segments(
    config: &Config,
    input: &crate::config::InputData,
) -> Vec<(SegmentConfig, SegmentData)> {
    use crate::core::segments::*;

    let mut results = Vec::new();

    for segment_config in &config.segments {
        // Skip disabled segments to avoid unnecessary API requests
        if !segment_config.enabled {
            continue;
        }

        let segment_data = match segment_config.id {
            crate::config::SegmentId::Model => {
                let segment = ModelSegment::new();
                segment.collect(input)
            }
            crate::config::SegmentId::Directory => {
                let segment = DirectorySegment::new();
                segment.collect(input)
            }
            crate::config::SegmentId::Git => {
                let show_sha = segment_config
                    .options
                    .get("show_sha")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let segment = GitSegment::new().with_sha(show_sha);
                segment.collect(input)
            }
            crate::config::SegmentId::ContextWindow => {
                let segment = ContextWindowSegment::new();
                segment.collect(input)
            }
            crate::config::SegmentId::Usage => {
                let segment = UsageSegment::new();
                segment.collect(input)
            }
            crate::config::SegmentId::WeeklyUsage => {
                let segment = WeeklyUsageSegment::new();
                segment.collect(input)
            }
            crate::config::SegmentId::Cost => {
                let segment = CostSegment::new();
                segment.collect(input)
            }
            crate::config::SegmentId::Session => {
                let segment = SessionSegment::new();
                segment.collect(input)
            }
            crate::config::SegmentId::OutputStyle => {
                let segment = OutputStyleSegment::new();
                segment.collect(input)
            }
            crate::config::SegmentId::Update => {
                let segment = UpdateSegment::new();
                segment.collect(input)
            }
        };

        if let Some(data) = segment_data {
            results.push((segment_config.clone(), data));
        }
    }

    results
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        ColorConfig, IconConfig, SegmentId, StyleConfig, StyleMode, TextStyleConfig,
    };
    use std::collections::HashMap;

    // ---------- visible_width ----------

    #[test]
    fn visible_width_plain_ascii() {
        assert_eq!(visible_width(""), 0);
        assert_eq!(visible_width("abc"), 3);
        assert_eq!(visible_width("hello world"), 11);
    }

    #[test]
    fn visible_width_strips_ansi_color() {
        // \x1b[31mRED\x1b[0m — three visible chars
        assert_eq!(visible_width("\x1b[31mRED\x1b[0m"), 3);
    }

    #[test]
    fn visible_width_multiple_ansi_sequences() {
        let s = "\x1b[1;38;2;255;0;0mA\x1b[0m\x1b[32mB\x1b[0mC";
        assert_eq!(visible_width(s), 3);
    }

    #[test]
    fn visible_width_only_ansi_no_text() {
        assert_eq!(visible_width("\x1b[31m\x1b[0m"), 0);
    }

    #[test]
    fn visible_width_counts_chars_not_bytes() {
        // Multi-byte char must count as 1 (function uses .chars().count()).
        assert_eq!(visible_width("ё"), 1);
        assert_eq!(visible_width("привет"), 6);
    }

    // ---------- StatusLineGenerator::generate ----------

    fn synth_segment(id: SegmentId, primary: &str) -> (SegmentConfig, SegmentData) {
        (
            SegmentConfig {
                id,
                enabled: true,
                icon: IconConfig {
                    plain: "x".to_string(),
                    nerd_font: "x".to_string(),
                },
                colors: ColorConfig {
                    icon: None,
                    text: None,
                    background: None,
                },
                styles: TextStyleConfig { text_bold: false },
                options: HashMap::new(),
            },
            SegmentData {
                primary: primary.to_string(),
                secondary: String::new(),
                metadata: HashMap::new(),
            },
        )
    }

    fn plain_config(separator: &str) -> Config {
        Config {
            style: StyleConfig {
                mode: StyleMode::Plain,
                separator: separator.to_string(),
            },
            segments: vec![],
            theme: "test".to_string(),
        }
    }

    #[test]
    fn generate_empty_input_returns_empty_string() {
        let gen = StatusLineGenerator::new(plain_config(" | "));
        assert_eq!(gen.generate(vec![]), "");
    }

    #[test]
    fn generate_single_segment_contains_primary() {
        let gen = StatusLineGenerator::new(plain_config(" | "));
        let out = gen.generate(vec![synth_segment(SegmentId::Model, "Opus 4")]);
        assert!(out.contains("Opus 4"), "output was: {:?}", out);
        // No separator with a single segment.
        assert!(!out.contains(" | "), "unexpected separator in: {:?}", out);
    }

    #[test]
    fn generate_two_segments_preserves_order_and_separator() {
        let gen = StatusLineGenerator::new(plain_config(" | "));
        let out = gen.generate(vec![
            synth_segment(SegmentId::Model, "Opus 4"),
            synth_segment(SegmentId::Directory, "myproj"),
        ]);
        let i_first = out.find("Opus 4").expect("first primary missing");
        let i_second = out.find("myproj").expect("second primary missing");
        assert!(i_first < i_second, "order broken: {:?}", out);
        assert!(out.contains(" | "), "separator missing in: {:?}", out);
        // Separator must sit between the two primaries.
        let i_sep = out.find(" | ").expect("separator missing");
        assert!(
            i_first < i_sep && i_sep < i_second,
            "separator not between primaries in: {:?}",
            out
        );
    }

    // ---------- T03: threshold-based color override ----------

    fn segment_with_thresholds(
        primary: &str,
        percent: &str,
        thresholds: serde_json::Value,
    ) -> (SegmentConfig, SegmentData) {
        let mut opts = HashMap::new();
        opts.insert("thresholds".to_string(), thresholds);
        let cfg = SegmentConfig {
            id: SegmentId::Usage,
            enabled: true,
            icon: IconConfig {
                plain: "x".to_string(),
                nerd_font: "x".to_string(),
            },
            colors: ColorConfig {
                icon: None,
                text: None,
                background: None,
            },
            styles: TextStyleConfig::default(),
            options: opts,
        };
        let mut meta = HashMap::new();
        meta.insert("percent".to_string(), percent.to_string());
        let data = SegmentData {
            primary: primary.to_string(),
            secondary: String::new(),
            metadata: meta,
        };
        (cfg, data)
    }

    #[test]
    fn threshold_overrides_primary_color_when_percent_meets_threshold() {
        let gen = StatusLineGenerator::new(plain_config(" | "));
        let (cfg, data) = segment_with_thresholds(
            "70%",
            "70.0",
            serde_json::json!([{ "at": 60, "color": { "c16": 3 } }]),
        );
        let out = gen.generate(vec![(cfg, data)]);
        // c16=3 maps to foreground code 33 ("\x1b[33m").
        assert!(
            out.contains("\x1b[33m"),
            "expected yellow code in output: {:?}",
            out
        );
    }

    #[test]
    fn threshold_does_not_apply_when_percent_below_all() {
        let gen = StatusLineGenerator::new(plain_config(" | "));
        let (cfg, data) = segment_with_thresholds(
            "30%",
            "30.0",
            serde_json::json!([{ "at": 60, "color": { "c16": 3 } }]),
        );
        let out = gen.generate(vec![(cfg, data)]);
        assert!(
            !out.contains("\x1b[33m"),
            "yellow leaked for percent below threshold: {:?}",
            out
        );
    }

    #[test]
    fn threshold_picks_highest_matching_color() {
        let gen = StatusLineGenerator::new(plain_config(" | "));
        let (cfg, data) = segment_with_thresholds(
            "90%",
            "90.0",
            serde_json::json!([
                { "at": 60, "color": { "c16": 3 } },  // yellow → \x1b[33m
                { "at": 85, "color": { "c16": 1 } },  // red    → \x1b[31m
            ]),
        );
        let out = gen.generate(vec![(cfg, data)]);
        assert!(out.contains("\x1b[31m"), "red expected: {:?}", out);
        assert!(
            !out.contains("\x1b[33m"),
            "yellow must not also fire when red matches: {:?}",
            out
        );
    }

    #[test]
    fn threshold_ignored_when_metadata_percent_missing() {
        // Segment has thresholds but never published a percent metadata key.
        let gen = StatusLineGenerator::new(plain_config(" | "));
        let mut opts = HashMap::new();
        opts.insert(
            "thresholds".to_string(),
            serde_json::json!([{ "at": 60, "color": { "c16": 3 } }]),
        );
        let cfg = SegmentConfig {
            id: SegmentId::Usage,
            enabled: true,
            icon: IconConfig {
                plain: "x".to_string(),
                nerd_font: "x".to_string(),
            },
            colors: ColorConfig {
                icon: None,
                text: None,
                background: None,
            },
            styles: TextStyleConfig::default(),
            options: opts,
        };
        let data = SegmentData {
            primary: "anything".to_string(),
            secondary: String::new(),
            metadata: HashMap::new(),
        };
        let out = gen.generate(vec![(cfg, data)]);
        assert!(!out.contains("\x1b[33m"));
    }

    #[test]
    fn threshold_does_not_color_the_icon() {
        // Configure an icon color AND a threshold color. The icon must keep
        // its configured color (cyan, c16=14 → 96) and only the primary text
        // should pick up the threshold color (yellow, 33).
        let gen = StatusLineGenerator::new(plain_config(" | "));
        let mut opts = HashMap::new();
        opts.insert(
            "thresholds".to_string(),
            serde_json::json!([{ "at": 60, "color": { "c16": 3 } }]),
        );
        let cfg = SegmentConfig {
            id: SegmentId::Usage,
            enabled: true,
            icon: IconConfig {
                plain: "ICON".to_string(),
                nerd_font: "ICON".to_string(),
            },
            colors: ColorConfig {
                icon: Some(AnsiColor::Color16 { c16: 14 }), // cyan → \x1b[96m
                text: Some(AnsiColor::Color16 { c16: 7 }),  // gray → \x1b[37m
                background: None,
            },
            styles: TextStyleConfig::default(),
            options: opts,
        };
        let mut meta = HashMap::new();
        meta.insert("percent".to_string(), "75.0".to_string());
        let data = SegmentData {
            primary: "75%".to_string(),
            secondary: String::new(),
            metadata: meta,
        };
        let out = gen.generate(vec![(cfg, data)]);
        // Icon's configured cyan color must still be present.
        assert!(out.contains("\x1b[96m"), "icon color lost: {:?}", out);
        // Threshold yellow applied to the primary text.
        assert!(out.contains("\x1b[33m"), "threshold not applied: {:?}", out);
        // Original gray text color (37) must NOT have rendered.
        assert!(
            !out.contains("\x1b[37m"),
            "configured text color leaked alongside threshold: {:?}",
            out
        );
    }

    #[test]
    fn generate_skips_disabled_segments() {
        let gen = StatusLineGenerator::new(plain_config(" | "));
        let (mut cfg_a, data_a) = synth_segment(SegmentId::Model, "Opus 4");
        cfg_a.enabled = false;
        let visible = synth_segment(SegmentId::Directory, "myproj");
        let out = gen.generate(vec![(cfg_a, data_a), visible]);
        assert!(
            !out.contains("Opus 4"),
            "disabled segment leaked: {:?}",
            out
        );
        assert!(out.contains("myproj"));
        // Only one visible segment → no separator.
        assert!(!out.contains(" | "), "spurious separator in: {:?}", out);
    }
}
