//! Desktop app: the egui shell in a native window, plus the optional
//! loopback control server that lets scripts and agents drive this window.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::mpsc::Receiver;

use axegrid_automation::control::{self, Pending};
use axegrid_ui_egui::Shell;

struct App {
    shell: Shell,
    control: Option<Receiver<Pending>>,
}

impl eframe::App for App {
    fn logic(&mut self, _ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Control requests run here, on the UI thread, exactly like clicks.
        if let Some(requests) = &self.control {
            while let Ok(pending) = requests.try_recv() {
                let outcome = axegrid_automation::dispatch(&mut self.shell.engine, &pending.method, &pending.params);
                pending.respond(outcome);
            }
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.shell.ui(ui);
    }
}

/// The app icon, rendered from `assets/icon/axe-grid.svg` by `scripts/render-icon.py`.
const ICON_PNG: &[u8] = include_bytes!("../../../assets/icon/axe-grid.png");

struct Options {
    control_port: Option<u16>,
    token_file: Option<PathBuf>,
}

fn parse_args() -> Result<Options, String> {
    let mut options = Options { control_port: None, token_file: None };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--control" => {
                let port = args.next().ok_or("--control needs a port (0 picks a free one)")?;
                options.control_port = Some(port.parse().map_err(|_| format!("{port:?} is not a port"))?);
            }
            "--control-token-file" => {
                options.token_file = Some(args.next().ok_or("--control-token-file needs a path")?.into());
            }
            other => return Err(format!("unknown option {other}")),
        }
    }
    Ok(options)
}

/// Starts the control server and writes its token where only the caller
/// chose. Prints the bound address so a launcher can find a port-0 server.
fn start_control(options: &Options, ctx: egui::Context) -> Result<Option<Receiver<Pending>>, String> {
    let Some(port) = options.control_port else { return Ok(None) };
    let path = options.token_file.as_ref().ok_or("--control needs --control-token-file <path>")?;
    let token = control::new_token().map_err(|e| e.to_string())?;
    std::fs::write(path, &token).map_err(|e| format!("{}: {e}", path.display()))?;
    let (address, requests) =
        control::start_server(port, token, move || ctx.request_repaint()).map_err(|e| e.to_string())?;
    println!("control channel listening on {address}");
    Ok(Some(requests))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let options = parse_args()?;
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([1320.0, 800.0])
        .with_min_inner_size(axegrid_ui_egui::MIN_WINDOW)
        .with_title("Axe-Grid");
    // A window without its icon is still a window.
    if let Ok(icon) = eframe::icon_data::from_png_bytes(ICON_PNG) {
        viewport = viewport.with_icon(icon);
    }
    let native = eframe::NativeOptions { viewport, ..Default::default() };
    eframe::run_native(
        "Axe-Grid",
        native,
        Box::new(move |cc| {
            let control = start_control(&options, cc.egui_ctx.clone())?;
            Ok(Box::new(App { shell: Shell::new(), control }))
        }),
    )?;
    Ok(())
}
