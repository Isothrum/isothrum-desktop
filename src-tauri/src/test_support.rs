//! What the tests share (compiled for tests only): a folder of their own, the labs they run (and
//! stop), and waiting for something by polling to a deadline.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// A fresh, empty folder, removed when dropped (also when the test fails).
pub struct TempDir(PathBuf);

impl TempDir {
    /// `<temp>/isothrum-test-<what>-<pid>-<n>`: no two tests share one, even in one process.
    pub fn new(what: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("isothrum-test-{what}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a temporary folder");
        TempDir(dir)
    }
}

impl std::ops::Deref for TempDir {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The lab prepared in a folder, started; stopped again when dropped, so that a test that
/// fails half-way leaves no container or network behind.
pub struct Started<'a>(&'a Path);

impl<'a> Started<'a> {
    pub fn up(dir: &'a Path) -> Self {
        // Made first: a lab that starts only in part is stopped too.
        let started = Started(dir);
        crate::runner::up(dir).expect("the lab starts");
        started
    }
}

impl Drop for Started<'_> {
    fn drop(&mut self) {
        let _ = crate::runner::down(self.0);
    }
}

/// Whether `check` holds within `within`: asked again every 100 ms until it does or the
/// deadline passes. For what a test waits on (a container starting, a file filling up).
pub fn eventually(within: Duration, mut check: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + within;
    loop {
        if check() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// An isoloom.yml for a test, in the shape the editor writes: networks, then Alpine machines on
/// them (`machine(name, &[(network, host)])`).
pub struct LabYaml {
    name: String,
    networks: Vec<String>,
    reach: Vec<String>,
    machines: Vec<String>,
}

impl LabYaml {
    pub fn named(name: &str) -> Self {
        LabYaml {
            name: name.into(),
            networks: Vec::new(),
            reach: Vec::new(),
            machines: Vec::new(),
        }
    }

    /// A LAN or a cable: `cidr` is its IPv4 block.
    pub fn network(mut self, name: &str, cidr: &str) -> Self {
        self.networks.push(format!("  {name}: {{ cidr: {cidr} }}"));
        self
    }

    /// A switch with VLANs: its block, and each VLAN's id and block (`sw1.vlan10` in the spec).
    pub fn switch(mut self, name: &str, cidr: &str, vlans: &[(u16, &str)]) -> Self {
        let vlans: Vec<String> = vlans
            .iter()
            .map(|(id, cidr)| format!("{id}: {{ cidr: {cidr} }}"))
            .collect();
        self.networks.push(format!(
            "  {name}: {{ cidr: {cidr}, vlans: {{ {} }} }}",
            vlans.join(", ")
        ));
        self
    }

    /// Routes from one network to another.
    pub fn reach(mut self, from: &str, to: &str) -> Self {
        self.reach.push(format!("{{ from: {from}, to: {to} }}"));
        self
    }

    /// An Alpine container on `networks` (network, host number).
    pub fn machine(self, name: &str, networks: &[(&str, u8)]) -> Self {
        self.with_machine(name, networks, "")
    }

    /// An Alpine container that idles until stopped (for a test that runs the lab).
    pub fn idle(self, name: &str, networks: &[(&str, u8)]) -> Self {
        self.with_machine(name, networks, ", command: [sleep, infinity]")
    }

    fn with_machine(mut self, name: &str, networks: &[(&str, u8)], rest: &str) -> Self {
        let nets: Vec<String> = networks.iter().map(|(n, h)| format!("{n}: {h}")).collect();
        self.machines.push(format!(
            "  {name}: {{ networks: {{ {} }}, app: {{ image: \"alpine:3.20\" }}{rest} }}",
            nets.join(", ")
        ));
        self
    }

    pub fn build(&self) -> String {
        let mut yaml = format!("version: 1\nname: {}\nnetworks:\n", self.name);
        for n in &self.networks {
            yaml.push_str(n);
            yaml.push('\n');
        }
        if !self.reach.is_empty() {
            yaml.push_str(&format!("reach: [{}]\n", self.reach.join(", ")));
        }
        yaml.push_str("machines:\n");
        for m in &self.machines {
            yaml.push_str(m);
            yaml.push('\n');
        }
        yaml
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_temporary_folder_is_its_own_and_goes_away() {
        let (a, b) = (TempDir::new("support"), TempDir::new("support"));
        assert_ne!(a.to_path_buf(), b.to_path_buf());
        let kept = a.to_path_buf();
        assert!(kept.is_dir());
        drop(a);
        assert!(!kept.exists());
    }

    #[test]
    fn eventually_is_true_once_the_check_holds_and_false_at_the_deadline() {
        let mut asked = 0;
        assert!(eventually(Duration::from_secs(5), || {
            asked += 1;
            asked == 3
        }));
        assert_eq!(asked, 3);
        assert!(!eventually(Duration::ZERO, || false));
    }

    #[test]
    fn a_built_lab_is_one_isoloom_reads() {
        let yaml = LabYaml::named("built")
            .network("a", "10.70.1.0/24")
            .network("b", "10.70.2.0/24")
            .switch("sw1", "10.70.64.0/19", &[(10, "10.70.74.0/24")])
            .reach("a", "b")
            .machine("pc", &[("a", 10), ("sw1.vlan10", 10)])
            .idle("srv", &[("b", 10)])
            .build();
        let spec = crate::runner::spec_for(&yaml, crate::runner::Runtime::Docker).unwrap();
        assert_eq!(spec.name, "built");
        assert_eq!(
            spec.machines.keys().collect::<Vec<_>>(),
            ["pc", "srv"],
            "{yaml}"
        );
        assert!(spec.networks.contains_key("sw1-vlan10"), "{yaml}");
    }
}
