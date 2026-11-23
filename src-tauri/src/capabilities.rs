//! Which pages may call the app's commands. Release builds: the bundled editor and isothrum.com
//! (`capabilities/editor.json`). Debug builds add the same permissions for a local build of the
//! site, so `ISOTHRUM_URL=http://localhost:3000` works while developing.

use serde::Deserialize;
use tauri::ipc::CapabilityBuilder;
use tauri::{Manager, Runtime};

const EDITOR: &str = include_str!("../capabilities/editor.json");
/// A local build of the site: allowed in debug builds only.
const LOCAL_SITE: &str = "http://localhost:*/*";

#[derive(Deserialize)]
struct Capability {
    windows: Vec<String>,
    permissions: Vec<String>,
}

fn editor() -> Capability {
    serde_json::from_str(EDITOR).expect("capabilities/editor.json is a capability")
}

/// The editor's permissions, for a local build of the site.
fn local_site() -> CapabilityBuilder {
    let editor = editor();
    let builder = CapabilityBuilder::new("editor-local-site")
        .local(false)
        .remote(LOCAL_SITE.into())
        .windows(editor.windows);
    editor
        .permissions
        .into_iter()
        .fold(builder, |b, p| b.permission(p))
}

/// Lets a local build of the site call the commands, in debug builds only.
pub fn allow_local_site<R: Runtime, M: Manager<R>>(app: &M) -> tauri::Result<()> {
    if cfg!(debug_assertions) {
        app.add_capability(local_site())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_builds_trust_isothrum_com_only() {
        let editor: serde_json::Value = serde_json::from_str(EDITOR).unwrap();
        assert_eq!(
            editor["remote"]["urls"],
            serde_json::json!(["https://www.isothrum.com/*"])
        );
        assert_eq!(editor["windows"], serde_json::json!(["main"]));
    }

    /// The updater's file for macOS is the `.app` bundle, archived (`Isothrum.app.tar.gz`): without
    /// the `app` target, a release has nothing for Macs to update from.
    #[test]
    fn every_platform_has_an_update_to_download() {
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        assert_eq!(conf["bundle"]["createUpdaterArtifacts"], true);
        let targets = conf["bundle"]["targets"].as_array().unwrap();
        for t in ["app", "appimage", "msi"] {
            assert!(targets.contains(&t.into()), "{t} in {targets:?}");
        }
    }

    /// The words between `start` and the next `end` in `text`, split on commas, trimmed and
    /// unquoted: a list as Rust source writes it.
    fn listed<'a>(text: &'a str, start: &str, end: &str) -> std::collections::BTreeSet<&'a str> {
        let from = text.find(start).expect(start) + start.len();
        let to = from + text[from..].find(end).expect(end);
        text[from..to]
            .split(',')
            .map(|w| w.trim().trim_matches('"'))
            .filter(|w| !w.is_empty())
            .collect()
    }

    /// What the editor can call: the commands the app registers (lib.rs), those it makes
    /// permissions for (build.rs), and those the editor's capability allows are the same 18. A
    /// command missing from one is a button that fails in the editor.
    #[test]
    fn the_editor_may_call_every_command_the_app_has_and_no_other() {
        let registered = listed(include_str!("lib.rs"), "generate_handler![", "]");
        let permitted = listed(include_str!("../build.rs"), "let commands = &[", "]");
        let allowed: std::collections::BTreeSet<String> = editor()
            .permissions
            .iter()
            .filter_map(|p| p.strip_prefix("allow-"))
            .map(|p| p.replace('-', "_"))
            .collect();
        assert_eq!(registered.len(), 18, "{registered:?}");
        assert_eq!(registered, permitted);
        assert_eq!(
            registered,
            allowed.iter().map(String::as_str).collect(),
            "capabilities/editor.json"
        );
    }

    /// The updater asks this repository's latest release, and checks it with the app's key.
    #[test]
    fn updates_come_signed_from_this_repositorys_releases() {
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let updater = &conf["plugins"]["updater"];
        assert_eq!(
            updater["endpoints"],
            serde_json::json!([
                "https://github.com/Isothrum/isothrum-desktop/releases/latest/download/latest.json"
            ])
        );
        assert!(!updater["pubkey"].as_str().unwrap_or_default().is_empty());
    }

    #[test]
    fn the_bundled_editor_runs_only_its_own_scripts() {
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let csp = conf["app"]["security"]["csp"].as_str().unwrap_or_default();
        // Directive -> its sources.
        let directives: std::collections::BTreeMap<&str, Vec<&str>> = csp
            .split(';')
            .filter_map(|d| {
                let mut words = d.split_whitespace();
                Some((words.next()?, words.collect()))
            })
            .collect();
        assert_eq!(directives["default-src"], ["'self'"]);
        assert_eq!(directives["object-src"], ["'none'"]);
        // Its own scripts and WebAssembly (the engine): nothing inline, no eval.
        assert_eq!(directives["script-src"], ["'self'", "'wasm-unsafe-eval'"]);
    }
}
