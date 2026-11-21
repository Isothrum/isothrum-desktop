#!/usr/bin/env node
// The app's tests that need this computer's real tools (`#[ignore = "needs Docker"]` or
// `"needs Vagrant"` in src-tauri/src), each run alone. Not a step of `check` and not in CI: the
// workspace's lab runs (`ws test --on labs`) run them and record each one for `ws matrix`.
//
//   npm run check:real                       every one
//   npm run check:real -- --results <file>   each one's outcome as a line of JSON added to <file>
//   npm run check:real -- --list             their names, one per line
//
// A test whose tool doesn't answer here (`docker info`, `vagrant --version`) is skipped, saying
// so; the bundled editor, which the crate embeds, is built first.
import { spawnSync } from "node:child_process";
import { appendFileSync, readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(import.meta.dirname, "..");
const src = join(root, "src-tauri", "src");
const npm = process.platform === "win32" ? "npm.cmd" : "npm";

/** Every Rust source under `dir`. */
function sources(dir) {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) return sources(path);
    return name.endsWith(".rs") ? [path] : [];
  });
}

/** Each real test of `text` (a source at `path`, under src/): its libtest path and the tool it needs. */
export function realTestsOf(path, text) {
  const module = path.replace(/\.rs$/, "").split(sep).join("::").replace(/::(mod|lib)$/, "");
  const found = [];
  const lines = text.split("\n");
  lines.forEach((line, i) => {
    const needs = /#\[ignore = "needs (\w+)"\]/.exec(line)?.[1];
    if (!needs) return;
    const fn = lines.slice(i + 1).find((l) => !l.trim().startsWith("#["));
    const name = fn && /fn (\w+)/.exec(fn)?.[1];
    if (name) found.push({ name, path: `${module}::tests::${name}`, needs });
  });
  return found;
}

/** Every real test of the crate, by name. */
export function realTests() {
  return sources(src)
    .flatMap((path) => realTestsOf(relative(src, path), readFileSync(path, "utf8")))
    .sort((a, b) => a.name.localeCompare(b.name));
}

/** Whether `tool` answers here. */
function answers(tool) {
  const probe = { Docker: ["docker", ["info"]], Vagrant: ["vagrant", ["--version"]] }[tool];
  if (!probe) return false;
  return spawnSync(probe[0], probe[1], { stdio: "ignore" }).status === 0;
}

/** Why a test failed, from its output `out`: the line after its panic, else its last line. */
export function reason(out) {
  const lines = out.split("\n").map((l) => l.trim()).filter(Boolean);
  const panic = lines.findIndex((l) => l.includes("panicked at"));
  return (panic === -1 ? lines.at(-1) : lines[panic + 1]) ?? "it failed";
}

function main(args) {
  const tests = realTests();
  if (args.includes("--list")) {
    for (const t of tests) console.log(t.name);
    return 0;
  }
  const at = args.indexOf("--results");
  const results = at === -1 ? null : args[at + 1];
  const record = (r) => results && appendFileSync(results, `${JSON.stringify({ suite: "real", provider: "", form: "", ...r })}\n`);
  const editor = spawnSync(npm, ["run", "editor"], { cwd: root, stdio: "inherit", shell: process.platform === "win32" });
  if (editor.status !== 0) {
    for (const t of tests) record({ case: t.name, result: "fail", reason: "the bundled editor wasn't built (npm run editor)", seconds: 0 });
    return 1;
  }
  let red = false;
  for (const t of tests) {
    if (!answers(t.needs)) {
      console.log(`skip ${t.name}: needs ${t.needs}, which doesn't answer here`);
      record({ case: t.name, result: "skip", reason: `needs ${t.needs}, which doesn't answer here`, seconds: 0 });
      continue;
    }
    const started = Date.now();
    const ran = spawnSync("cargo", ["test", "--manifest-path", "src-tauri/Cargo.toml", "--lib", "--", "--ignored", "--exact", t.path], { cwd: root, encoding: "utf8" });
    const out = `${ran.stdout ?? ""}${ran.stderr ?? ""}`;
    const ok = ran.status === 0 && out.includes("1 passed");
    const seconds = Math.round((Date.now() - started) / 1000);
    console.log(`${ok ? "pass" : "FAIL"} ${t.name} (${seconds}s)`);
    if (!ok) {
      red = true;
      console.log(out);
    }
    record({ case: t.name, result: ok ? "pass" : "fail", reason: ok ? "" : reason(out), seconds });
  }
  return red ? 1 : 0;
}

// Run, not imported.
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exit(main(process.argv.slice(2)));
}
