//! Labs with VMs: run through Isoloom's Vagrant output, with `vagrant` instead of `docker
//! compose`. Every machine is a VM there, a container machine in the VM form its Dockerfile or its
//! OS gives it. Machines are Vagrant's (`config.vm.define "<name>"`): a console is `vagrant ssh`,
//! starting and stopping one is `vagrant up` and `halt`, a snapshot is `vagrant snapshot`, a
//! capture is `tcpdump` over `vagrant ssh`.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::console::Console;
use crate::runner::{RunError, run};

/// The folder Isoloom wrote the Vagrantfile into.
pub fn dir(lab: &Path) -> PathBuf {
    lab.join(".isoloom").join("vagrant")
}

/// `vagrant` in the lab's Vagrant folder.
pub fn vagrant(lab: &Path, args: &[&str]) -> Command {
    let mut c = Command::new("vagrant");
    c.current_dir(dir(lab)).args(args);
    c
}

/// A shell command run as root in a VM, base64-encoded so that its quotes survive `ssh -c`.
pub fn ssh_root(lab: &Path, vm: &str, script: &str) -> Command {
    vagrant(
        lab,
        &[
            "ssh",
            vm,
            "-c",
            &format!("echo {} | base64 -d | sudo sh", base64(script.as_bytes())),
        ],
    )
}

pub fn up(lab: &Path) -> Result<String, RunError> {
    run(vagrant(lab, &["up"]), "Starting the lab's VMs")
}

pub fn down(lab: &Path) -> Result<String, RunError> {
    run(vagrant(lab, &["destroy", "-f"]), "Removing the lab's VMs")
}

/// The machines' states, in the shape the editor reads (`docker compose ps` JSON lines):
/// `{"Service": <machine>, "State": "running" | "exited"}`.
pub fn status(lab: &Path) -> Result<String, RunError> {
    let out = run(
        vagrant(lab, &["status", "--machine-readable"]),
        "Reading the lab's state",
    )?;
    Ok(states(&out))
}

/// `vagrant status --machine-readable` (`time,machine,state,<state>` lines) as JSON lines.
pub fn states(machine_readable: &str) -> String {
    machine_readable
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split(',').collect();
            (f.len() >= 4 && f[2] == "state" && !f[1].is_empty()).then(|| {
                let state = if f[3] == "running" {
                    "running"
                } else {
                    "exited"
                };
                serde_json::json!({ "Service": f[1], "State": state }).to_string()
            })
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// A console: `vagrant ssh` to the machine's VM.
pub fn console_command(lab: &Path, machine: &str) -> Console {
    Console::new(&["vagrant", "ssh", machine]).in_dir(&dir(lab))
}

/// Starts, stops or restarts one machine.
pub fn action(lab: &Path, machine: &str, action: &str) -> Result<String, RunError> {
    let (args, what): (&[&str], &str) = match action {
        "start" => (&["up", machine], "Starting the machine"),
        "stop" => (&["halt", machine], "Stopping the machine"),
        "restart" => (&["reload", machine], "Restarting the machine"),
        other => return Err(crate::runner::unknown_machine_action(other)),
    };
    run(vagrant(lab, args), what)
}

// Snapshots of a VM: Vagrant's (the provider's own: VirtualBox, libvirt...), named by the id.

pub fn snapshot_save(lab: &Path, machine: &str, id: &str) -> Result<String, RunError> {
    run(
        vagrant(lab, &["snapshot", "save", machine, id]),
        "Saving the machine",
    )
}

pub fn snapshot_restore(lab: &Path, machine: &str, id: &str) -> Result<String, RunError> {
    run(
        vagrant(lab, &["snapshot", "restore", machine, id, "--no-provision"]),
        "Restoring the machine",
    )
}

pub fn snapshot_delete(lab: &Path, machine: &str, id: &str) -> Result<String, RunError> {
    run(
        vagrant(lab, &["snapshot", "delete", machine, id]),
        "Removing the snapshot",
    )
}

/// A capture from `machine`: the container capture's script (find the interface on the network,
/// pcap on stdout) as root in the VM over `vagrant ssh`, with tcpdump installed first when the VM
/// lacks it.
pub fn capture_command(lab: &Path, machine: &str, script: &str) -> Command {
    let full = format!(
        "command -v tcpdump >/dev/null || {{ (apt-get install -y -qq tcpdump || dnf install -y -q tcpdump) >/dev/null 2>&1; }}; {script}"
    );
    ssh_root(lab, machine, &full)
}

/// Sets `machine`'s routes again after a link of its came back (the kernel dropped those
/// through it): Isoloom's `routes` provisioners. Best effort: a machine without routes has
/// nothing to set.
pub fn restore_routes(lab: &Path, machine: &str) {
    let cmd = vagrant(
        lab,
        &[
            "provision",
            machine,
            "--provision-with",
            "routes,default route",
        ],
    );
    let _ = run(cmd, "Setting the routes again");
}

/// Standard, padded base64 (one small use: no dependency for it).
fn base64(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let v = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            out.push(if i <= chunk.len() {
                A[((v >> (18 - 6 * i)) & 63) as usize] as char
            } else {
                '='
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The VMs' states reach the editor as a lab of containers' do: read by the same reader.
    #[test]
    fn vagrant_states_read_as_the_machines_states() {
        use crate::runner::live::{MachineLive, machine_states};
        use crate::runner::spec_for;
        use crate::test_support::LabYaml;
        let out = "1700000000,,metadata,provider,virtualbox\n1700000000,web,state,running\n1700000000,db,state,poweroff\n1700000000,web,state-human-short,running\n";
        let spec = spec_for(
            &LabYaml::named("vms")
                .network("a", "10.10.1.0/24")
                .machine("web", &[("a", 10)])
                .machine("db", &[("a", 11)])
                .build(),
            crate::runner::Runtime::Docker,
        )
        .unwrap();
        let state = |s: &str| MachineLive {
            state: s.into(),
            health: String::new(),
        };
        assert_eq!(
            machine_states(&spec, &states(out)),
            [
                ("db".to_string(), state("exited")),
                ("web".to_string(), state("running"))
            ]
            .into()
        );
    }

    /// RFC 4648's test vectors (section 10).
    #[test]
    fn base64_is_the_standard_padded_one() {
        for (plain, encoded) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(plain.as_bytes()), encoded, "{plain:?}");
        }
    }

    #[test]
    fn a_vms_console_and_capture_go_over_vagrant_ssh() {
        assert!(console_command(Path::new("/lab"), "pc").argv.ends_with(&[
            "vagrant".to_owned(),
            "ssh".to_owned(),
            "pc".to_owned()
        ]));
        let cap = capture_command(Path::new("/lab"), "pc", "tcpdump -w -");
        let args: Vec<String> = cap
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(&args[..3], ["ssh", "pc", "-c"]);
        assert_eq!(
            cap.get_current_dir(),
            Some(Path::new("/lab/.isoloom/vagrant"))
        );
    }

    /// Vagrant accepts the Vagrantfile the app prepares for a lab of VMs (`ISOTHRUM_VM_LAB`
    /// keeps the folder). Its machines aren't named `a` or `b`: Vagrant takes a name that
    /// begins a machine ID in its global index (hexadecimal) as that machine, so a stale entry
    /// of another lab stood in for them ("The working directory for Vagrant doesn't exist").
    #[test]
    #[ignore = "needs Vagrant"]
    fn vagrant_validates_the_prepared_lab() {
        use crate::runner::{Runtime, prepare, runtime};
        let base = std::env::var("ISOTHRUM_VM_LAB")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::env::temp_dir());
        // Playbook steps: VMs only (a script's steps would build containers too, run on Docker).
        let yaml = "version: 1\nname: vms\nnetworks:\n  office: {cidr: 10.10.1.0/24}\nmachines:\n  pc:\n    networks: {office: 10}\n    os: debian@12\n    app: {steps: [{playbook: provision/pc.yml}]}\n  server:\n    networks: {office: 11}\n    os: ubuntu@24.04\n    app: {steps: [{playbook: provision/server.yml}]}\n";
        let dir = base.join("vms");
        let own: Vec<(String, String)> = ["pc", "server"]
            .iter()
            .map(|m| {
                (
                    format!("provision/{m}.yml"),
                    "- hosts: all\n  tasks: []\n".to_string(),
                )
            })
            .collect();
        prepare(&dir, yaml, &own).unwrap();
        assert_eq!(runtime(&dir), Runtime::Vagrant);
        let out = vagrant(&dir, &["validate"]).output().unwrap();
        assert!(
            out.status.success(),
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        let st = status(&dir).unwrap();
        assert!(st.contains("\"State\":\"exited\""), "{st}");
    }
}
