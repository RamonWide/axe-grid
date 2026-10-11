//! The Axe-Grid installer for Windows. Double-click it in the unpacked
//! download: it copies the programs that sit next to it into the current
//! user's programs folder, puts a shortcut on the desktop and in the Start
//! menu, and lists Axe-Grid under Installed apps. No administrator rights,
//! no drivers, no services.
//!
//!   axegrid-setup [install | uninstall] [--quiet] [--no-desktop]

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const APP_NAME: &str = "Axe-Grid";
const APP_EXE: &str = "axegrid.exe";
const SETUP_EXE: &str = "axegrid-setup.exe";
/// Files installed when they sit next to the installer. Only the app itself is required.
const FILES: [&str; 6] = [APP_EXE, "axegrid-cli.exe", SETUP_EXE, "LICENSE", "ATTRIBUTION.md", "INSTALL.md"];
const SHORTCUT: &str = "Axe-Grid.lnk";
/// Where Windows keeps the current user's list of installed apps.
const UNINSTALL_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\Axe-Grid";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Options {
    uninstall: bool,
    quiet: bool,
    desktop: bool,
}

fn parse(args: &[String]) -> Result<Options, String> {
    let mut options = Options { uninstall: false, quiet: false, desktop: true };
    for arg in args {
        match arg.as_str() {
            "install" => options.uninstall = false,
            "uninstall" => options.uninstall = true,
            "--quiet" => options.quiet = true,
            "--no-desktop" => options.desktop = false,
            other => return Err(format!("unknown option {other}")),
        }
    }
    Ok(options)
}

/// The folder the app is installed into, under the user's own programs.
fn install_dir(local_app_data: &Path) -> PathBuf {
    local_app_data.join("Programs").join(APP_NAME)
}

/// A path as a single-quoted PowerShell string.
fn quoted(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "''"))
}

/// The PowerShell folder name of each place a shortcut goes.
fn shortcut_folders(desktop: bool) -> Vec<&'static str> {
    let mut folders = vec!["Programs"];
    if desktop {
        folders.push("Desktop");
    }
    folders
}

/// A script that writes a shortcut to `target` into one of the user's
/// special folders. Windows resolves the folder, so a desktop that was
/// moved (to a cloud folder, say) is found.
fn shortcut_script(folder: &str, target: &Path) -> String {
    let working = target.parent().unwrap_or(target);
    format!(
        "$f=[Environment]::GetFolderPath('{folder}');\
         $s=(New-Object -ComObject WScript.Shell).CreateShortcut((Join-Path $f '{SHORTCUT}'));\
         $s.TargetPath={};$s.WorkingDirectory={};$s.IconLocation={};\
         $s.Description='Editor for the Axe-Fx II';$s.Save()",
        quoted(target),
        quoted(working),
        quoted(target)
    )
}

fn remove_shortcut_script(folder: &str) -> String {
    format!(
        "Remove-Item -ErrorAction SilentlyContinue (Join-Path ([Environment]::GetFolderPath('{folder}')) '{SHORTCUT}')"
    )
}

/// The values of the Installed apps entry.
fn uninstall_entry(dir: &Path) -> Vec<(&'static str, String)> {
    vec![
        ("DisplayName", APP_NAME.to_owned()),
        ("DisplayVersion", env!("CARGO_PKG_VERSION").to_owned()),
        ("Publisher", "The Axe-Grid contributors".to_owned()),
        ("InstallLocation", dir.display().to_string()),
        ("DisplayIcon", dir.join(APP_EXE).display().to_string()),
        ("UninstallString", format!("\"{}\" uninstall", dir.join(SETUP_EXE).display())),
    ]
}

/// Runs a Windows tool and reports whether it succeeded.
fn tool(program: &str, args: &[&str]) -> bool {
    Command::new(program).args(args).output().is_ok_and(|out| out.status.success())
}

fn powershell(script: &str) -> bool {
    tool("powershell", &["-NoProfile", "-NonInteractive", "-Command", script])
}

struct Report {
    problems: u32,
}

impl Report {
    fn say(&self, text: &str) {
        println!("{text}");
    }

    fn problem(&mut self, text: &str) {
        self.problems += 1;
        println!("  ! {text}");
    }
}

fn install(report: &mut Report, dir: &Path, desktop: bool) {
    report.say(&format!("Installing {APP_NAME} {} into {}", env!("CARGO_PKG_VERSION"), dir.display()));
    let Some(source) = std::env::current_exe().ok().and_then(|exe| exe.parent().map(Path::to_path_buf)) else {
        return report.problem("could not find the folder this installer is in");
    };
    if !source.join(APP_EXE).exists() {
        return report.problem(&format!("{APP_EXE} is not next to this installer; unpack the whole download first"));
    }
    if let Err(error) = std::fs::create_dir_all(dir) {
        return report.problem(&format!("could not create {}: {error}", dir.display()));
    }
    if source != dir {
        for name in FILES.iter().filter(|name| source.join(name).exists()) {
            match std::fs::copy(source.join(name), dir.join(name)) {
                Ok(_) => report.say(&format!("  copied {name}")),
                Err(error) => report.problem(&format!("{name}: {error} (is {APP_NAME} still running?)")),
            }
        }
    }
    let app = dir.join(APP_EXE);
    for folder in shortcut_folders(desktop) {
        let place = if folder == "Desktop" { "the desktop" } else { "the Start menu" };
        if powershell(&shortcut_script(folder, &app)) {
            report.say(&format!("  shortcut added to {place}"));
        } else {
            report.problem(&format!("could not add a shortcut to {place}"));
        }
    }
    let listed = uninstall_entry(dir)
        .iter()
        .all(|(name, value)| tool("reg", &["add", UNINSTALL_KEY, "/v", name, "/t", "REG_SZ", "/d", value, "/f"]))
        && ["NoModify", "NoRepair"]
            .iter()
            .all(|name| tool("reg", &["add", UNINSTALL_KEY, "/v", name, "/t", "REG_DWORD", "/d", "1", "/f"]));
    if listed {
        report.say("  listed under Installed apps");
    } else {
        report.problem("could not add the Installed apps entry");
    }
}

fn uninstall(report: &mut Report, dir: &Path) {
    report.say(&format!("Removing {APP_NAME} from {}", dir.display()));
    for folder in shortcut_folders(true) {
        powershell(&remove_shortcut_script(folder));
    }
    report.say("  shortcuts removed");
    tool("reg", &["delete", UNINSTALL_KEY, "/f"]);
    for name in FILES.iter().filter(|name| **name != SETUP_EXE) {
        let path = dir.join(name);
        if path.exists() {
            if let Err(error) = std::fs::remove_file(&path) {
                report.problem(&format!("{name}: {error} (is {APP_NAME} still running?)"));
            }
        }
    }
    // This program cannot delete itself while it runs: hand the folder to a
    // command that waits a moment and then removes it.
    let folder = dir.to_string_lossy();
    if !folder.is_empty() && dir.ends_with(APP_NAME) {
        let script = format!("ping -n 3 127.0.0.1 >nul & rmdir /s /q \"{folder}\"");
        let _ = Command::new("cmd").args(["/c", &script]).current_dir(std::env::temp_dir()).spawn();
    }
    report.say("  files removed");
}

fn pause(quiet: bool) {
    if !quiet {
        println!("\nPress Enter to close this window.");
        let _ = std::io::stdin().read_line(&mut String::new());
    }
}

fn main() -> ExitCode {
    if !cfg!(windows) {
        eprintln!("This installer is for Windows. On other systems see INSTALL.md.");
        return ExitCode::FAILURE;
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    let options = match parse(&args) {
        Ok(options) => options,
        Err(problem) => {
            eprintln!("{problem}\nusage: axegrid-setup [install | uninstall] [--quiet] [--no-desktop]");
            return ExitCode::from(2);
        }
    };
    let mut report = Report { problems: 0 };
    match std::env::var_os("LOCALAPPDATA").map(PathBuf::from) {
        Some(base) if options.uninstall => uninstall(&mut report, &install_dir(&base)),
        Some(base) => install(&mut report, &install_dir(&base), options.desktop),
        None => report.problem("Windows did not say where this user's programs go (LOCALAPPDATA is not set)"),
    }
    let ok = report.problems == 0;
    report.say(match (ok, options.uninstall) {
        (true, false) => "\nDone. Start Axe-Grid from the shortcut on your desktop.",
        (true, true) => "\nDone.",
        (false, _) => "\nFinished with problems; see the lines marked with ! above.",
    });
    pause(options.quiet);
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_parse() {
        let args = |list: &[&str]| list.iter().map(|a| (*a).to_owned()).collect::<Vec<_>>();
        assert_eq!(parse(&[]), Ok(Options { uninstall: false, quiet: false, desktop: true }));
        let all = parse(&args(&["uninstall", "--quiet", "--no-desktop"]));
        assert_eq!(all, Ok(Options { uninstall: true, quiet: true, desktop: false }));
        assert!(parse(&args(&["--force"])).is_err());
    }

    // Windows paths only come apart and join up as expected on Windows.
    #[cfg(windows)]
    #[test]
    fn shortcuts_point_at_the_installed_app() {
        let app = install_dir(Path::new(r"C:\Users\O'Brien\AppData\Local")).join(APP_EXE);
        let script = shortcut_script("Desktop", &app);
        assert!(script.contains("GetFolderPath('Desktop')"));
        // A quote in the user's name must not end the string early.
        assert!(script.contains(r"$s.TargetPath='C:\Users\O''Brien\AppData\Local\Programs\Axe-Grid\axegrid.exe'"));
        assert!(script.contains(r"$s.WorkingDirectory='C:\Users\O''Brien\AppData\Local\Programs\Axe-Grid'"));
        assert!(remove_shortcut_script("Programs").contains("'Axe-Grid.lnk'"));
        assert_eq!(shortcut_folders(true), ["Programs", "Desktop"]);
        assert_eq!(shortcut_folders(false), ["Programs"]);
    }

    #[cfg(windows)]
    #[test]
    fn the_installed_apps_entry_can_uninstall() {
        let entry = uninstall_entry(Path::new(r"C:\Apps\Axe-Grid"));
        let value = |name: &str| entry.iter().find(|(n, _)| *n == name).map(|(_, v)| v.as_str());
        assert_eq!(value("UninstallString"), Some(r#""C:\Apps\Axe-Grid\axegrid-setup.exe" uninstall"#));
        assert_eq!(value("DisplayVersion"), Some(env!("CARGO_PKG_VERSION")));
    }
}
