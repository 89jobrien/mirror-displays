//! macOS display mirroring via the public CoreGraphics API — no
//! external dependencies.
//!
//! - default or `--mirror`: mirror the primary display to all other displays
//! - `--restore`: restore the original display configuration
//! - `--list`: list connected displays
//! - `--install`: nothing to install (the native version needs no
//!   dependencies)
//! - `--help`: usage

use std::env;
use std::ffi::c_void;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::exit;

const APP: &str = "mirror-displays";

type CGDirectDisplayID = u32;
type CGError = i32;

#[repr(C)]
struct CGPoint {
    x: f64,
    y: f64,
}

#[repr(C)]
struct CGSize {
    width: f64,
    height: f64,
}

#[repr(C)]
struct CGRect {
    origin: CGPoint,
    size: CGSize,
}

const K_CG_NULL_DIRECT_DISPLAY: CGDirectDisplayID = 0;
const K_CG_CONFIGURE_PERMANENTLY: u32 = 2;

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGGetActiveDisplayList(
        maxDisplays: u32,
        activeDisplays: *mut CGDirectDisplayID,
        displayCount: *mut u32,
    ) -> CGError;
    fn CGDisplayIsMain(display: CGDirectDisplayID) -> i32;
    fn CGDisplayBounds(display: CGDirectDisplayID) -> CGRect;
    fn CGDisplayCopyDisplayMode(display: CGDirectDisplayID) -> *mut c_void;
    fn CGDisplayCopyAllDisplayModes(
        display: CGDirectDisplayID,
        options: *mut c_void,
    ) -> *const c_void;
    fn CGDisplayModeGetWidth(mode: *mut c_void) -> usize;
    fn CGDisplayModeGetHeight(mode: *mut c_void) -> usize;
    fn CGDisplayModeGetRefreshRate(mode: *mut c_void) -> f64;
    fn CGBeginDisplayConfiguration(config: *mut *mut c_void) -> CGError;
    fn CGConfigureDisplayOrigin(
        config: *mut c_void,
        display: CGDirectDisplayID,
        x: i32,
        y: i32,
    ) -> CGError;
    fn CGConfigureDisplayWithDisplayMode(
        config: *mut c_void,
        display: CGDirectDisplayID,
        mode: *mut c_void,
        options: *mut c_void,
    ) -> CGError;
    fn CGConfigureDisplayMirrorOfDisplay(
        config: *mut c_void,
        display: CGDirectDisplayID,
        master: CGDirectDisplayID,
    ) -> CGError;
    fn CGCancelDisplayConfiguration(config: *mut c_void) -> CGError;
    fn CGCompleteDisplayConfiguration(config: *mut c_void, option: u32) -> CGError;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFArrayGetCount(array: *const c_void) -> isize;
    fn CFArrayGetValueAtIndex(array: *const c_void, index: isize) -> *const c_void;
}

// ---- helpers ----

fn log(msg: &str) {
    println!("[{}] {}", APP, msg);
}

fn die(msg: &str) -> ! {
    eprintln!("[{}] ERROR: {}", APP, msg);
    exit(1);
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
        die("This tool only supports macOS.");
    }
}

fn exe_name() -> String {
    env::args_os()
        .next()
        .map(|a| a.to_string_lossy().into_owned())
        .unwrap_or_else(|| APP.to_string())
}

// ---- display data ----

#[derive(Clone, Debug, PartialEq)]
struct ModeSpec {
    width: usize,
    height: usize,
    refresh: f64,
}

impl ModeSpec {
    fn from_mode(mode: *mut c_void) -> ModeSpec {
        unsafe {
            ModeSpec {
                width: CGDisplayModeGetWidth(mode),
                height: CGDisplayModeGetHeight(mode),
                refresh: CGDisplayModeGetRefreshRate(mode),
            }
        }
    }

    fn matches(a: &ModeSpec, b: &ModeSpec) -> bool {
        a.width == b.width && a.height == b.height && (a.refresh - b.refresh).abs() < 0.01
    }
}

#[derive(Clone, Debug)]
struct DisplayInfo {
    id: CGDirectDisplayID,
    origin_x: f64,
    origin_y: f64,
    mode: ModeSpec,
}

fn enumerate_displays() -> Vec<DisplayInfo> {
    unsafe {
        let mut count: u32 = 0;
        let mut ids: [CGDirectDisplayID; 32] = [0; 32];
        let err = CGGetActiveDisplayList(32, ids.as_mut_ptr(), &mut count);
        if err != 0 {
            die("Unable to enumerate connected displays.");
        }
        if count > ids.len() as u32 {
            die("Too many connected displays.");
        }
        let mut displays = Vec::new();
        for i in 0..count {
            let id = ids[i as usize];
            let bounds = CGDisplayBounds(id);
            let mode = current_mode(id);
            displays.push(DisplayInfo {
                id,
                origin_x: bounds.origin.x,
                origin_y: bounds.origin.y,
                mode,
            });
        }
        displays
    }
}

fn current_mode(id: CGDirectDisplayID) -> ModeSpec {
    unsafe {
        let mode = CGDisplayCopyDisplayMode(id);
        if mode.is_null() {
            die("Unable to read display mode.");
        }
        ModeSpec::from_mode(mode)
    }
}

fn find_mode_object(id: CGDirectDisplayID, spec: &ModeSpec) -> Option<*mut c_void> {
    unsafe {
        let modes = CGDisplayCopyAllDisplayModes(id, std::ptr::null_mut());
        if modes.is_null() {
            return None;
        }
        let n = CFArrayGetCount(modes);
        for i in 0..n {
            let mode = CFArrayGetValueAtIndex(modes, i) as *mut c_void;
            if ModeSpec::matches(&ModeSpec::from_mode(mode), spec) {
                return Some(mode);
            }
        }
        None
    }
}

fn primary_display(displays: &[DisplayInfo]) -> Option<&DisplayInfo> {
    displays
        .iter()
        .find(|d| unsafe { CGDisplayIsMain(d.id) != 0 })
        .or_else(|| {
            displays
                .iter()
                .find(|d| d.origin_x == 0.0 && d.origin_y == 0.0)
        })
}

// ---- backup ----

fn backup_spec_line(d: &DisplayInfo) -> String {
    format!(
        "display {} {} {} {} {} {}\n",
        d.id, d.origin_x, d.origin_y, d.mode.width, d.mode.height, d.mode.refresh
    )
}

fn parse_backup_spec(line: &str) -> Option<DisplayInfo> {
    let mut it = line.split_whitespace();
    if it.next()? != "display" {
        return None;
    }
    let id: CGDirectDisplayID = it.next()?.parse().ok()?;
    let origin_x: f64 = it.next()?.parse().ok()?;
    let origin_y: f64 = it.next()?.parse().ok()?;
    let width: usize = it.next()?.parse().ok()?;
    let height: usize = it.next()?.parse().ok()?;
    let refresh: f64 = it.next()?.parse().ok()?;
    Some(DisplayInfo {
        id,
        origin_x,
        origin_y,
        mode: ModeSpec {
            width,
            height,
            refresh,
        },
    })
}

fn write_backup(displays: &[DisplayInfo], path: &Path) {
    let mut content = String::from("mirror-displays original configuration\n");
    for d in displays {
        content.push_str(&backup_spec_line(d));
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).ok();
    }
    fs::write(path, content).unwrap_or_else(|e| {
        die(&format!(
            "Unable to save original display configuration: {e}"
        ))
    });
}

// ---- mirror / restore ----

fn apply_mirror(primary: CGDirectDisplayID, others: &[CGDirectDisplayID]) {
    unsafe {
        let mut config: *mut c_void = std::ptr::null_mut();
        if CGBeginDisplayConfiguration(&mut config) != 0 {
            die("Unable to configure displays.");
        }
        for &d in others {
            if CGConfigureDisplayMirrorOfDisplay(config, d, primary) != 0 {
                let _ = CGCancelDisplayConfiguration(config);
                die(&format!("Unable to mirror display {d}."));
            }
        }
        if CGCompleteDisplayConfiguration(config, K_CG_CONFIGURE_PERMANENTLY) != 0 {
            die(&format!(
                "Mirroring failed. Your original profile is saved at {}",
                backup_path().display()
            ));
        }
    }
}

fn enable_mirroring() {
    check_os();
    log("Detecting connected displays...");

    let displays = enumerate_displays();
    if displays.len() < 2 {
        die("At least two enabled displays are required.");
    }

    let primary_id = primary_display(&displays)
        .map(|d| d.id)
        .unwrap_or_else(|| die("Cannot identify the primary display."));

    let backup = backup_path();
    if !backup.exists() {
        log("Saving original display arrangement...");
        write_backup(&displays, &backup);
    } else {
        log("Existing original configuration backup preserved.");
    }

    log(&format!("Primary display: {primary_id}"));
    log(&format!("Connected enabled displays: {}", displays.len()));
    log("Applying mirroring configuration...");

    let others: Vec<CGDirectDisplayID> = displays
        .iter()
        .filter(|d| d.id != primary_id)
        .map(|d| d.id)
        .collect();
    apply_mirror(primary_id, &others);

    log("Mirroring configuration applied.");
}

fn restore_displays() {
    check_os();
    let backup = backup_path();
    if !backup.is_file() {
        die("No saved original configuration found.");
    }
    let text =
        fs::read_to_string(&backup).unwrap_or_else(|_| die("Unable to read saved configuration."));
    let specs: Vec<DisplayInfo> = text.lines().filter_map(parse_backup_spec).collect();

    log("Restoring original display configuration...");

    let displays = enumerate_displays();
    unsafe {
        let mut config: *mut c_void = std::ptr::null_mut();
        if CGBeginDisplayConfiguration(&mut config) != 0 {
            die("Unable to configure displays.");
        }
        for spec in &specs {
            if !displays.iter().any(|d| d.id == spec.id) {
                log(&format!("Display {} not found; skipped.", spec.id));
                continue;
            }
            let _ = CGConfigureDisplayMirrorOfDisplay(config, spec.id, K_CG_NULL_DIRECT_DISPLAY);
            let _ = CGConfigureDisplayOrigin(
                config,
                spec.id,
                spec.origin_x as i32,
                spec.origin_y as i32,
            );
            if let Some(mode) = find_mode_object(spec.id, &spec.mode) {
                let _ =
                    CGConfigureDisplayWithDisplayMode(config, spec.id, mode, std::ptr::null_mut());
            }
        }
        if CGCompleteDisplayConfiguration(config, K_CG_CONFIGURE_PERMANENTLY) != 0 {
            die(&format!(
                "Restoration failed. Backup retained at {}",
                backup.display()
            ));
        }
    }

    let _ = fs::remove_file(&backup);
    log("Original display configuration restored.");
}

// ---- other commands ----

fn list_displays() {
    let displays = enumerate_displays();
    log(&format!("Connected displays: {}", displays.len()));
    for d in &displays {
        println!(
            "display {} origin:({:.0},{:.0}) mode:({}x{}@{}Hz)",
            d.id, d.origin_x, d.origin_y, d.mode.width, d.mode.height, d.mode.refresh
        );
    }
}

fn install_dependency() {
    log("The native CoreGraphics version needs no external dependencies.");
}

fn show_help() {
    let lines = [
        format!("{} - macOS display mirroring (CoreGraphics)", APP),
        String::new(),
        "Usage:".to_string(),
        format!(
            "  {}                  Mirror the primary display to all connected displays",
            exe_name()
        ),
        format!(
            "  {} --mirror         Mirror the primary display to all connected displays",
            exe_name()
        ),
        format!(
            "  {} --restore        Restore the original display layout",
            exe_name()
        ),
        format!("  {} --list           List connected displays", exe_name()),
        format!(
            "  {} --install        Nothing to install (no external dependencies)",
            exe_name()
        ),
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
    fn backup_spec_round_trip() {
        let d = DisplayInfo {
            id: 42,
            origin_x: 0.0,
            origin_y: 1080.0,
            mode: ModeSpec {
                width: 1920,
                height: 1080,
                refresh: 60.0,
            },
        };
        let parsed = parse_backup_spec(&backup_spec_line(&d)).unwrap();
        assert_eq!(parsed.id, 42);
        assert_eq!(parsed.origin_y, 1080.0);
        assert_eq!(parsed.mode.width, 1920);
        assert_eq!(parsed.mode.height, 1080);
        assert!((parsed.mode.refresh - 60.0).abs() < 0.01);
    }

    #[test]
    fn parse_backup_spec_ignores_non_display_lines() {
        assert!(parse_backup_spec("mirror-displays original configuration").is_none());
        assert!(parse_backup_spec("garbage").is_none());
    }

    #[test]
    fn mode_spec_matches_within_epsilon() {
        let a = ModeSpec {
            width: 1920,
            height: 1200,
            refresh: 59.94,
        };
        let b = ModeSpec {
            width: 1920,
            height: 1200,
            refresh: 59.93998718261719,
        };
        assert!(ModeSpec::matches(&a, &b));
        assert!(!ModeSpec::matches(
            &a,
            &ModeSpec {
                width: 1920,
                height: 1200,
                refresh: 50.0
            }
        ));
    }
}
