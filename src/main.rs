//! macOS display mirroring via `displayplacer`.
//!
//! Rewrite of `mirror-displays.sh`: mirror the primary display to every
//! connected external display, backing up the original layout so it can
//! be restored later.

use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};

const APP: &str = "mirror-displays";

fn log(msg: &str) {
    println!("[{}] {}", APP, msg);
}

fn die(msg: &str) -> ! {
    eprintln!("[{}] ERROR: {}", APP, msg);
    std::process::exit(1);
}

fn home_dir() -> PathBuf {
    env::var_os("HOME").map(PathBuf::from).unwrap_or_default()
}

fn state_dir() -> PathBuf {
    let mut dir = home_dir();
    dir.push(".config");
    dir.push(APP);
    dir
}

fn backup_path() -> PathBuf {
    let mut path = state_dir();
    path.push("original-profile");
    path
}

fn check_os() {
    if env::consts::OS != "macos" {
        die("This script only supports macOS.");
    }
}

fn is_executable_file(path: &Path) -> bool {
    fs::metadata(path)
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

fn command_exists(name: &str) -> bool {
    match env::var_os("PATH") {
        Some(paths) => env::split_paths(&paths).any(|dir| is_executable_file(&dir.join(name))),
        None => false,
    }
}

fn exe_name() -> String {
    env::args_os()
        .next()
        .map(|a| a.to_string_lossy().into_owned())
        .unwrap_or_else(|| APP.to_string())
}

fn check_dependency() {
    if !command_exists("displayplacer") {
        die(&format!(
            "displayplacer is missing. Run: {} --install",
            exe_name()
        ));
    }
}

fn install_dependency() {
    if !command_exists("brew") {
        die("Homebrew is required. Install it from https://brew.sh");
    }
    run_or_exit("brew", &["install", "displayplacer"]);
}

/// Run a command, exiting with its status (mirrors the script's `set -e`).
fn run_or_exit(program: &str, args: &[&str]) {
    match run(program, args) {
        Some(status) if status.success() => std::process::exit(0),
        Some(status) => std::process::exit(status.code().unwrap_or(1)),
        None => std::process::exit(1),
    }
}

/// Run a command with inherited stdio; `None` if it cannot be spawned.
fn run(program: &str, args: &[&str]) -> Option<ExitStatus> {
    Command::new(program).args(args).status().ok()
}

/// Capture stdout of a command (stderr passes through). `None` if the
/// command fails or cannot be spawned.
fn run_capture(program: &str, args: &[&str]) -> Option<String> {
    let child = Command::new(program)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .ok()?;
    let output = child.wait_with_output().ok()?;
    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        None
    }
}

/// Extract the quoted segments of a displayplacer profile line, mirroring
/// the script's regex loop: repeated `\"([^\"]+)\"` captures.
fn parse_quoted_args(s: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut offset = 0usize;
    while let Some(rel_start) = s[offset..].find('"') {
        let start = offset + rel_start;
        let after_start = start + 1;
        let Some(rel_end) = s[after_start..].find('"') else {
            break;
        };
        let end = after_start + rel_end;
        let arg = &s[after_start..end];
        if !arg.is_empty() {
            args.push(arg.to_string());
        }
        offset = end + 1;
    }
    args
}

fn parse_profile(profile: &str) -> Vec<String> {
    parse_quoted_args(profile)
}

/// The last line of `displayplacer list` output when any line starts with
/// `displayplacer ` (the executable profile); empty otherwise.
fn get_profile() -> String {
    let output = match run_capture("displayplacer", &["list"]) {
        Some(o) => o.trim_end_matches('\n').to_string(),
        None => die("Unable to read display configuration."),
    };

    let mut saw_profile_line = false;
    let mut last_line = String::new();
    for line in output.lines() {
        if line.starts_with("displayplacer ") {
            saw_profile_line = true;
        }
        last_line = line.to_owned();
    }

    if saw_profile_line && !last_line.is_empty() {
        last_line
    } else {
        die("Could not extract display configuration.")
    }
}

/// Extract the display id token from a config line: `^id:([^[:space:]]+)`
/// (the capture is non-empty).
fn parse_id_token(config: &str) -> Option<String> {
    let rest = config.strip_prefix("id:")?;
    let token: String = rest.chars().take_while(|c| !c.is_whitespace()).collect();
    (!token.is_empty()).then_some(token)
}

/// Flatten displayplacer mirror groups: `A+B+C` -> `["A", "B", "C"]`.
fn flatten_ids(id: &str) -> Vec<String> {
    let mut ids = Vec::new();
    let mut remaining = id;
    while let Some((group_id, tail)) = remaining.split_once('+') {
        ids.push(group_id.to_string());
        remaining = tail;
    }
    ids.push(remaining.to_string());
    ids
}

/// The primary display's config with its `id:` prefix removed, mirroring
/// `sed 's/^id:[^ ]* //'`.
fn rest_after_id(config: &str) -> String {
    let Some(rest) = config.strip_prefix("id:") else {
        return config.to_string();
    };
    match rest.find(' ') {
        Some(pos) => rest[pos + 1..].to_string(),
        None => config.to_string(),
    }
}

fn enable_mirroring() {
    check_os();
    check_dependency();

    log("Detecting connected displays...");

    let profile = get_profile();
    let profile_args = parse_profile(&profile);
    if profile_args.is_empty() {
        die("Could not parse display configuration.");
    }

    let mut primary: Option<String> = None;
    let mut source_config = String::new();
    let mut ids: Vec<String> = Vec::new();

    for config in &profile_args {
        if config.contains("enabled:false") {
            continue;
        }

        let Some(display_id) = parse_id_token(config) else {
            continue;
        };

        // Flatten any existing mirror groups.
        ids.extend(flatten_ids(&display_id));

        if config.contains("origin:(0,0)") {
            primary = Some(display_id);
            source_config = config.clone();
        }
    }

    if ids.len() < 2 {
        die("At least two enabled displays are required.");
    }

    let primary = match primary {
        Some(p) if !p.is_empty() => p,
        _ => die("Cannot identify the primary display at origin (0,0)."),
    };

    // Do not overwrite the original backup on repeated runs.
    fs::create_dir_all(state_dir())
        .unwrap_or_else(|e| die(&format!("Unable to create {}: {e}", state_dir().display())));

    let backup = backup_path();
    if !backup.is_file() {
        log("Saving original display arrangement...");
        fs::write(&backup, format!("{}\n", profile))
            .unwrap_or_else(|e| die(&format!("Unable to save original display configuration: {e}")));
    } else {
        log("Existing original configuration backup preserved.");
    }

    // Build the grouped mirror ID list.
    let grouped_ids = ids.join("+");

    // Preserve the primary display's current mode and settings.
    let rest = rest_after_id(&source_config);
    if rest.is_empty() {
        die("Could not determine primary display settings.");
    }
    let mirror_config = format!("id:{grouped_ids} {rest}");

    log(&format!("Primary display: {primary}"));
    log(&format!("Connected enabled displays: {}", ids.len()));
    log("Applying mirroring configuration...");

    match run("displayplacer", &[mirror_config.as_str()]) {
        Some(status) if status.success() => {}
        _ => die(&format!(
            "Mirroring failed. Your original profile is saved at {}",
            backup.display()
        )),
    }

    log("Mirroring configuration applied.");
}

fn restore_displays() {
    check_os();
    check_dependency();

    let backup = backup_path();
    if !backup.is_file() {
        die("No saved original configuration found.");
    }

    let profile =
        fs::read_to_string(&backup).unwrap_or_else(|_| die("Unable to read saved configuration."));
    let profile_args = parse_profile(&profile);
    if profile_args.is_empty() {
        die("Could not parse display configuration.");
    }

    log("Restoring original display configuration...");

    let args: Vec<&str> = profile_args.iter().map(String::as_str).collect();
    match run("displayplacer", &args) {
        Some(status) if status.success() => {}
        _ => die(&format!(
            "Restoration failed. Backup retained at {}",
            backup.display()
        )),
    }

    let _ = fs::remove_file(&backup);

    log("Original display configuration restored.");
}

fn list_displays() {
    check_dependency();
    run_or_exit("displayplacer", &["list"]);
}

fn show_help() {
    let lines = [
        format!("{} - macOS display mirroring", APP),
        String::new(),
        "Usage:".to_string(),
        format!("  {}                  Enable display mirroring", exe_name()),
        format!("  {} --mirror         Enable display mirroring", exe_name()),
        format!(
            "  {} --restore        Restore original display arrangement",
            exe_name()
        ),
        format!("  {} --list           List connected displays", exe_name()),
        format!("  {} --install        Install displayplacer", exe_name()),
        format!("  {} --help           Show this help", exe_name()),
        String::new(),
        "Backup:".to_string(),
        format!("  {}", backup_path().display()),
    ];
    for line in lines {
        println!("{line}");
    }
}

fn main() {
    let first: Option<String> = env::args_os()
        .nth(1)
        .map(|a| a.to_string_lossy().into_owned());
    match first.as_deref() {
        Some("--install") => install_dependency(),
        Some("--list") => {
            check_os();
            list_displays();
        }
        Some("--restore") => restore_displays(),
        Some("--mirror") | Some("") | None => enable_mirroring(),
        Some("--help") | Some("-h") => show_help(),
        Some(other) => die(&format!("Unknown option: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flatten_ids_splits_mirror_groups() {
        assert_eq!(
            flatten_ids("A+B+C"),
            vec!["A".to_string(), "B".to_string(), "C".to_string()]
        );
        assert_eq!(flatten_ids("A+B"), vec!["A".to_string(), "B".to_string()]);
        assert_eq!(flatten_ids("A"), vec!["A".to_string()]);
        assert_eq!(flatten_ids("A+"), vec!["A".to_string(), String::new()]);
    }

    #[test]
    fn parse_id_token_matches_regex_anchor() {
        assert_eq!(
            parse_id_token("id:A+B origin:(0,0)"),
            Some("A+B".to_string())
        );
        assert_eq!(parse_id_token("id: origin:(0,0)"), None);
        assert_eq!(parse_id_token("origin:(0,0)"), None);
    }

    #[test]
    fn rest_after_id_matches_sed() {
        assert_eq!(
            rest_after_id("id:A+B origin:(0,0) res:1920x1080"),
            "origin:(0,0) res:1920x1080"
        );
        assert_eq!(rest_after_id("id:A"), "id:A");
        assert_eq!(rest_after_id("id: A B"), "A B");
    }

    #[test]
    fn parse_quoted_args_extracts_segments() {
        assert_eq!(
            parse_quoted_args("displayplacer \"id:A+id:B origin:(0,0)\""),
            vec!["id:A+id:B origin:(0,0)".to_string()]
        );
        assert_eq!(parse_quoted_args("no quotes here"), Vec::<String>::new());
    }
}
