//! Running a lab on this machine, and capturing its traffic.
//!
//! A lab is the `isoloom.yml` the editor writes. Running it: Isoloom's library generates the
//! Docker Compose files into the lab's folder, and `docker compose` brings them up. Capturing:
//! a throwaway container joins the network namespace of one machine on the LAN or cable, finds
//! the interface on that network, and streams `tcpdump` to Wireshark (or to a `.pcap` file):
//! GNS3's "capture on a link", for a lab of containers.
//!
//! A lab with VMs runs through Isoloom's Vagrant output, every machine a VM: `vagrant.rs`. The lab's
//! folder records which (`runtime`).

mod capture;
mod display;
mod lab_files;
mod lifecycle;
mod links;
mod live;
mod machines;
mod snapshots;
mod vagrant;

use std::path::{Path, PathBuf};
use std::process::Command;

use isoloom_spec::model::{Spec, Target};
use isoloom_targets::Lab;

pub use capture::{CAPTURE_IMAGE, Capture, live_capture_possible, plan_capture, start_capture};
pub use display::open_display;
pub use lifecycle::{down, prepare, status, up};
pub use links::{Link, editor_links, link_action, suspended_links};
pub use live::{EnvironmentLive, environment_live};
pub use machines::{console_command, container, machine_action, unknown_machine_action};
pub use snapshots::{Snapshot, delete_snapshot, snapshots, take_snapshot, use_snapshot};

/// Why a command couldn't do what the editor asked. The editor shows its text; the tests match
/// its variant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunError {
    /// The YAML isn't an Isoloom spec, or Isoloom finds problems in it (in its words).
    Spec(String),
    /// What the editor sent as a lab's name isn't one (it becomes a folder's name).
    LabName(String),
    /// One of the lab's own files would be written outside its folder.
    NotInsideLab(String),
    /// No local target runs the lab: neither Docker (a machine has no container form) nor
    /// Vagrant (a machine has no VM form).
    NoLocalTarget,
    /// The lab has VMs, and Vagrant isn't installed.
    VagrantMissing,
    /// The lab's folder has no isoloom.yml: it was never started.
    NotStarted,
    NoMachine(String),
    NoNetwork(String),
    /// A network's block isn't IPv4 (its interface can't be found by address).
    NotIpv4(String),
    NotOnNetwork {
        machine: String,
        network: String,
    },
    /// No machine (or, `containers`, no container) is on the network.
    NobodyOn {
        network: String,
        containers: bool,
    },
    /// The machine has no running container (or VM).
    NotRunning(String),
    /// A network appliance without an address on Isoloom's management network.
    NoManagementAddress(String),
    /// The machine shows no screen: no VNC server in its image, no network, or a VM whose
    /// provider shows none from here.
    NoScreen {
        machine: String,
        why: NoScreen,
    },
    UnknownAction {
        action: String,
        /// The actions there are, as the message lists them.
        expected: &'static str,
    },
    NoSnapshot {
        id: String,
        /// The machine it was asked for, when one was.
        machine: Option<String>,
    },
    /// The machine runs from this snapshot: it can't be deleted.
    SnapshotInUse(String),
    Io(String),
    /// A command failed; its output says why.
    Command {
        what: String,
        output: String,
    },
}

/// Why a machine shows no screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoScreen {
    /// A container whose image runs no VNC server.
    NoVncServer,
    /// A container on no network.
    NoNetwork,
    /// A VM whose provider gives no display address.
    NoDisplay,
    /// A VM that isn't running, or whose provider shows nothing from here.
    NotShown,
}

impl std::fmt::Display for RunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RunError::Spec(e) | RunError::Io(e) => write!(f, "{e}"),
            RunError::LabName(name) => write!(
                f,
                "`{name}` isn't a lab name (lowercase letters, digits and dashes)"
            ),
            RunError::NotInsideLab(path) => write!(f, "`{path}` isn't a path inside the lab"),
            RunError::NoLocalTarget => write!(
                f,
                "This lab runs neither as containers nor as local VMs: export it and run it with \
                 `isoloom run` on a target that fits it."
            ),
            RunError::VagrantMissing => write!(
                f,
                "This lab has VMs: they run with Vagrant (and VirtualBox or libvirt), which isn't \
                 installed. Install it from vagrantup.com, or export the lab."
            ),
            RunError::NotStarted => write!(f, "start the lab first"),
            RunError::NoMachine(m) => write!(f, "no machine named `{m}`"),
            RunError::NoNetwork(n) => write!(f, "the lab has no network `{n}`"),
            RunError::NotIpv4(cidr) => write!(f, "`{cidr}` isn't an IPv4 block"),
            RunError::NotOnNetwork { machine, network } => {
                write!(f, "`{machine}` isn't on `{network}`")
            }
            RunError::NobodyOn {
                network,
                containers,
            } => {
                let who = if *containers { "container" } else { "machine" };
                write!(f, "no {who} is on `{network}`")
            }
            RunError::NotRunning(m) => write!(f, "`{m}` isn't running: start the lab first"),
            RunError::NoManagementAddress(m) => write!(f, "`{m}` has no management address"),
            RunError::NoScreen { machine, why } => match why {
                NoScreen::NoVncServer => write!(
                    f,
                    "`{machine}` has no screen: its image runs no VNC server (none of its ports \
                     is in 5900–5999). Open its console instead."
                ),
                NoScreen::NoNetwork => write!(f, "`{machine}` is on no network"),
                NoScreen::NoDisplay => write!(f, "`{machine}` has no display"),
                NoScreen::NotShown => write!(
                    f,
                    "`{machine}` isn't running, or its provider shows no screen from here \
                     (VirtualBox and libvirt do): open its console instead"
                ),
            },
            RunError::UnknownAction { action, expected } => {
                write!(f, "unknown action `{action}`: {expected}")
            }
            RunError::NoSnapshot { id, machine } => match machine {
                Some(m) => write!(f, "no snapshot `{id}` of `{m}`"),
                None => write!(f, "no snapshot `{id}`"),
            },
            RunError::SnapshotInUse(m) => write!(
                f,
                "`{m}` runs from this snapshot: bring it back to its own image first"
            ),
            RunError::Command { what, output } => write!(f, "{what} failed:\n{output}"),
        }
    }
}

/// The editor gets a command's error as its text.
impl From<RunError> for String {
    fn from(e: RunError) -> String {
        e.to_string()
    }
}

impl std::error::Error for RunError {}

/// How a lab runs: Docker Compose, or Vagrant (every machine a VM, a container machine in its VM
/// form).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Runtime {
    Docker,
    Vagrant,
}

impl Runtime {
    /// The Isoloom target whose files it runs.
    pub fn target(self) -> Target {
        match self {
            Runtime::Docker => Target::Docker,
            Runtime::Vagrant => Target::Vagrant,
        }
    }

    /// How the lab's folder records it.
    fn name(self) -> &'static str {
        match self {
            Runtime::Docker => "docker",
            Runtime::Vagrant => "vagrant",
        }
    }
}

/// Where the lab's folder records its runtime.
const RUNTIME: &str = "runtime";

/// The runtime the lab was prepared for (Docker when nothing says otherwise).
pub fn runtime(dir: &Path) -> Runtime {
    match std::fs::read_to_string(dir.join(RUNTIME))
        .unwrap_or_default()
        .trim()
    {
        // "hybrid": a folder an earlier version prepared (Isoloom's hybrid output, gone in 3.0).
        // Its files aren't Vagrant's own folder's, so it reads as not prepared: started again,
        // it is prepared anew.
        "vagrant" | "hybrid" => Runtime::Vagrant,
        _ => Runtime::Docker,
    }
}

/// Whether the lab has been prepared (its generated files are there).
pub fn prepared(dir: &Path) -> bool {
    match runtime(dir) {
        Runtime::Docker => dir.join(".isoloom/docker/compose.yml").is_file(),
        Runtime::Vagrant => vagrant::dir(dir).join("Vagrantfile").is_file(),
    }
}

/// A lab's folder under the app's data directory: `<data>/labs/<name>/`.
pub fn lab_dir(data: &Path, name: &str) -> PathBuf {
    data.join("labs").join(name)
}

/// `docker compose` on the lab's file, from its folder.
pub fn compose(dir: &Path, args: &[&str]) -> Command {
    let mut c = Command::new("docker");
    c.current_dir(dir)
        .args(["compose", "-f", ".isoloom/docker/compose.yml"]);
    // Machines brought back from a snapshot run its image (see `restore_snapshot`).
    if dir.join(snapshots::SNAPSHOT_OVERRIDE).is_file() {
        c.args(["-f", snapshots::SNAPSHOT_OVERRIDE]);
    }
    c.args(args);
    c
}

/// Runs a command to the end; its output becomes the error when it fails.
pub fn run(mut cmd: Command, what: &str) -> Result<String, RunError> {
    let out = cmd.output().map_err(|e| RunError::Command {
        what: what.into(),
        output: e.to_string(),
    })?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    if out.status.success() {
        Ok(text)
    } else {
        Err(RunError::Command {
            what: what.into(),
            output: text,
        })
    }
}

/// The spec's network that the editor names `name`. Isoloom flattens a switch's VLANs into
/// networks of their own (`sw1.vlan10` becomes `sw1-vlan10`), while the editor names a VLAN's
/// LAN `vlan10` (a name it gives one switch only); a plain LAN or cable keeps its name.
pub fn spec_network(spec: &Spec, name: &str) -> Result<String, RunError> {
    if spec.networks.contains_key(name) {
        return Ok(name.to_string());
    }
    let (lan, vlan) = match name.split_once('.') {
        Some((lan, vlan)) => (Some(lan), vlan),
        None => (None, name),
    };
    let id: Option<u16> = vlan.strip_prefix("vlan").and_then(|v| v.parse().ok());
    spec.networks
        .iter()
        .find(|(_, n)| {
            n.vlan
                .as_ref()
                .is_some_and(|(l, i)| Some(*i) == id && lan.is_none_or(|lan| lan == l))
        })
        .map(|(flat, _)| flat.clone())
        .ok_or_else(|| RunError::NoNetwork(name.into()))
}

/// The editor's name for one of the spec's networks: `vlan10` for a VLAN's, else its own.
pub fn editor_network(spec: &Spec, network: &str) -> String {
    match spec.networks.get(network).and_then(|n| n.vlan.as_ref()) {
        Some((_, id)) => format!("vlan{id}"),
        None => network.to_string(),
    }
}

/// The lab in `yaml`, Isoloom's format: the parser's problems when it can't read it.
pub fn lab_of(yaml: &str) -> Result<Lab, RunError> {
    Lab::parse(yaml).map_err(|problems| RunError::Spec(lines(&problems)))
}

/// Problems as Isoloom words them, one after the other.
pub fn lines(problems: &[isoloom_spec::syntax::Problem]) -> String {
    let list: Vec<String> = problems.iter().map(ToString::to_string).collect();
    list.join("\n")
}

/// The spec in `yaml` as `runtime` runs it: lowered for its target (each machine in the form that
/// target gives it, a switch's VLANs as networks of their own).
pub fn spec_for(yaml: &str, runtime: Runtime) -> Result<Spec, RunError> {
    Ok(lab_of(yaml)?.for_target(runtime.target()))
}

/// The lab's spec as last started (its folder's isoloom.yml), as its runtime runs it.
pub fn lab_spec(dir: &Path) -> Result<Spec, RunError> {
    let yaml =
        std::fs::read_to_string(dir.join("isoloom.yml")).map_err(|_| RunError::NotStarted)?;
    spec_for(&yaml, runtime(dir))
}
