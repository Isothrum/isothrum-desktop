//! What each of the editor's commands does, as a function of the app's data folder (and of the
//! captures in progress). `lib.rs`'s `#[tauri::command]`s only find that folder and run these
//! off the window's thread, so each command is tested here, without a window.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::captures::{Captures, Stop};
use crate::console::Console;
use crate::lab_name::checked;
use crate::runner::{self, RunError};

/// The folder of the lab the editor names `name`, once the name is checked.
fn lab(data: &Path, name: &str) -> Result<PathBuf, RunError> {
    Ok(runner::lab_dir(data, checked(name)?))
}

#[derive(Debug, Serialize)]
pub struct DesktopInfo {
    pub version: &'static str,
    /// Whether `docker` answers (installed, running, and this user may use it).
    pub docker: bool,
    /// Whether Vagrant is installed (labs with VMs run with it).
    pub vagrant: bool,
    pub wireshark: bool,
    /// Wireshark can show captures live (its `dumpcap` is usable by this account).
    pub live_capture: bool,
    /// Where the labs' folders are.
    pub labs: String,
}

/// The app's version, the tools it found, and where it keeps the labs.
pub fn desktop_info(data: &Path) -> DesktopInfo {
    let docker = std::process::Command::new("docker")
        .args(["info", "--format", "{{.ServerVersion}}"])
        .output()
        .is_ok_and(|o| o.status.success());
    DesktopInfo {
        version: env!("CARGO_PKG_VERSION"),
        docker,
        vagrant: crate::tools::has("vagrant"),
        wireshark: crate::tools::has("wireshark"),
        live_capture: runner::live_capture_possible(),
        labs: data.join("labs").display().to_string(),
    }
}

/// Writes the lab (its isoloom.yml, its own files at their paths, the generated ones) and
/// starts it.
pub fn environment_up(
    data: &Path,
    name: &str,
    yaml: &str,
    own: &[(String, String)],
) -> Result<String, RunError> {
    let dir = lab(data, name)?;
    runner::prepare(&dir, yaml, own)?;
    runner::up(&dir)
}

/// Stops the lab, and its captures first. A lab never started has nothing to stop.
pub fn environment_down<C: Stop>(
    data: &Path,
    captures: &Captures<C>,
    name: &str,
) -> Result<String, RunError> {
    let dir = lab(data, name)?;
    captures.stop_lab(checked(name)?);
    if !runner::prepared(&dir) {
        return Ok(String::new());
    }
    runner::down(&dir)
}

/// The lab's machines and their state (`docker compose ps` JSON lines); empty when it has never
/// been started.
pub fn environment_status(data: &Path, name: &str) -> Result<String, RunError> {
    let dir = lab(data, name)?;
    if !runner::prepared(&dir) {
        return Ok(String::new());
    }
    runner::status(&dir)
}

/// The lab now: each machine's state, each machine's port on each network. `None` when it has
/// never been started.
pub fn environment_live(
    data: &Path,
    name: &str,
) -> Result<Option<runner::EnvironmentLive>, RunError> {
    let dir = lab(data, name)?;
    if !runner::prepared(&dir) {
        return Ok(None);
    }
    runner::environment_live(&dir, &runner::lab_spec(&dir)?).map(Some)
}

#[derive(Debug, Serialize)]
pub struct CaptureStarted {
    /// Where the packets are written when Wireshark isn't installed; `None`: Wireshark is open.
    pub file: Option<String>,
}

/// Where a capture without Wireshark goes: `<data>/captures/<lab>-<network>-<seconds>.pcap`.
pub fn capture_file(data: &Path, lab: &str, network: &str, at: std::time::SystemTime) -> PathBuf {
    let seconds = at
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    data.join("captures")
        .join(format!("{lab}-{network}-{seconds}.pcap"))
}

/// Captures a LAN's or a cable's traffic (`network`: its name in the editor); a capture of the
/// same network before it is stopped.
pub fn capture_start(
    data: &Path,
    captures: &Captures,
    name: &str,
    network: &str,
) -> Result<CaptureStarted, RunError> {
    let dir = lab(data, name)?;
    let plan = runner::plan_capture(&dir, &runner::lab_spec(&dir)?, network)?;
    let file = capture_file(data, name, network, std::time::SystemTime::now());
    let capture = runner::start_capture(&plan, &file)?;
    let file = capture.file.as_ref().map(|f| f.display().to_string());
    captures.add(name, network, capture);
    Ok(CaptureStarted { file })
}

pub fn capture_stop<C: Stop>(
    captures: &Captures<C>,
    name: &str,
    network: &str,
) -> Result<(), RunError> {
    captures.stop(checked(name)?, network);
    Ok(())
}

/// What a terminal on one machine of the running lab runs, and its title.
pub fn machine_console(
    data: &Path,
    name: &str,
    machine: &str,
) -> Result<(String, Console), RunError> {
    let dir = lab(data, name)?;
    let console = runner::console_command(&dir, &runner::lab_spec(&dir)?, machine)?;
    Ok((format!("{machine} ({name})"), console))
}

/// Opens one machine's screen; returns what was opened.
pub fn machine_display(data: &Path, name: &str, machine: &str) -> Result<String, RunError> {
    let dir = lab(data, name)?;
    runner::open_display(&dir, &runner::lab_spec(&dir)?, machine)
}

/// Starts, stops or restarts one machine of the running lab.
pub fn machine_action(
    data: &Path,
    name: &str,
    machine: &str,
    action: &str,
) -> Result<String, RunError> {
    let dir = lab(data, name)?;
    runner::machine_action(&dir, &runner::lab_spec(&dir)?, machine, action)
}

/// The lab's suspended links, by the editor's names for their networks (by the spec's when the
/// lab's spec can't be read).
pub fn link_list(data: &Path, name: &str) -> Result<Vec<runner::Link>, RunError> {
    let dir = lab(data, name)?;
    let links = runner::suspended_links(&dir);
    Ok(match runner::lab_spec(&dir) {
        Ok(spec) => runner::editor_links(&spec, links),
        Err(_) => links,
    })
}

/// Suspends or resumes the ports of `machines` (or of every machine) on `network`.
pub fn link_action(
    data: &Path,
    name: &str,
    network: &str,
    machines: Option<Vec<String>>,
    action: &str,
) -> Result<Vec<runner::Link>, RunError> {
    let dir = lab(data, name)?;
    runner::link_action(&dir, &runner::lab_spec(&dir)?, network, machines, action)
}

pub fn snapshot_list(data: &Path, name: &str) -> Result<Vec<runner::Snapshot>, RunError> {
    Ok(runner::snapshots(&lab(data, name)?))
}

/// Saves one machine of the running lab.
pub fn snapshot_take(
    data: &Path,
    name: &str,
    machine: &str,
    label: &str,
) -> Result<runner::Snapshot, RunError> {
    let dir = lab(data, name)?;
    if !runner::lab_spec(&dir)?.machines.contains_key(machine) {
        return Err(RunError::NoMachine(machine.into()));
    }
    runner::take_snapshot(&dir, name, machine, label)
}

/// Runs a machine from one of its snapshots (`id`), or from its own image again (no `id`).
pub fn snapshot_use(
    data: &Path,
    name: &str,
    machine: &str,
    id: Option<&str>,
) -> Result<String, RunError> {
    runner::use_snapshot(&lab(data, name)?, machine, id)
}

pub fn snapshot_delete(data: &Path, name: &str, id: &str) -> Result<(), RunError> {
    runner::delete_snapshot(&lab(data, name)?, id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{LabYaml, TempDir};

    /// A capture that records that it was stopped.
    #[derive(Default)]
    struct Fake(std::sync::Arc<std::sync::atomic::AtomicBool>);

    impl Stop for Fake {
        fn stop(&mut self) {
            self.0.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }

    /// A lab of two containers on a LAN, prepared (written, not started) in `data` as `name`.
    fn prepared_lab(data: &Path, name: &str) -> PathBuf {
        let dir = runner::lab_dir(data, name);
        let yaml = LabYaml::named(name)
            .switch("sw1", "10.80.0.0/19", &[(10, "10.80.10.0/24")])
            .machine("pc", &[("sw1.vlan10", 10)])
            .machine("srv", &[("sw1.vlan10", 11)])
            .build();
        runner::prepare(&dir, &yaml, &[]).unwrap();
        dir
    }

    #[test]
    fn every_command_refuses_a_name_that_isnt_a_lab_name() {
        let data = TempDir::new("commands-names");
        let captures = Captures::<Fake>::default();
        let bad = "../x";
        let refused = RunError::LabName(bad.into());
        let results: [(&str, Result<(), RunError>); 15] = [
            (
                "environment_up",
                environment_up(&data, bad, "", &[]).map(drop),
            ),
            (
                "environment_down",
                environment_down(&data, &captures, bad).map(drop),
            ),
            (
                "environment_status",
                environment_status(&data, bad).map(drop),
            ),
            ("environment_live", environment_live(&data, bad).map(drop)),
            (
                "capture_start",
                capture_start(&data, &Captures::default(), bad, "a").map(drop),
            ),
            ("capture_stop", capture_stop(&captures, bad, "a")),
            (
                "machine_console",
                machine_console(&data, bad, "pc").map(drop),
            ),
            (
                "machine_display",
                machine_display(&data, bad, "pc").map(drop),
            ),
            (
                "machine_action",
                machine_action(&data, bad, "pc", "stop").map(drop),
            ),
            ("link_list", link_list(&data, bad).map(drop)),
            (
                "link_action",
                link_action(&data, bad, "a", None, "suspend").map(drop),
            ),
            ("snapshot_list", snapshot_list(&data, bad).map(drop)),
            (
                "snapshot_take",
                snapshot_take(&data, bad, "pc", "").map(drop),
            ),
            (
                "snapshot_use",
                snapshot_use(&data, bad, "pc", None).map(drop),
            ),
            ("snapshot_delete", snapshot_delete(&data, bad, "pc-1")),
        ];
        for (command, result) in results {
            assert_eq!(result, Err(refused.clone()), "{command}");
        }
        assert_eq!(
            std::fs::read_dir(&*data).unwrap().count(),
            0,
            "nothing written"
        );
    }

    #[test]
    fn a_lab_never_started_reads_as_empty_and_refuses_what_needs_it_running() {
        let data = TempDir::new("commands-unstarted");
        let lab = "never";
        // What the editor polls: nothing yet.
        assert_eq!(environment_status(&data, lab), Ok(String::new()));
        assert_eq!(environment_live(&data, lab), Ok(None));
        assert_eq!(link_list(&data, lab), Ok(vec![]));
        assert!(snapshot_list(&data, lab).unwrap().is_empty());
        // What needs the lab: to be started first.
        let not_started: [(&str, Result<(), RunError>); 6] = [
            (
                "capture_start",
                capture_start(&data, &Captures::default(), lab, "a").map(drop),
            ),
            (
                "machine_console",
                machine_console(&data, lab, "pc").map(drop),
            ),
            (
                "machine_display",
                machine_display(&data, lab, "pc").map(drop),
            ),
            (
                "machine_action",
                machine_action(&data, lab, "pc", "stop").map(drop),
            ),
            (
                "link_action",
                link_action(&data, lab, "a", None, "suspend").map(drop),
            ),
            (
                "snapshot_take",
                snapshot_take(&data, lab, "pc", "").map(drop),
            ),
        ];
        for (command, result) in not_started {
            assert_eq!(result, Err(RunError::NotStarted), "{command}");
        }
        assert_eq!(
            snapshot_use(&data, lab, "pc", Some("pc-1")),
            Err(RunError::NoSnapshot {
                id: "pc-1".into(),
                machine: Some("pc".into())
            })
        );
    }

    #[test]
    fn stopping_a_lab_stops_its_captures_even_when_it_never_ran() {
        let data = TempDir::new("commands-down");
        let captures = Captures::<Fake>::default();
        let (ours, other) = (Fake::default(), Fake::default());
        let (ours_stopped, other_stopped) = (ours.0.clone(), other.0.clone());
        captures.add("lab", "office", ours);
        captures.add("other", "office", other);
        assert_eq!(environment_down(&data, &captures, "lab"), Ok(String::new()));
        assert!(ours_stopped.load(std::sync::atomic::Ordering::SeqCst));
        assert!(!other_stopped.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[test]
    fn a_lab_is_refused_before_anything_is_written_or_run() {
        let data = TempDir::new("commands-up");
        assert!(matches!(
            environment_up(&data, "bad", "version: 1\nname: x\nmachines: {}\n", &[]),
            Err(RunError::Spec(_))
        ));
        let yaml = LabYaml::named("own")
            .network("a", "10.80.1.0/24")
            .machine("pc", &[("a", 10)])
            .build();
        assert_eq!(
            environment_up(&data, "own", &yaml, &[("/etc/x".into(), String::new())]),
            Err(RunError::NotInsideLab("/etc/x".into()))
        );
        assert!(!data.join("labs").exists(), "nothing written");
    }

    #[test]
    fn a_machine_the_lab_hasnt_is_refused() {
        let data = TempDir::new("commands-machine");
        prepared_lab(&data, "lab");
        let nobody = Err(RunError::NoMachine("nobody".into()));
        assert_eq!(machine_console(&data, "lab", "nobody").map(drop), nobody);
        assert_eq!(machine_display(&data, "lab", "nobody").map(drop), nobody);
        assert_eq!(
            machine_action(&data, "lab", "nobody", "stop").map(drop),
            nobody
        );
        assert_eq!(snapshot_take(&data, "lab", "nobody", "").map(drop), nobody);
        assert_eq!(
            link_action(
                &data,
                "lab",
                "vlan10",
                Some(vec!["nobody".into()]),
                "suspend"
            ),
            Err(RunError::NotOnNetwork {
                machine: "nobody".into(),
                network: "sw1-vlan10".into()
            })
        );
    }

    #[test]
    fn suspended_links_are_listed_by_the_editors_names() {
        let data = TempDir::new("commands-links");
        let dir = prepared_lab(&data, "lab");
        std::fs::write(
            dir.join("links.json"),
            r#"[{"network":"sw1-vlan10","machine":"pc"}]"#,
        )
        .unwrap();
        let link = |network: &str| runner::Link {
            network: network.into(),
            machine: "pc".into(),
        };
        assert_eq!(link_list(&data, "lab"), Ok(vec![link("vlan10")]));
        // Its spec unreadable: the names as recorded, rather than nothing.
        std::fs::remove_file(dir.join("isoloom.yml")).unwrap();
        assert_eq!(link_list(&data, "lab"), Ok(vec![link("sw1-vlan10")]));
    }

    #[test]
    fn a_vms_console_is_vagrant_ssh_titled_by_machine_and_lab() {
        let data = TempDir::new("commands-console");
        let dir = prepared_lab(&data, "lab");
        // Prepared for Vagrant: its console needs nothing running to be written.
        std::fs::write(dir.join("runtime"), "vagrant").unwrap();
        let (title, console) = machine_console(&data, "lab", "pc").unwrap();
        assert_eq!(title, "pc (lab)");
        assert_eq!(console.argv, ["vagrant", "ssh", "pc"]);
        assert_eq!(console.dir, Some(dir.join(".isoloom").join("vagrant")));
    }

    #[test]
    fn a_capture_without_wireshark_is_written_under_captures() {
        let at = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        assert_eq!(
            capture_file(Path::new("/data"), "lab", "vlan10", at),
            Path::new("/data/captures/lab-vlan10-1700000000.pcap")
        );
    }

    #[test]
    fn the_editor_is_told_where_the_labs_are() {
        let info = desktop_info(Path::new("/data"));
        assert_eq!(Path::new(&info.labs), Path::new("/data/labs"));
        assert_eq!(info.version, env!("CARGO_PKG_VERSION"));
    }
}
