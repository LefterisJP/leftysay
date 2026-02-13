use anyhow::{anyhow, Context, Result};
use clap::{ArgAction, Parser, ValueEnum};
use directories::ProjectDirs;
use rand::prelude::*;
use serde::Deserialize;
use std::cmp::min;
use std::ffi::OsStr;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use terminal_size::{terminal_size, Height, Width};
use textwrap::wrap;
use walkdir::WalkDir;

const DEFAULT_MESSAGE: &str = "Hello from leftysay!";
const DEFAULT_MAX_HEIGHT_RATIO: f32 = 0.55;
const DEFAULT_BUBBLE_MAX_WIDTH: usize = 60;
const DEFAULT_CACHE_MAX_MB: u64 = 64;
const CACHE_FILE_EXT: &str = "txt";

#[derive(Parser, Debug)]
#[command(
    name = "leftysay",
    version,
    about = "A terminal greeter that renders a speech bubble and image via chafa"
)]
struct Cli {
    /// Override message
    #[arg(long)]
    text: Option<String>,
    /// Render a specific image
    #[arg(long)]
    image: Option<PathBuf>,
    /// Choose a pack
    #[arg(long)]
    pack: Option<String>,
    /// List packs and images
    #[arg(long, action = ArgAction::SetTrue)]
    list: bool,
    /// Diagnostics
    #[arg(long, action = ArgAction::SetTrue)]
    doctor: bool,
    /// Render image only
    #[arg(long, action = ArgAction::SetTrue)]
    no_bubble: bool,
    /// Deterministic selection
    #[arg(long)]
    seed: Option<u64>,
    /// Force chafa format
    #[arg(long)]
    format: Option<ChafaFormat>,
    /// Force chafa colors
    #[arg(long)]
    colors: Option<ChafaColors>,
    /// Maximum image height ratio (0.0-1.0)
    #[arg(long)]
    max_height_ratio: Option<f32>,
    /// Enable animation
    #[arg(long, action = ArgAction::SetTrue)]
    animate: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
struct Config {
    enabled: bool,
    default_pack: String,
    format: ChafaFormat,
    colors: ChafaColors,
    max_height_ratio: f32,
    bubble_style: String,
    cache: bool,
    animate: bool,
    cache_max_mb: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            enabled: true,
            default_pack: "default".to_string(),
            format: ChafaFormat::Auto,
            colors: ChafaColors::Auto,
            max_height_ratio: DEFAULT_MAX_HEIGHT_RATIO,
            bubble_style: "classic".to_string(),
            cache: true,
            animate: false,
            cache_max_mb: DEFAULT_CACHE_MAX_MB,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
struct PackMeta {
    name: String,
    version: String,
    license: String,
    description: String,
    images_dir: String,
}

#[derive(Clone, Debug)]
struct Pack {
    meta: PackMeta,
    images: Vec<PathBuf>,
    messages: Vec<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, ValueEnum, PartialEq)]
#[serde(rename_all = "lowercase")]
enum ChafaFormat {
    Auto,
    #[serde(alias = "symbols")]
    #[value(alias = "symbols")]
    Unicode,
    Kitty,
    #[serde(alias = "iterm")]
    #[value(alias = "iterm")]
    Iterm2,
    #[serde(alias = "sixels")]
    #[value(alias = "sixels")]
    Sixel,
}

impl ChafaFormat {
    fn as_arg(self) -> &'static str {
        match self {
            ChafaFormat::Auto => "auto",
            ChafaFormat::Unicode => "symbols",
            ChafaFormat::Kitty => "kitty",
            ChafaFormat::Iterm2 => "iterm",
            ChafaFormat::Sixel => "sixels",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, ValueEnum, PartialEq)]
#[serde(rename_all = "lowercase")]
enum ChafaColors {
    Auto,
    #[serde(alias = "full")]
    #[value(alias = "full")]
    Truecolor,
    #[serde(alias = "256")]
    #[value(alias = "256")]
    C256,
    #[serde(alias = "16")]
    #[value(alias = "16")]
    C16,
}

impl ChafaColors {
    fn as_arg(self) -> &'static str {
        match self {
            ChafaColors::Auto => "auto",
            ChafaColors::Truecolor => "full",
            ChafaColors::C256 => "256",
            ChafaColors::C16 => "16",
        }
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let config = load_config()?;

    if !config.enabled {
        return Ok(());
    }

    let chafa = find_chafa().map_err(|e| {
        eprintln!("{e}");
        anyhow!("chafa missing")
    })?;

    let (term_cols, term_rows) = terminal_dimensions();

    if cli.doctor {
        print_doctor(&chafa, term_cols, term_rows, &config)?;
        return Ok(());
    }

    let packs = scan_packs()?;
    if cli.list {
        print_pack_list(&packs);
        return Ok(());
    }

    let format = cli.format.unwrap_or(config.format);
    let colors = cli.colors.unwrap_or(config.colors);
    let max_height_ratio = cli.max_height_ratio.unwrap_or(config.max_height_ratio);
    let animate = if cli.animate { true } else { config.animate };

    let message = resolve_message(&cli, &packs, &config, cli.seed)?;
    let image_path = resolve_image(&cli, &packs, &config, cli.seed)?;

    let bubble = if cli.no_bubble {
        Vec::new()
    } else {
        render_bubble(&message, term_cols)
    };

    if !bubble.is_empty() {
        for line in &bubble {
            println!("{line}");
        }
    } else if !message.is_empty() && !cli.no_bubble {
        println!("{message}");
    }

    let bubble_height = bubble.len();
    let max_image_rows = ((term_rows as f32) * max_height_ratio).floor() as usize;
    let remaining_rows = term_rows.saturating_sub(bubble_height + 1);
    let image_rows = min(max_image_rows, remaining_rows).max(1);

    let image_output = render_image(
        &chafa,
        &image_path,
        RenderOptions {
            cols: term_cols,
            rows: image_rows,
            format,
            colors,
            animate,
            cache_enabled: config.cache,
            cache_max_mb: config.cache_max_mb,
        },
    )?;

    print!("{image_output}");

    Ok(())
}

fn terminal_dimensions() -> (usize, usize) {
    if let Some((Width(w), Height(h))) = terminal_size() {
        (w as usize, h as usize)
    } else {
        (80, 24)
    }
}

fn load_config() -> Result<Config> {
    let Some(proj_dirs) = ProjectDirs::from("", "", "leftysay") else {
        return Ok(Config::default());
    };
    let config_path = proj_dirs.config_dir().join("config.toml");
    if !config_path.exists() {
        return Ok(Config::default());
    }
    let contents = fs::read_to_string(&config_path)
        .with_context(|| format!("reading config {}", config_path.display()))?;
    let mut config: Config = toml::from_str(&contents).context("parsing config")?;
    if config.max_height_ratio <= 0.0 || config.max_height_ratio > 1.0 {
        config.max_height_ratio = DEFAULT_MAX_HEIGHT_RATIO;
    }
    if config.cache_max_mb == 0 {
        config.cache_max_mb = DEFAULT_CACHE_MAX_MB;
    }
    Ok(config)
}

fn find_chafa() -> Result<PathBuf> {
    if let Ok(path) = std::env::var("LEFTYSAY_CHAFA") {
        return Ok(PathBuf::from(path));
    }

    let candidate = if cfg!(windows) { "chafa.exe" } else { "chafa" };
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            let full = dir.join(candidate);
            if full.is_file() {
                return Ok(full);
            }
        }
    }

    let install_hint = match std::env::consts::OS {
        "linux" => "Install: sudo apt install chafa (Debian/Ubuntu) or sudo pacman -S chafa (Arch)",
        "macos" => "Install: brew install chafa",
        _ => "Install chafa from your package manager",
    };
    Err(anyhow!("leftysay requires chafa. {install_hint}"))
}

fn pack_search_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    if let Ok(extra) = std::env::var("LEFTYSAY_PACKS_DIR") {
        paths.push(PathBuf::from(extra));
    }

    if let Some(proj_dirs) = ProjectDirs::from("", "", "leftysay") {
        paths.push(proj_dirs.data_dir().join("packs"));
    }

    if cfg!(target_os = "macos") {
        let brew_prefixes = [
            std::env::var("HOMEBREW_PREFIX").ok(),
            Some("/opt/homebrew".to_string()),
            Some("/usr/local".to_string()),
        ];
        for prefix in brew_prefixes.iter().flatten() {
            let candidate = Path::new(prefix).join("share/leftysay/packs");
            if candidate.exists() {
                paths.push(candidate);
            }
        }
    } else if cfg!(target_os = "linux") {
        paths.push(PathBuf::from("/usr/share/leftysay/packs"));
    }

    if Path::new("packs").exists() {
        paths.push(PathBuf::from("packs"));
    }

    paths
}

fn scan_packs() -> Result<Vec<Pack>> {
    let mut packs = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for base in pack_search_paths() {
        if !base.exists() {
            continue;
        }

        for entry in WalkDir::new(&base)
            .max_depth(3)
            .into_iter()
            .filter_map(Result::ok)
        {
            if entry.file_name() == "pack.toml" {
                let pack_root = entry.path().parent().unwrap_or(entry.path()).to_path_buf();
                let meta = read_pack_meta(entry.path())?;
                if seen.contains(&meta.name) {
                    continue;
                }
                let images = collect_images(&pack_root, &meta.images_dir);
                if images.is_empty() {
                    continue;
                }
                let messages = read_messages(&pack_root);
                packs.push(Pack {
                    meta,
                    images,
                    messages,
                });
                seen.insert(packs.last().unwrap().meta.name.clone());
            }
        }
    }

    Ok(packs)
}

fn read_pack_meta(path: &Path) -> Result<PackMeta> {
    let contents = fs::read_to_string(path)
        .with_context(|| format!("reading pack meta {}", path.display()))?;
    let meta: PackMeta = toml::from_str(&contents)
        .with_context(|| format!("parsing pack meta {}", path.display()))?;
    Ok(meta)
}

fn collect_images(pack_root: &Path, images_dir: &str) -> Vec<PathBuf> {
    let dir = pack_root.join(images_dir);
    if !dir.exists() {
        return Vec::new();
    }
    WalkDir::new(dir)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .filter(|entry| is_supported_image(entry.path()))
        .map(|entry| entry.into_path())
        .collect()
}

fn is_supported_image(path: &Path) -> bool {
    let Some(ext) = path.extension().and_then(OsStr::to_str) else {
        return false;
    };
    matches!(ext.to_lowercase().as_str(), "png" | "jpg" | "jpeg" | "gif")
}

fn read_messages(pack_root: &Path) -> Vec<String> {
    let path = pack_root.join("messages.txt");
    if !path.exists() {
        return Vec::new();
    }
    let contents = match fs::read_to_string(path) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    contents
        .lines()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .map(|line| line.to_string())
        .collect()
}

fn resolve_message(
    cli: &Cli,
    packs: &[Pack],
    config: &Config,
    seed: Option<u64>,
) -> Result<String> {
    if let Some(text) = &cli.text {
        return Ok(text.clone());
    }

    let pack_name = cli
        .pack
        .clone()
        .unwrap_or_else(|| config.default_pack.clone());
    if let Some(pack) = packs.iter().find(|p| p.meta.name == pack_name) {
        if !pack.messages.is_empty() {
            let idx = pick_index(pack.messages.len(), seed)?;
            return Ok(pack.messages[idx].clone());
        }
    }

    Ok(DEFAULT_MESSAGE.to_string())
}

fn resolve_image(cli: &Cli, packs: &[Pack], config: &Config, seed: Option<u64>) -> Result<PathBuf> {
    if let Some(path) = &cli.image {
        return Ok(path.clone());
    }
    let pack_name = cli
        .pack
        .clone()
        .unwrap_or_else(|| config.default_pack.clone());
    let pack = packs
        .iter()
        .find(|p| p.meta.name == pack_name)
        .ok_or_else(|| anyhow!("pack not found: {pack_name}"))?;
    let idx = pick_index(pack.images.len(), seed)?;
    Ok(pack.images[idx].clone())
}

fn pick_index(len: usize, seed: Option<u64>) -> Result<usize> {
    if len == 0 {
        return Err(anyhow!("no images available"));
    }
    let mut rng: StdRng = match seed {
        Some(seed) => SeedableRng::seed_from_u64(seed),
        None => SeedableRng::from_entropy(),
    };
    Ok(rng.gen_range(0..len))
}

fn render_bubble(text: &str, term_cols: usize) -> Vec<String> {
    let padding = 4usize;
    if term_cols <= padding + 10 {
        return vec![text.to_string()];
    }

    let bubble_width = min(term_cols.saturating_sub(padding), DEFAULT_BUBBLE_MAX_WIDTH);
    let wrapped: Vec<String> = wrap(text, bubble_width)
        .into_iter()
        .map(|line| line.into_owned())
        .collect();

    if wrapped.is_empty() {
        return Vec::new();
    }

    let max_line_len = wrapped.iter().map(|line| line.len()).max().unwrap_or(0);
    let mut lines = Vec::new();
    lines.push(format!(" {}", "_".repeat(max_line_len + 2)));
    if wrapped.len() == 1 {
        lines.push(format!("< {} >", pad_line(&wrapped[0], max_line_len)));
    } else {
        for (idx, line) in wrapped.iter().enumerate() {
            let (left, right) = match idx {
                0 => ('/', '\\'),
                i if i + 1 == wrapped.len() => ('\\', '/'),
                _ => ('|', '|'),
            };
            lines.push(format!("{left} {} {right}", pad_line(line, max_line_len)));
        }
    }
    lines.push(format!(" {}", "-".repeat(max_line_len + 2)));

    lines
}

fn pad_line(line: &str, width: usize) -> String {
    let mut s = line.to_string();
    if line.len() < width {
        s.push_str(&" ".repeat(width - line.len()));
    }
    s
}

fn render_image(chafa: &Path, image: &Path, options: RenderOptions) -> Result<String> {
    let cache_dir = cache_dir();
    let cache_key = cache_key(
        image,
        options.cols,
        options.rows,
        options.format,
        options.colors,
        options.animate,
    )?;
    let cache_path = cache_dir.join(format!("{cache_key}.{CACHE_FILE_EXT}"));

    if options.cache_enabled && cache_path.exists() {
        let contents = fs::read_to_string(&cache_path)?;
        // Touch file for LRU by rewriting.
        fs::write(&cache_path, &contents)?;
        return Ok(contents);
    }

    let output = run_chafa(
        chafa,
        image,
        options.cols,
        options.rows,
        options.format,
        options.colors,
        options.animate,
    )?;

    if options.cache_enabled {
        fs::create_dir_all(&cache_dir)?;
        let mut file = fs::File::create(&cache_path)?;
        file.write_all(output.as_bytes())?;
        enforce_cache_limit(&cache_dir, options.cache_max_mb * 1024 * 1024)?;
    }

    Ok(output)
}

fn run_chafa(
    chafa: &Path,
    image: &Path,
    cols: usize,
    rows: usize,
    format: ChafaFormat,
    colors: ChafaColors,
    animate: bool,
) -> Result<String> {
    let output = run_chafa_once(chafa, image, cols, rows, format, colors, animate)?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).to_string());
    }

    let mut last_err = String::from_utf8_lossy(&output.stderr).to_string();
    let mut fallback_format = format;
    let mut fallback_colors = colors;

    if matches!(format, ChafaFormat::Auto) {
        fallback_format = ChafaFormat::Unicode;
    }
    if matches!(colors, ChafaColors::Auto) {
        fallback_colors = ChafaColors::Truecolor;
    }

    if fallback_format != format || fallback_colors != colors {
        let retry = run_chafa_once(
            chafa,
            image,
            cols,
            rows,
            fallback_format,
            fallback_colors,
            animate,
        )?;
        if retry.status.success() {
            return Ok(String::from_utf8_lossy(&retry.stdout).to_string());
        }
        last_err = String::from_utf8_lossy(&retry.stderr).to_string();
    }

    Err(anyhow!("chafa failed: {last_err}"))
}

fn run_chafa_once(
    chafa: &Path,
    image: &Path,
    cols: usize,
    rows: usize,
    format: ChafaFormat,
    colors: ChafaColors,
    animate: bool,
) -> Result<std::process::Output> {
    let mut cmd = Command::new(chafa);
    cmd.arg(image)
        .arg("--format")
        .arg(format.as_arg())
        .arg("--colors")
        .arg(colors.as_arg())
        .arg("--size")
        .arg(format!("{cols}x{rows}"));
    if animate {
        cmd.arg("--animate");
    }

    cmd.output().with_context(|| "running chafa")
}

fn cache_key(
    image: &Path,
    cols: usize,
    rows: usize,
    format: ChafaFormat,
    colors: ChafaColors,
    animate: bool,
) -> Result<String> {
    let mut hasher = blake3::Hasher::new();
    let meta = fs::metadata(image).with_context(|| "reading image metadata")?;
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    hasher.update(image.to_string_lossy().as_bytes());
    hasher.update(&mtime.to_le_bytes());
    hasher.update(&cols.to_le_bytes());
    hasher.update(&rows.to_le_bytes());
    hasher.update(format.as_arg().as_bytes());
    hasher.update(colors.as_arg().as_bytes());
    hasher.update(&[animate as u8]);
    Ok(hasher.finalize().to_hex().to_string())
}

fn cache_dir() -> PathBuf {
    ProjectDirs::from("", "", "leftysay")
        .map(|proj| proj.cache_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from(".cache/leftysay"))
}

fn enforce_cache_limit(cache_dir: &Path, max_bytes: u64) -> Result<()> {
    if !cache_dir.exists() {
        return Ok(());
    }

    let mut entries: Vec<_> = fs::read_dir(cache_dir)
        .with_context(|| format!("reading cache dir {}", cache_dir.display()))?
        .filter_map(Result::ok)
        .collect();

    let mut total_size: u64 = entries
        .iter()
        .filter_map(|entry| entry.metadata().ok().map(|m| m.len()))
        .sum();

    if total_size <= max_bytes {
        return Ok(());
    }

    entries.sort_by_key(|entry| entry.metadata().and_then(|m| m.modified()).ok());

    for entry in entries {
        if total_size <= max_bytes {
            break;
        }
        let meta = entry.metadata().ok();
        if let Ok(()) = fs::remove_file(entry.path()) {
            if let Some(len) = meta.map(|m| m.len()) {
                total_size = total_size.saturating_sub(len);
            }
        }
    }

    Ok(())
}

fn print_pack_list(packs: &[Pack]) {
    if packs.is_empty() {
        println!("No packs found.");
        return;
    }
    for pack in packs {
        println!(
            "{} (v{}, {}): {}",
            pack.meta.name, pack.meta.version, pack.meta.license, pack.meta.description
        );
        for image in &pack.images {
            if let Some(name) = image.file_name().and_then(OsStr::to_str) {
                println!("  - {name}");
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct RenderOptions {
    cols: usize,
    rows: usize,
    format: ChafaFormat,
    colors: ChafaColors,
    animate: bool,
    cache_enabled: bool,
    cache_max_mb: u64,
}

fn print_doctor(chafa: &Path, cols: usize, rows: usize, config: &Config) -> Result<()> {
    println!("leftysay doctor");
    println!("chafa: {}", chafa.display());
    println!("terminal: {} cols x {} rows", cols, rows);
    println!("config.format: {}", config.format.as_arg());
    println!("config.colors: {}", config.colors.as_arg());
    println!("config.max_height_ratio: {}", config.max_height_ratio);
    println!("config.cache: {}", config.cache);
    println!("config.cache_max_mb: {}", config.cache_max_mb);

    if let Some(proj_dirs) = ProjectDirs::from("", "", "leftysay") {
        println!("config dir: {}", proj_dirs.config_dir().display());
        println!("data dir: {}", proj_dirs.data_dir().display());
        println!("cache dir: {}", proj_dirs.cache_dir().display());
    }
    println!("pack search paths:");
    for path in pack_search_paths() {
        println!("  - {}", path.display());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn bubble_renders_multiple_lines() {
        let lines = render_bubble("hello world from leftysay", 40);
        assert!(lines.len() >= 3);
        assert!(lines.first().unwrap().contains('_'));
        assert!(lines.last().unwrap().contains('-'));
    }

    #[test]
    fn cache_key_changes_with_size() {
        let dir = TempDir::new().unwrap();
        let image_path = dir.path().join("image.png");
        fs::write(&image_path, b"fake").unwrap();

        let key_small = cache_key(
            &image_path,
            40,
            10,
            ChafaFormat::Auto,
            ChafaColors::Auto,
            false,
        )
        .unwrap();
        let key_large = cache_key(
            &image_path,
            80,
            10,
            ChafaFormat::Auto,
            ChafaColors::Auto,
            false,
        )
        .unwrap();

        assert_ne!(key_small, key_large);
    }

    #[test]
    fn scan_packs_reads_pack_meta_and_images() {
        let dir = TempDir::new().unwrap();
        let pack_root = dir.path().join("packs/default");
        fs::create_dir_all(pack_root.join("images")).unwrap();
        fs::write(
            pack_root.join("pack.toml"),
            "name = \"default\"\nversion = \"0.1.0\"\nlicense = \"CC0-1.0\"\ndescription = \"Test\"\nimages_dir = \"images\"\n",
        )
        .unwrap();
        fs::write(pack_root.join("images/test.png"), b"fake").unwrap();

        std::env::set_var("LEFTYSAY_PACKS_DIR", dir.path().join("packs"));
        let packs = scan_packs().unwrap();
        assert_eq!(packs.len(), 1);
        assert_eq!(packs[0].meta.name, "default");
        assert_eq!(packs[0].images.len(), 1);
        std::env::remove_var("LEFTYSAY_PACKS_DIR");
    }
}
