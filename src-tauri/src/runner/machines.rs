//! One machine of a running lab: its console, and starting or stopping it alone.

use std::path::Path;
use std::process::Command;

use isoloom_spec::model::Spec;

use super::{RunError, Runtime, compose, run, runtime};
use crate::console::Console;

/// The machine's container, when it runs.
pub fn running_container(dir: &Path, machine: &str) -> Result<Option<String>, RunError> {
    let id = run(compose(dir, &["ps", "-q", machine]), "Finding the machine")?
        .trim()
        .to_string();
    Ok((!id.is_empty()).then_some(id))
}

/// The machine's container in the running lab.
pub fn container(dir: &Path, machine: &str) -> Result<String, RunError> {
    running_container(dir, machine)?.ok_or_else(|| RunError::NotRunning(machine.into()))
}

/// A container's shell: bash when it has it, else sh.
pub const SHELL: &str = "command -v bash >/dev/null && exec bash || exec sh";

/// The command a console on `machine` runs: a shell in its container, or SSH to a network
/// appliance's OS (its management address; `admin`/`admin`).
pub fn console_command(dir: &Path, spec: &Spec, machine: &str) -> Result<Console, RunError> {
    let m = spec
        .machines
        .get(machine)
        .ok_or_else(|| RunError::NoMachine(machine.into()))?;
    if runtime(dir) == Runtime::Vagrant {
        return Ok(super::vagrant::console_command(dir, machine));
    }
    let id = container(dir, machine)?;
    if m.docker.as_ref().is_some_and(|d| d.appliance.is_some()) {
        return appliance_console(&id, machine);
    }
    Ok(Console::new(&[
        "docker", "exec", "-it", &id, "sh", "-c", SHELL,
    ]))
}

/// SSH to a network appliance's OS, at its management address (`admin`/`admin`), from a helper
/// container on the management network: under Docker Desktop (macOS, Windows) the host can't
/// reach container addresses.
fn appliance_console(id: &str, machine: &str) -> Result<Console, RunError> {
    let (network, ip) =
        management_address(id)?.ok_or_else(|| RunError::NoManagementAddress(machine.into()))?;
    Ok(Console::new(&[
        "docker",
        "run",
        "-it",
        "--rm",
        "--network",
        &network,
        super::CAPTURE_IMAGE,
        "ssh",
        "-o",
        "StrictHostKeyChecking=no",
        "-o",
        "UserKnownHostsFile=/dev/null",
        &format!("admin@{ip}"),
    ]))
}

/// A container's management network and its address there (Isoloom's management network).
fn management_address(id: &str) -> Result<Option<(String, String)>, RunError> {
    let mut c = Command::new("docker");
    c.args([
        "inspect",
        "-f",
        "{{range $n, $v := .NetworkSettings.Networks}}{{$n}} {{$v.IPAddress}}{{println}}{{end}}",
        id,
    ]);
    let nets = run(c, "Finding its management address")?;
    Ok(nets
        .lines()
        .find_map(|l| {
            l.split_once(' ')
                .filter(|(n, _)| n.ends_with("isoloom-mgmt"))
                .map(|(n, ip)| (n.to_string(), ip.trim().to_string()))
        })
        .filter(|(_, ip)| !ip.is_empty()))
}

/// Starts, stops or restarts one machine (and its network sidecar, which sets its routes).
pub fn machine_action(
    dir: &Path,
    spec: &Spec,
    machine: &str,
    action: &str,
) -> Result<String, RunError> {
    if !spec.machines.contains_key(machine) {
        return Err(RunError::NoMachine(machine.into()));
    }
    let out = if runtime(dir) == Runtime::Vagrant {
        super::vagrant::action(dir, machine, action)?
    } else {
        compose_action(dir, machine, action)?
    };
    // A machine comes back with its ports up: its suspended links are cut again.
    if action != "stop" {
        super::links::reapply_links(dir, spec, machine)?;
    }
    Ok(out)
}

/// Starts, stops or restarts one machine's Compose services (itself and its sidecar).
fn compose_action(dir: &Path, machine: &str, action: &str) -> Result<String, RunError> {
    const STOP: (&[&str], &str) = (&["stop"], "Stopping the machine");
    const START: (&[&str], &str) = (&["up", "-d", "--no-deps"], "Starting the machine");
    let steps: &[(&[&str], &str)] = match action {
        "start" => &[START],
        "stop" => &[STOP],
        "restart" => &[STOP, START],
        other => return Err(unknown_machine_action(other)),
    };
    let services = with_sidecar(dir, machine)?;
    let names: Vec<&str> = services.iter().map(String::as_str).collect();
    let mut out = String::new();
    for (args, what) in steps {
        out = run(compose(dir, &[args, &names[..]].concat()), what)?;
    }
    Ok(out)
}

/// The Compose service that sets a machine's routes, as Isoloom names it (3.0:
/// `isoloom-routes-<machine>`, a prefix no machine's name can take).
pub fn routes_sidecar(machine: &str) -> String {
    format!("isoloom-routes-{machine}")
}

/// What a machine can be asked to do, as `machine_action` says when asked anything else.
pub fn unknown_machine_action(action: &str) -> RunError {
    RunError::UnknownAction {
        action: action.into(),
        expected: "start, stop or restart",
    }
}

/// The machine and its network sidecar, as Compose services.
pub(super) fn with_sidecar(dir: &Path, machine: &str) -> Result<Vec<String>, RunError> {
    let services = run(
        compose(dir, &["config", "--services"]),
        "Reading the lab's services",
    )?;
    Ok(machine_services(&services, machine))
}

/// The machine and, when the lab has one (`services`: `docker compose config --services`), its
/// network sidecar.
fn machine_services(services: &str, machine: &str) -> Vec<String> {
    let sidecar = routes_sidecar(machine);
    let mut out = vec![machine.to_string()];
    if services.lines().any(|l| l.trim() == sidecar) {
        out.push(sidecar);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::prepare;
    use crate::test_support::{LabYaml, Started, TempDir};

    /// The sidecar a machine is started and stopped with is the one Isoloom writes beside it
    /// (named otherwise before Isoloom 3.0: the machine then lost its routes when started alone).
    #[test]
    fn a_machines_routes_sidecar_is_the_one_isoloom_writes() {
        let dir = TempDir::new("sidecar");
        let yaml = LabYaml::named("sidecar")
            .network("a", "10.61.1.0/24")
            .network("b", "10.61.2.0/24")
            .reach("a", "b")
            .machine("pc", &[("a", 10)])
            .machine("srv", &[("b", 10)])
            .build();
        prepare(&dir, &yaml, &[]).unwrap();
        let compose: serde_yaml_ng::Value = serde_yaml_ng::from_str(
            &std::fs::read_to_string(dir.join(".isoloom/docker/compose.yml")).unwrap(),
        )
        .unwrap();
        let services: Vec<&str> = compose["services"]
            .as_mapping()
            .unwrap()
            .keys()
            .filter_map(|k| k.as_str())
            .collect();
        assert!(
            services.contains(&routes_sidecar("pc").as_str()),
            "{services:?}"
        );
        assert_eq!(
            machine_services(&services.join("\n"), "pc"),
            ["pc".to_string(), routes_sidecar("pc")]
        );
        assert_eq!(
            machine_services("pc\nsrv\n", "pc"),
            ["pc"],
            "without routes, no sidecar"
        );
    }

    #[test]
    fn a_machine_or_an_action_the_lab_hasnt_is_refused_before_anything_runs() {
        // An empty folder: had anything run, `docker compose` would have found no Compose file
        // (or `vagrant` no Vagrantfile), and the error would be that command's.
        let dir = TempDir::new("refused");
        let yaml = LabYaml::named("refused")
            .network("a", "10.61.1.0/24")
            .machine("pc", &[("a", 10)])
            .build();
        let spec = crate::runner::spec_for(&yaml, crate::runner::Runtime::Docker).unwrap();
        let nobody = RunError::NoMachine("nobody".into());
        assert_eq!(
            machine_action(&dir, &spec, "nobody", "stop").unwrap_err(),
            nobody
        );
        assert_eq!(console_command(&dir, &spec, "nobody").unwrap_err(), nobody);
        assert_eq!(
            crate::runner::open_display(&dir, &spec, "nobody").unwrap_err(),
            nobody
        );
        for runtime in ["docker", "vagrant"] {
            std::fs::write(dir.join("runtime"), runtime).unwrap();
            assert_eq!(
                machine_action(&dir, &spec, "pc", "reboot").unwrap_err(),
                unknown_machine_action("reboot"),
                "{runtime}"
            );
        }
    }

    /// A machine's console and starting/stopping it alone.
    #[test]
    #[ignore = "needs Docker"]
    fn one_machine_stops_starts_and_has_a_console() {
        let dir = TempDir::new("machine");
        let yaml = LabYaml::named("onem")
            .network("a", "10.61.1.0/24")
            .network("b", "10.61.2.0/24")
            .reach("a", "b")
            .idle("pc", &[("a", 10)])
            .idle("srv", &[("b", 10)])
            .build();
        let spec = prepare(&dir, &yaml, &[]).unwrap();
        let _lab = Started::up(&dir);
        let running = |m: &str| {
            !run(compose(&dir, &["ps", "-q", m]), "ps")
                .unwrap()
                .trim()
                .is_empty()
        };
        let cmd = console_command(&dir, &spec, "pc").unwrap();
        assert_eq!(cmd.argv[..3], ["docker", "exec", "-it"]);
        assert_eq!(cmd.argv.last().map(String::as_str), Some(SHELL));
        machine_action(&dir, &spec, "pc", "stop").unwrap();
        let sidecar = routes_sidecar("pc");
        assert!(
            !running("pc") && !running(&sidecar),
            "the machine and its sidecar stop"
        );
        assert!(running("srv"), "the others keep running");
        machine_action(&dir, &spec, "pc", "start").unwrap();
        assert!(running("pc") && running(&sidecar));
        // Its routes came back with the sidecar.
        let routes = run(
            compose(&dir, &["exec", "-T", "pc", "ip", "route"]),
            "routes",
        )
        .unwrap();
        assert!(routes.contains("10.61.2.0/24 via"), "{routes}");
    }
}
