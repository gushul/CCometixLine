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
}

impl Cli {
    pub fn parse_args() -> Self {
        Self::parse()
    }
}
