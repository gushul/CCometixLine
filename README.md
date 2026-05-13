# CCometixLine

[English](README.md) | [中文](README.zh.md)

A high-performance Claude Code statusline tool written in Rust with Git integration, usage tracking, interactive TUI configuration, and Claude Code enhancement utilities.

![Language:Rust](https://img.shields.io/static/v1?label=Language&message=Rust&color=orange&style=flat-square)
![License:MIT](https://img.shields.io/static/v1?label=License&message=MIT&color=blue&style=flat-square)

## Screenshots

![CCometixLine](assets/img1.png)

The statusline shows: Model | Directory | Git Branch Status | Context Window Information

## Features

### Core Functionality
- **Git integration** with branch, status, and tracking info  
- **Model display** with simplified Claude model names
- **Usage tracking** based on transcript analysis
- **Directory display** showing current workspace
- **Minimal design** using Nerd Font icons

### Interactive TUI Features
- **Interactive main menu** when executed without input
- **TUI configuration interface** with real-time preview
- **Theme system** with multiple built-in presets
- **Segment customization** with granular control
- **Configuration management** (init, check, edit)

### Claude Code Enhancement
- **Context warning disabler** - Remove annoying "Context low" messages
- **Verbose mode enabler** - Enhanced output detail
- **Robust patcher** - Survives Claude Code version updates
- **Automatic backups** - Safe modification with easy recovery

## Installation

### Quick Install (Recommended)

Install via npm (works on all platforms):

```bash
# Install globally
npm install -g @cometix/ccline

# Or using yarn
yarn global add @cometix/ccline

# Or using pnpm
pnpm add -g @cometix/ccline
```

Use npm mirror for faster download:
```bash
npm install -g @cometix/ccline --registry https://registry.npmmirror.com
```

After installation:
- ✅ Global command `ccline` is available everywhere
- ⚙️ Follow the configuration steps below to integrate with Claude Code
- 🎨 Run `ccline -c` to open configuration panel for theme selection

### Claude Code Configuration

Add to your Claude Code `settings.json`:

**Cross-Platform (Recommended)**
```json
{
  "statusLine": {
    "type": "command",
    "command": "~/.claude/ccline/ccline",
    "padding": 0
  }
}
```

> **Note for Windows users:** Starting from Claude Code v2.1.47+, Unix-style path parsing is supported on Windows. The `~` symbol is automatically expanded to your user home directory. **Do not use `%USERPROFILE%`** - it no longer works reliably in v2.1.47+.
> - Recommended: `~/.claude/ccline/ccline` (works on all platforms)
> - Alternative: `"ccline"` (requires npm global installation)

**Fallback (npm installation):**
```json
{
  "statusLine": {
    "type": "command",
    "command": "ccline",
    "padding": 0
  }
}
```
*Use this if npm global installation is available in PATH*

### Update

```bash
npm update -g @cometix/ccline
```

<details>
<summary>Manual Installation (Click to expand)</summary>

Alternatively, download from [Releases](https://github.com/Haleclipse/CCometixLine/releases):

#### Linux

#### Option 1: Dynamic Binary (Recommended)
```bash
mkdir -p ~/.claude/ccline
wget https://github.com/Haleclipse/CCometixLine/releases/latest/download/ccline-linux-x64.tar.gz
tar -xzf ccline-linux-x64.tar.gz
cp ccline ~/.claude/ccline/
chmod +x ~/.claude/ccline/ccline
```
*Requires: Ubuntu 22.04+, CentOS 9+, Debian 11+, RHEL 9+ (glibc 2.35+)*

#### Option 2: Static Binary (Universal Compatibility)
```bash
mkdir -p ~/.claude/ccline
wget https://github.com/Haleclipse/CCometixLine/releases/latest/download/ccline-linux-x64-static.tar.gz
tar -xzf ccline-linux-x64-static.tar.gz
cp ccline ~/.claude/ccline/
chmod +x ~/.claude/ccline/ccline
```
*Works on any Linux distribution (static, no dependencies)*

#### macOS (Intel)

```bash  
mkdir -p ~/.claude/ccline
wget https://github.com/Haleclipse/CCometixLine/releases/latest/download/ccline-macos-x64.tar.gz
tar -xzf ccline-macos-x64.tar.gz
cp ccline ~/.claude/ccline/
chmod +x ~/.claude/ccline/ccline
```

#### macOS (Apple Silicon)

```bash
mkdir -p ~/.claude/ccline  
wget https://github.com/Haleclipse/CCometixLine/releases/latest/download/ccline-macos-arm64.tar.gz
tar -xzf ccline-macos-arm64.tar.gz
cp ccline ~/.claude/ccline/
chmod +x ~/.claude/ccline/ccline
```

#### Windows

```powershell
# Create directory and download
New-Item -ItemType Directory -Force -Path "$env:USERPROFILE\.claude\ccline"
Invoke-WebRequest -Uri "https://github.com/Haleclipse/CCometixLine/releases/latest/download/ccline-windows-x64.zip" -OutFile "ccline-windows-x64.zip"
Expand-Archive -Path "ccline-windows-x64.zip" -DestinationPath "."
Move-Item "ccline.exe" "$env:USERPROFILE\.claude\ccline\"
```

</details>

### Build from Source

```bash
git clone https://github.com/Haleclipse/CCometixLine.git
cd CCometixLine
cargo build --release

# Linux/macOS
mkdir -p ~/.claude/ccline
cp target/release/ccometixline ~/.claude/ccline/ccline
chmod +x ~/.claude/ccline/ccline

# Windows (PowerShell)
New-Item -ItemType Directory -Force -Path "$env:USERPROFILE\.claude\ccline"
copy target\release\ccometixline.exe "$env:USERPROFILE\.claude\ccline\ccline.exe"
```

## Usage

### Theme Override

```bash
# Temporarily use specific theme (overrides config file)
ccline --theme cometix
ccline --theme minimal
ccline --theme gruvbox
ccline --theme nord
ccline --theme powerline-dark

# Or use custom theme files from ~/.claude/ccline/themes/
ccline --theme my-custom-theme
```

### Usage history

`ccline --stats` summarizes the JSONL history accumulated by the background refresh subprocess (`~/.claude/ccline/usage_history.jsonl`). One line per refresh; no network.

```bash
ccline --stats              # last 7 days (default)
ccline --stats day          # last 24 hours
ccline --stats week
ccline --stats month        # last 30 days
ccline --stats --json       # one-line JSON, scriptable
```

Plain output shows samples + avg/max/current for both 5-hour and weekly utilization. JSON output mirrors the same fields. History is bounded: when the file grows past ~1 MB the writer prunes entries older than 90 days.

### Claude Code Enhancement

```bash
# Disable context warnings and enable verbose mode
ccline --patch /path/to/claude-code/cli.js

# Example for common installation
ccline --patch ~/.local/share/fnm/node-versions/v24.4.1/installation/lib/node_modules/@anthropic-ai/claude-code/cli.js
```

## Segments reference

Each segment is a column in the status line. Toggle, recolor, or swap icons in `~/.claude/ccline/config.toml` (per-instance) or `~/.claude/ccline/themes/*.toml` (per-theme). The TUI configurator (`ccline -c`) is the easiest way to edit.

| Segment | Icon (plain / nerd font) | Example output | Default | Purpose |
| --- | --- | --- | --- | --- |
| **Model** | 🤖 / `` | `Opus 4.7 1M` | enabled | Current Claude model with optional `[1m]` context-modifier suffix |
| **Directory** | 📁 / `B` | `CCometixLine` | enabled | Workspace basename |
| **Git** | 🌿 / `2` | `main ✓` or `feat/x ● ↑3` | enabled | Branch + working-tree status |
| **ContextWindow** | ⚡️ / `` | `49.8% · 498.0k tokens` | enabled | Conversation context usage out of model limit |
| **Usage** | 8 circle glyphs ([scale](#usage-indicator-scale)) | `26% · 5-13-22` | disabled | Anthropic 5-hour utilization + reset time |
| **WeeklyUsage** | same 8 circle glyphs | `32% · 5-14-0` | disabled | Anthropic weekly utilization + reset time |
| **BurnRate** | 🔥 / `` | `1.2k/m` or `—` | disabled | Tokens/minute over recent transcript window |
| **ProjectedExhaust** | ⏳ / `2` | `~38m`, `@16:42`, `after reset`, `—` | disabled | ETA to 5-hour limit at current rate |
| **Cost** | 💰 / `` | `$48.45` | disabled | Session cost in USD (from Claude Code) |
| **Session** | ⏱️ / `B` | `3m45s +156 -23` | disabled | Turn duration + lines added/removed |
| **OutputStyle** | 🎯 / `5` | `default` | disabled | Active Claude Code `output_style` |
| **Update** | (varies) | `update available` | not in default themes | Notifies when a newer ccline release exists; add to config manually |

Default rendered status line: `🤖 Model | 📁 Directory | 🌿 Git | ⚡️ ContextWindow`. The other segments are opt-in.

### Git status indicators

- Branch name shown with the configured icon.
- Status: `✓` clean, `●` dirty (uncommitted changes), `⚠` conflicts.
- Remote tracking: `↑n` n commits ahead of remote, `↓n` n commits behind.

### Model display

Names are simplified for compactness:

- `claude-3-5-sonnet` → `Sonnet 3.5`
- `claude-4-sonnet` → `Sonnet 4`
- `claude-opus-4-7` → `Opus 4.7`

Context modifiers (e.g. `[1m]` for the 1-million-token Opus context) are appended as a display suffix — see [Model Configuration](#model-configuration-modelstoml).

### Context window display

Format: `{percent}% · {tokens}k tokens`. The percent is `used_tokens / model_context_limit` based on the running transcript. When no transcript usage is available yet, the segment shows `- · - tokens`.

### Usage indicator scale

Both **Usage** and **WeeklyUsage** segments key their icon on the current percent. The 8 circle-fill glyphs come from Material Design Icons (Nerd Font):

| Utilization | Glyph | Codepoint |
| --- | --- | --- |
| 0–12% | 󰪞 | `E` |
| 13–25% | 󰪟 | `F` |
| 26–37% | 󰪠 | `0` |
| 38–50% | 󰪡 | `1` |
| 51–62% | 󰪢 | `2` |
| 63–75% | 󰪣 | `3` |
| 76–87% | 󰪤 | `4` |
| 88–100% | 󰪥 | `5` |

Plain (non-Nerd-Font) terminals fall back to `📊` for both segments.

### Color thresholds

Usage and WeeklyUsage primaries change color at configurable thresholds (Burn Rate, ContextWindow and Cost can be wired the same way; see follow-up notes in `docs/tasks/`). Default in every theme:

| Threshold | Color | ANSI |
| --- | --- | --- |
| ≥ 60% | yellow | `c16 = 3` |
| ≥ 85% | red | `c16 = 1` |

Override per segment in `config.toml`:

```toml
[[segments.usage.options.thresholds]]
at = 60
color = { c16 = 3 }              # 16-color yellow

[[segments.usage.options.thresholds]]
at = 85
color = { r = 220, g = 60, b = 60 }   # 24-bit RGB
```

Only the primary text picks up the threshold color — the icon and secondary text keep their configured colors so themes remain recognizable. Threshold lookup reads the segment's `metadata.percent` key, which Usage / WeeklyUsage populate automatically.

### Reset time format

For Usage and WeeklyUsage, the secondary text shows the next reset in `month-day-hour` form in your local timezone (e.g. `5-13-22` = May 13, 22:00 local). If the minute is past 45, the hour rounds up to reflect the next effective boundary.

The format is currently hardcoded; a future task (see `docs/tasks/T15-...md`) makes this configurable (clock-only, relative, ISO, hidden).

### BurnRate `token_basis`

BurnRate sums per-turn tokens from the transcript and divides by the elapsed window. Which token fields are counted is configurable:

```toml
[segments.burn_rate.options]
token_basis = "input_output"   # default — input + output tokens
# token_basis = "output_only"  # output_only — strictest; closest to what moves the 5h quota
# token_basis = "total"        # legacy — input + output + cache_creation + cache_read
window_seconds = 900           # sliding window (default 15 min)
min_data_seconds = 300         # need ≥ 5 min of data before showing a rate
min_samples = 3                # need ≥ 3 turns in window
```

`"total"` is the pre-T11 behavior; in practice it inflates the displayed rate by 1–2 orders of magnitude because Anthropic's cache reads (50–200k tokens per turn) don't actually drive the 5h limit. Stick with the default `"input_output"` unless you have a reason.

When data is too thin (cold session, fewer than `min_samples` turns, or under `min_data_seconds` of elapsed time), the segment displays `—`.

### ProjectedExhaust modes

ProjectedExhaust projects when the 5-hour utilization will hit 100% at the current rate, using the **change in utilization** between two cache snapshots (independent of BurnRate).

```toml
[segments.projected_exhaust.options]
format = "duration"            # ~38m, ~2h15m, <1m
# format = "clock"             # @16:42 in local time
min_history_seconds = 300      # need ≥ 5 min between snapshots to project
```

Outcomes:

- `~38m` / `~2h15m` — projected time to exhaust.
- `@16:42` — projected clock time (when `format = "clock"`).
- `after reset` — at this rate, the window resets before you exhaust it.
- `—` — not enough history yet, or utilization didn't grow between snapshots (idle / just past a reset).

The "two snapshots" come from the SWR cache history (rotated automatically on every refresh). On a fresh install, the segment renders `—` until the second background refresh completes — usually within a minute.

### SWR cache behavior

The Usage / WeeklyUsage / ProjectedExhaust segments share a single on-disk cache at `~/.claude/ccline/.api_usage_cache.json`. Status-line renders never block on network: a detached `ccline --refresh-usage` subprocess does the fetch when the cache is stale.

```toml
[segments.usage.options]
cache_duration = 180             # hot window — serve, no refresh (default 180s)
revalidate_after_seconds = 1800  # past this, segment hides until refresh completes
```

| Age of cache | State | Behavior |
| --- | --- | --- |
| `< cache_duration` | Hot | Serve immediately, no refresh. |
| `cache_duration ≤ age < revalidate_after_seconds` | SoftStale | Serve immediately + spawn background refresh. |
| `≥ revalidate_after_seconds` | HardStale | Segment hides, spawn refresh. |
| no file yet | Cold | Segment hides, spawn refresh. |

Concurrent renders fan out to a single refresh subprocess via the `~/.claude/ccline/.usage_refresh.lock` file (cleaned up after 30s if the holder dies).

## Configuration

CCometixLine supports full configuration via TOML files and interactive TUI:

- **Configuration file**: `~/.claude/ccline/config.toml`
- **Interactive TUI**: `ccline --config` for real-time editing with preview
- **Theme files**: `~/.claude/ccline/themes/*.toml` for custom themes
- **Automatic initialization**: `ccline --init` creates default configuration

### Available Segments

All segments support enable/disable toggles, custom icons and colors, and per-segment options. See the [Segments reference](#segments-reference) above for icons, options, and outputs.

### Model Configuration (`models.toml`)

Location: `~/.claude/ccline/models.toml` (auto-created on first run)

This file configures how model IDs are displayed and their context window limits. Claude models (Sonnet, Opus, Haiku) are automatically recognized with version extraction — you only need this file for overrides or third-party models.

```toml
# Model entries: simple substring matching on the model ID
# These take priority over built-in Claude model recognition
[[models]]
pattern = "glm-4.5"
display_name = "GLM-4.5"
context_limit = 128000

[[models]]
pattern = "kimi-k2"
display_name = "Kimi K2"
context_limit = 128000

# Context modifiers: matched independently and composable with model entries
# Overrides context_limit and appends display_suffix to the display name
# e.g., model "Opus 4" + modifier " 1M" = "Opus 4 1M"
[[context_modifiers]]
pattern = "[1m]"
display_suffix = " 1M"
context_limit = 1000000
```


## Requirements

- **Git**: Version 1.5+ (Git 2.22+ recommended for better branch detection)
- **Terminal**: Must support Nerd Fonts for proper icon display
  - Install a [Nerd Font](https://www.nerdfonts.com/) (e.g., FiraCode Nerd Font, JetBrains Mono Nerd Font)
  - Configure your terminal to use the Nerd Font
- **Claude Code**: For statusline integration

## Development

```bash
# Build development version
cargo build

# Run tests
cargo test

# Build optimized release
cargo build --release
```

## Roadmap

- [x] TOML configuration file support
- [x] TUI configuration interface
- [x] Custom themes
- [x] Interactive main menu
- [x] Claude Code enhancement tools

## Contributing

Contributions are welcome! Please feel free to submit issues or pull requests.

## Related Projects

- [tweakcc](https://github.com/Piebald-AI/tweakcc) - Command-line tool to customize your Claude Code themes, thinking verbs, and more.

## License

This project is licensed under the [MIT License](LICENSE).

## Star History

[![Star History Chart](https://api.star-history.com/svg?repos=Haleclipse/CCometixLine&type=Date)](https://star-history.com/#Haleclipse/CCometixLine&Date)
