//! A lab's life: its folder prepared from the spec, started, read, stopped.

use std::path::Path;

use isoloom_spec::model::Spec;
use isoloom_targets::{GeneratedFile, Lab};

use super::lab_files;
use super::{RUNTIME, RunError, Runtime, compose, lab_of, lines, run, runtime};
use crate::tools::has;

/// The runtime that fits the lab and Isoloom's files for it: Docker when every machine has a
/// container form, else Vagrant (every machine a VM).
pub fn generated(lab: &Lab) -> Result<(Runtime, Vec<GeneratedFile>), RunError> {
    [Runtime::Docker, Runtime::Vagrant]
        .into_iter()
        .find_map(|rt| {
            isoloom_targets::generate(lab, rt.target())
                .ok()
                .map(|f| (rt, f))
        })
        .ok_or(RunError::NoLocalTarget)
}

/// Checks the spec and writes it with its generated files into `dir`: Docker's when every machine
/// is a container, else Vagrant's (every machine a VM).
/// `own`: the lab's own files (provisioning scripts, appliance configurations), at the paths
/// isoloom.yml gives them (relative, inside the lab's folder).
pub fn prepare(dir: &Path, yaml: &str, own: &[(String, String)]) -> Result<Spec, RunError> {
    if let Some((path, _)) = own.iter().find(|(p, _)| !lab_files::inside_lab(p)) {
        return Err(RunError::NotInsideLab(path.clone()));
    }
    let lab = valid_lab(yaml)?;
    let (runtime, generated) = generated(&lab)?;
    runnable(runtime, has("vagrant"))?;
    write_lab(dir, yaml, runtime, &generated, own)?;
    Ok(lab.for_target(runtime.target()))
}

/// Whether this machine can run a lab on `runtime`: Vagrant's needs it installed.
fn runnable(runtime: Runtime, vagrant_installed: bool) -> Result<(), RunError> {
    if runtime == Runtime::Vagrant && !vagrant_installed {
        return Err(RunError::VagrantMissing);
    }
    Ok(())
}

/// Writes the lab into its folder: isoloom.yml, the runtime it runs on, the generated files and
/// its own.
fn write_lab(
    dir: &Path,
    yaml: &str,
    runtime: Runtime,
    generated: &[GeneratedFile],
    own: &[(String, String)],
) -> Result<(), RunError> {
    lab_files::write(dir, "isoloom.yml", yaml)?;
    lab_files::write(dir, RUNTIME, runtime.name())?;
    let files: Vec<(&str, &str)> = generated
        .iter()
        .map(|f| (f.path.as_str(), f.contents.as_str()))
        .chain(own.iter().map(|(p, c)| (p.as_str(), c.as_str())))
        .collect();
    lab_files::write_all(dir, &files)
}

/// The lab in `yaml`, when Isoloom finds no problem in it.
fn valid_lab(yaml: &str) -> Result<Lab, RunError> {
    let lab = lab_of(yaml)?;
    let problems = lab.problems();
    if problems.is_empty() {
        return Ok(lab);
    }
    Err(RunError::Spec(lines(&problems)))
}

/// Starts the lab; the links suspended before it was started again stay cut.
pub fn up(dir: &Path) -> Result<String, RunError> {
    let mut out = if runtime(dir) == Runtime::Vagrant {
        super::vagrant::up(dir)?
    } else {
        run(
            compose(dir, &["up", "-d", "--wait", "--remove-orphans"]),
            "Starting the lab",
        )?
    };
    let notes = super::links::reapply_all_links(dir, &super::lab_spec(dir)?);
    if !notes.is_empty() {
        out.push('\n');
        out.push_str(&notes);
    }
    Ok(out)
}

/// Stops the lab: its machines are gone, and its suspended links with them.
pub fn down(dir: &Path) -> Result<String, RunError> {
    // The VNC forwarders first: on the lab's networks, they would keep Compose from removing them.
    super::display::remove_forwarders(dir);
    let out = if runtime(dir) == Runtime::Vagrant {
        super::vagrant::down(dir)?
    } else {
        run(
            compose(dir, &["down", "--volumes", "--remove-orphans"]),
            "Stopping the lab",
        )?
    };
    super::links::clear_links(dir);
    Ok(out)
}

/// The lab's services and their state, as `docker compose ps` reports them (one JSON per line).
pub fn status(dir: &Path) -> Result<String, RunError> {
    if runtime(dir) == Runtime::Vagrant {
        return super::vagrant::status(dir);
    }
    run(
        compose(dir, &["ps", "--all", "--format", "json"]),
        "Reading the lab's state",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::{lab_spec, prepared};
    use crate::test_support::{LabYaml, TempDir};

    /// Two Alpine containers on a LAN.
    fn containers() -> String {
        LabYaml::named("pair")
            .network("office", "10.10.1.0/24")
            .machine("a", &[("office", 10)])
            .machine("b", &[("office", 11)])
            .build()
    }

    /// A container machine with an OS (a VM form too) beside a Windows VM: Vagrant runs both.
    const WITH_VM: &str = r#"
version: 1
name: mixed
networks:
  office: { cidr: 10.10.1.0/24 }
machines:
  a:
    networks: { office: 10 }
    os: debian@12
  b:
    networks: { office: 11 }
    os: windows-server@2022
    app: { steps: [{ run: provision/b.ps1 }] }
"#;

    /// The lab the editor's compiler writes for a drawing (its nodes, on one LAN).
    fn compiled(nodes: &str) -> String {
        let topology = format!(
            r#"{{"name":"drawn","networks":{{"office":{{"cidr":"10.10.1.0/24"}}}},"nodes":{{{nodes}}}}}"#
        );
        let topo: isoloom_topology::Topology = serde_json::from_str(&topology).unwrap();
        isoloom_topology::compile(&topo).unwrap().into_parts().0
    }

    const WEB: &str = r#""web":{"kind":{"container":{"image":"nginx:1.27-alpine"}},"services":[{"port":80,"protocol":"http"}],"networks":{"office":null}}"#;
    const USER: &str = r#""user":{"kind":{"container":{"image":"alpine:3.20","idle":true}},"access":true,"networks":{"office":null}}"#;
    /// A Linux VM with steps but no port: Docker builds it as a container.
    const TOOLS_VM: &str = r#""tools":{"kind":{"vm":{"os":"debian@12","provision":["provision/tools.sh"]}},"networks":{"office":null}}"#;
    /// A VM whose steps install a service (a port), with nothing saying how it runs in the
    /// foreground: Isoloom refuses it as a container.
    const SERVICE_VM: &str = r#""db":{"kind":{"vm":{"os":"debian@12","provision":["provision/db.sh"]}},"services":[{"port":5432}],"networks":{"office":null}}"#;
    const KALI_VM: &str =
        r#""kali":{"kind":{"vm":{"os":"kali"}},"access":true,"networks":{"office":null}}"#;

    #[test]
    fn each_lab_runs_on_the_runtime_that_fits_it() {
        let cases: [(&str, String, Result<Runtime, RunError>); 5] = [
            ("containers", containers(), Ok(Runtime::Docker)),
            (
                "drawn containers and a VM without a port",
                compiled(&[WEB, TOOLS_VM, USER].join(",")),
                Ok(Runtime::Docker),
            ),
            (
                "an OS machine beside a Windows VM",
                WITH_VM.into(),
                Ok(Runtime::Vagrant),
            ),
            (
                "drawn VMs only",
                compiled(&[SERVICE_VM, KALI_VM].join(",")),
                Ok(Runtime::Vagrant),
            ),
            // The drawn containers are plain images, which have no VM form.
            (
                "a service VM beside plain images",
                compiled(&[WEB, SERVICE_VM, USER].join(",")),
                Err(RunError::NoLocalTarget),
            ),
        ];
        for (what, yaml, runs_on) in cases {
            let got = generated(&lab_of(&yaml).unwrap()).map(|(rt, _)| rt);
            assert_eq!(got, runs_on, "{what}");
        }
    }

    #[test]
    fn a_lab_of_containers_is_prepared_for_docker() {
        let dir = TempDir::new("prepare");
        let spec = prepare(&dir, &containers(), &[]).unwrap();
        assert_eq!(spec.name, "pair");
        // What starting, reading and stopping it use: its runtime, its Compose file, its spec.
        assert_eq!(runtime(&dir), Runtime::Docker);
        assert!(prepared(&dir));
        assert_eq!(
            lab_spec(&dir).unwrap().machines.keys().collect::<Vec<_>>(),
            ["a", "b"]
        );
        let compose: serde_yaml_ng::Value = serde_yaml_ng::from_str(
            &std::fs::read_to_string(dir.join(".isoloom/docker/compose.yml")).unwrap(),
        )
        .unwrap();
        assert_eq!(compose["name"].as_str(), Some("pair"));
        for machine in ["a", "b"] {
            assert!(compose["services"].get(machine).is_some(), "{machine}");
        }
    }

    #[test]
    fn a_drawn_lab_runs_as_the_compiler_writes_it() {
        let yaml = compiled(&[WEB, TOOLS_VM, USER].join(","));
        let dir = TempDir::new("drawn");
        let own = [("provision/tools.sh".to_owned(), "#!/bin/sh\n".to_owned())];
        let spec = prepare(&dir, &yaml, &own).unwrap();
        assert_eq!(runtime(&dir), Runtime::Docker);
        assert!(spec.machines["tools"].docker.is_some());
        assert!(dir.join(".isoloom/docker/build/tools/Dockerfile").is_file());
        assert_eq!(lab_spec(&dir).unwrap().machines.len(), 3);
    }

    #[test]
    fn a_lab_with_vms_needs_vagrant_installed() {
        assert_eq!(
            runnable(Runtime::Vagrant, false),
            Err(RunError::VagrantMissing)
        );
        assert_eq!(runnable(Runtime::Vagrant, true), Ok(()));
        assert_eq!(runnable(Runtime::Docker, false), Ok(()));
    }

    #[test]
    fn a_lab_with_vms_is_prepared_for_vagrant() {
        let dir = TempDir::new("vm");
        let lab = lab_of(WITH_VM).unwrap();
        let (rt, files) = generated(&lab).unwrap();
        write_lab(&dir, WITH_VM, rt, &files, &[]).unwrap();
        assert_eq!(runtime(&dir), Runtime::Vagrant);
        assert!(prepared(&dir), "its Vagrantfile is where the app looks");
    }

    #[test]
    fn the_labs_own_files_are_written_inside_it_only() {
        let dir = TempDir::new("own");
        prepare(
            &dir,
            &containers(),
            &[("configs/r1.cfg".into(), "hostname r1\n".into())],
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("configs/r1.cfg")).unwrap(),
            "hostname r1\n"
        );
        // Which paths are inside: lab_files' table. Here: a path outside refuses the whole lab.
        let refused = TempDir::new("own-refused");
        let err = prepare(
            &refused,
            &containers(),
            &[("../x.sh".into(), String::new())],
        );
        assert_eq!(err.unwrap_err(), RunError::NotInsideLab("../x.sh".into()));
        assert_eq!(
            std::fs::read_dir(&*refused).unwrap().count(),
            0,
            "nothing written"
        );
    }

    #[test]
    fn a_lab_isoloom_finds_problems_in_is_refused_with_them() {
        // A plain image has no VM form: beside a Windows VM, no target runs the lab.
        let image_beside_windows = WITH_VM.replace(
            "    os: debian@12\n",
            "    app: { image: \"alpine:3.20\" }\n",
        );
        let no_machine = "version: 1\nname: x\nnetworks: {}\nmachines: {}\n".to_string();
        for yaml in [image_beside_windows, no_machine] {
            let dir = TempDir::new("bad");
            match prepare(&dir, &yaml, &[]) {
                Err(RunError::Spec(problems)) => assert!(!problems.is_empty(), "{yaml}"),
                other => panic!("{other:?} for {yaml}"),
            }
            assert!(!dir.join("isoloom.yml").exists(), "nothing written");
        }
    }
}
