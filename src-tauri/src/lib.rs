//! Isothrum desktop: the editor from isothrum.com in a window, plus what a browser can't do:
//! running a lab on this machine and capturing its traffic. The editor finds these commands on
//! `window.__TAURI__` and shows Run, Stop and Capture only here.

mod capabilities;
mod captures;
mod commands;
mod console;
mod lab_name;
mod runner;
mod terminal;
#[cfg(test)]
mod test_support;
mod tools;

use std::path::PathBuf;

use serde::Serialize;

use captures::Captures;
use runner::RunError;
use tauri::{AppHandle, Manager, State, WebviewUrl, WebviewWindowBuilder};

// The window shows the editor bundled with the app (`apps/desktop/editor`, built from the website
// by `npm run editor`): it works offline. `ISOTHRUM_URL` points it at a site instead (isothrum.com
// for accounts and saved projects, or a local build while developing).
//
// Each command finds the app's data folder and runs its body (`commands.rs`, where it is tested)
// off the window's thread; the editor gets an error as its text.

fn data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path().app_data_dir().map_err(|e| e.to_string())
}

/// Runs `body` off the window's thread.
async fn blocking<T: Send + 'static>(
    body: impl FnOnce() -> Result<T, RunError> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(body)
        .await
        .map_err(|e| e.to_string())?
        .map_err(String::from)
}

#[tauri::command]
async fn desktop_info(app: AppHandle) -> Result<commands::DesktopInfo, String> {
    let data = data_dir(&app)?;
    blocking(move || Ok(commands::desktop_info(&data))).await
}

/// One of the lab's own files (a provisioning script, an appliance's configuration).
#[derive(serde::Deserialize)]
struct LabFile {
    path: String,
    contents: String,
}

/// Writes the lab (its isoloom.yml, its own files and the generated ones) and starts it.
#[tauri::command]
async fn environment_up(
    app: AppHandle,
    name: String,
    yaml: String,
    files: Option<Vec<LabFile>>,
) -> Result<String, String> {
    let data = data_dir(&app)?;
    let own: Vec<(String, String)> = files
        .unwrap_or_default()
        .into_iter()
        .map(|f| (f.path, f.contents))
        .collect();
    blocking(move || commands::environment_up(&data, &name, &yaml, &own)).await
}

#[tauri::command]
async fn environment_down(app: AppHandle, name: String) -> Result<String, String> {
    let data = data_dir(&app)?;
    blocking(move || commands::environment_down(&data, &app.state::<Captures>(), &name)).await
}

#[tauri::command]
async fn environment_status(app: AppHandle, name: String) -> Result<String, String> {
    let data = data_dir(&app)?;
    blocking(move || commands::environment_status(&data, &name)).await
}

/// The environment now: each machine's state, and each machine's port on each network, up or down
/// (read from the machines). Empty when the environment hasn't been started.
#[tauri::command]
async fn environment_live(
    app: AppHandle,
    name: String,
) -> Result<Option<runner::EnvironmentLive>, String> {
    let data = data_dir(&app)?;
    blocking(move || commands::environment_live(&data, &name)).await
}

/// Captures a LAN's or a cable's traffic (`network` is its name in isoloom.yml).
#[tauri::command]
async fn capture_start(
    app: AppHandle,
    name: String,
    network: String,
) -> Result<commands::CaptureStarted, String> {
    let data = data_dir(&app)?;
    blocking(move || commands::capture_start(&data, &app.state::<Captures>(), &name, &network))
        .await
}

#[tauri::command]
fn capture_stop(
    captures: State<'_, Captures>,
    name: String,
    network: String,
) -> Result<(), String> {
    Ok(commands::capture_stop(&captures, &name, &network)?)
}

/// Opens a terminal on one machine of the running lab (a shell, or SSH to a network appliance).
#[tauri::command]
async fn machine_console(app: AppHandle, name: String, machine: String) -> Result<(), String> {
    let data = data_dir(&app)?;
    let (title, console) =
        blocking(move || commands::machine_console(&data, &name, &machine)).await?;
    Ok(terminal::open_terminal(&title, &console).await?)
}

/// Opens one machine's screen (GNS3's VNC console): a container's VNC server, or a VM's display.
/// Returns what was opened (a `vnc://` address, or the VM's window).
#[tauri::command]
async fn machine_display(app: AppHandle, name: String, machine: String) -> Result<String, String> {
    let data = data_dir(&app)?;
    blocking(move || commands::machine_display(&data, &name, &machine)).await
}

/// Starts, stops or restarts one machine of the running lab.
#[tauri::command]
async fn machine_action(
    app: AppHandle,
    name: String,
    machine: String,
    action: String,
) -> Result<String, String> {
    let data = data_dir(&app)?;
    blocking(move || commands::machine_action(&data, &name, &machine, &action)).await
}

/// The lab's suspended links (a machine's port on a network, cut).
#[tauri::command]
fn link_list(app: AppHandle, name: String) -> Result<Vec<runner::Link>, String> {
    Ok(commands::link_list(&data_dir(&app)?, &name)?)
}

/// Suspends or resumes a link of the running lab: the ports of `machines` on `network` (a
/// cable's two ends, or one machine's port on a LAN), or of every machine on it.
#[tauri::command]
async fn link_action(
    app: AppHandle,
    name: String,
    network: String,
    machines: Option<Vec<String>>,
    action: String,
) -> Result<Vec<runner::Link>, String> {
    let data = data_dir(&app)?;
    blocking(move || commands::link_action(&data, &name, &network, machines, &action)).await
}

/// The lab's snapshots.
#[tauri::command]
fn snapshot_list(app: AppHandle, name: String) -> Result<Vec<runner::Snapshot>, String> {
    Ok(commands::snapshot_list(&data_dir(&app)?, &name)?)
}

/// Saves one machine of the running lab.
#[tauri::command]
async fn snapshot_take(
    app: AppHandle,
    name: String,
    machine: String,
    label: String,
) -> Result<runner::Snapshot, String> {
    let data = data_dir(&app)?;
    blocking(move || commands::snapshot_take(&data, &name, &machine, &label)).await
}

/// Runs a machine from one of its snapshots (`id`), or from its own image again (no `id`).
#[tauri::command]
async fn snapshot_use(
    app: AppHandle,
    name: String,
    machine: String,
    id: Option<String>,
) -> Result<String, String> {
    let data = data_dir(&app)?;
    blocking(move || commands::snapshot_use(&data, &name, &machine, id.as_deref())).await
}

#[tauri::command]
async fn snapshot_delete(app: AppHandle, name: String, id: String) -> Result<(), String> {
    let data = data_dir(&app)?;
    blocking(move || commands::snapshot_delete(&data, &name, &id)).await
}

/// A newer release, if there is one (the updater checks GitHub's latest release, signed with
/// the app's key).
#[derive(Serialize)]
struct UpdateInfo {
    version: String,
    notes: Option<String>,
}

#[tauri::command]
async fn update_check(app: AppHandle) -> Result<Option<UpdateInfo>, String> {
    use tauri_plugin_updater::UpdaterExt;
    let update = app
        .updater()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| e.to_string())?;
    Ok(update.map(|u| UpdateInfo {
        version: u.version,
        notes: u.body,
    }))
}

/// Downloads and installs the newer release, then restarts the app on it.
#[tauri::command]
async fn update_install(app: AppHandle) -> Result<(), String> {
    use tauri_plugin_updater::UpdaterExt;
    let Some(update) = app
        .updater()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| e.to_string())?
    else {
        return Ok(());
    };
    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(|e| e.to_string())?;
    app.restart();
}

pub fn run() {
    tools::complete_path();
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(Captures::<runner::Capture>::default())
        .invoke_handler(tauri::generate_handler![
            desktop_info,
            environment_up,
            environment_down,
            environment_status,
            environment_live,
            capture_start,
            capture_stop,
            machine_console,
            machine_display,
            machine_action,
            link_list,
            link_action,
            snapshot_list,
            snapshot_take,
            snapshot_use,
            snapshot_delete,
            update_check,
            update_install
        ])
        .setup(|app| {
            capabilities::allow_local_site(app)?;
            open_main_window(app)
        })
        .build(tauri::generate_context!())
        .expect("Isothrum desktop failed to start")
        .run(on_event);
}

/// The window: the bundled editor, or the site `ISOTHRUM_URL` names.
fn open_main_window(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let page = match std::env::var("ISOTHRUM_URL") {
        Ok(url) => WebviewUrl::External(url.parse().map_err(|e| format!("ISOTHRUM_URL: {e}"))?),
        Err(_) => WebviewUrl::App("index.html".into()),
    };
    WebviewWindowBuilder::new(app, "main", page)
        .title("Isothrum")
        .inner_size(1440.0, 900.0)
        .min_inner_size(900.0, 600.0)
        .build()?;
    Ok(())
}

/// Leaving the app ends every capture it started.
fn on_event(app: &AppHandle, event: tauri::RunEvent) {
    if let tauri::RunEvent::Exit = event
        && let Some(c) = app.try_state::<Captures>()
    {
        c.stop_all();
    }
}
