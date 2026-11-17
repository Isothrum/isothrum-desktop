//! Opening the system's terminal on a console, as GNS3's consoles do: isothrum-machine's
//! terminal (`isothrum_machine::terminal::open`), the window titled as the caller says.

use std::process::{Command, Stdio};
use std::sync::Arc;

use isothrum_machine::{Context, Devices, Host};
use isothrum_runner::{ShellCommand, SystemProcesses, this_arch, this_host};

use crate::console::Console;
use crate::runner::RunError;

/// Opens the system's terminal running `console`, titled `title`.
pub async fn open_terminal(title: &str, console: &Console) -> Result<(), RunError> {
    let Some((program, args)) = console.argv.split_first() else {
        return Err(RunError::Io(
            "couldn't open a terminal: nothing to run".into(),
        ));
    };
    // A console with no folder of its own (a container's shell) runs from the temp folder.
    let cwd = console.dir.clone().unwrap_or_else(std::env::temp_dir);
    let command = ShellCommand::new(program, args, cwd);
    isothrum_machine::terminal::open(&this_machine(), title, &command, &std::env::temp_dir())
        .await
        .map_err(|e| RunError::Io(format!("couldn't open a terminal: {e}")))
}

/// This computer, its tools started as processes of this app's.
fn this_machine() -> Context {
    let os = this_host();
    let host = Host {
        os,
        arch: this_arch(),
        packages: None,
        devices: Devices::here(os),
    };
    Context::new(Arc::new(SystemProcesses::default()), host)
}

/// Opens `url` (`vnc://...`, `spice://...`) with the system's handler for it: Screen Sharing for
/// VNC on macOS, the desktop's default viewer elsewhere.
pub fn open_url(url: &str) -> Result<(), RunError> {
    let mut c = if cfg!(target_os = "macos") {
        Command::new("open")
    } else if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.args(["/C", "start", ""]);
        c
    } else {
        Command::new("xdg-open")
    };
    c.arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| RunError::Io(format!("couldn't open {url}: {e}")))
}
