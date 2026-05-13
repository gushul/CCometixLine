use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "ccline")]
#[command(version, about = "High-performance Claude Code StatusLine")]
pub struct Cli {
    /// Enter TUI configuration mode
    #[arg(short = 'c', long = "config")]
    pub config: bool,

    /// Set theme
    #[arg(short = 't', long = "theme")]
    pub theme: Option<String>,

    /// Patch Claude Code cli.js to disable context warnings
    #[arg(long = "patch")]
    pub patch: Option<String>,

    /// Internal: refresh the usage cache and exit. Used by the statusline as a
    /// detached subprocess to keep the hot path non-blocking. Not meant for
    /// direct invocation.
    #[arg(long = "refresh-usage", hide = true)]
    pub refresh_usage: bool,

    /// Print a usage-history summary and exit. Optional window argument:
    /// `day` (last 24h), `week` (last 7 days — default), `month` (last 30
    /// days). History is populated by the background refresh — wait a few
    /// minutes after first run for data.
    #[arg(
        long = "stats",
        value_name = "WINDOW",
        num_args = 0..=1,
        default_missing_value = "week",
        value_parser = ["day", "week", "month"],
    )]
    pub stats: Option<String>,

    /// Emit `--stats` output as JSON on one line instead of a plain-text table.
    #[arg(long = "json")]
    pub json: bool,
}

impl Cli {
    pub fn parse_args() -> Self {
        Self::parse()
    }
}
