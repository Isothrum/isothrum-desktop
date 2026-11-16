#!/usr/bin/env node
// The app's check, the one command CI runs (ci.yml on pull requests into dev, gate.yml before
// main and before a release). Each step runs whatever the others did; the end is one line per
// step and a non-zero exit when any is red.
//
//   npm run check                 every step
//   npm run check -- --only fmt,clippy
//   npm run check -- --list       the steps, one per line
//   npm run check:real            the tests that need Docker (and Vagrant): not a step, not in CI
import { spawnSync } from "node:child_process";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(import.meta.dirname, "..");
const manifest = ["--manifest-path", "src-tauri/Cargo.toml"];
const npm = process.platform === "win32" ? "npm.cmd" : "npm";

/** Every step, in the order they run: the bundled editor first, which the crate embeds. */
export const STEPS = [
  { name: "editor", about: "the bundled editor, from @isothrum/editor (the crate embeds it)", run: [npm, "run", "editor"] },
  { name: "fmt", about: "rustfmt", run: ["cargo", "fmt", ...manifest, "--check"] },
  { name: "clippy", about: "clippy, any warning fails", run: ["cargo", "clippy", ...manifest, "--all-targets", "--", "-D", "warnings"] },
  { name: "test", about: "the app's tests (those needing Docker or Vagrant are check:real's)", run: ["cargo", "test", ...manifest] },
];

/** The steps `args` asks for: `--only a,b`, else every one. */
export function wanted(args) {
  const at = args.indexOf("--only");
  if (at === -1) return STEPS;
  const names = (args[at + 1] ?? "").split(",").filter(Boolean);
  const unknown = names.filter((n) => !STEPS.some((s) => s.name === n));
  if (unknown.length > 0) throw new Error(`no such step: ${unknown.join(", ")} (npm run check -- --list)`);
  return STEPS.filter((s) => names.includes(s.name));
}

function main(args) {
  if (args.includes("--list")) {
    for (const s of STEPS) console.log(`${s.name.padEnd(8)} ${s.about}`);
    return 0;
  }
  const results = [];
  for (const step of wanted(args)) {
    console.log(`\n=== check ${step.name}`);
    const started = Date.now();
    const [program, ...rest] = step.run;
    const ran = spawnSync(program, rest, { cwd: root, stdio: "inherit", shell: process.platform === "win32" && program === npm });
    results.push({ name: step.name, ok: ran.status === 0, seconds: Math.round((Date.now() - started) / 1000) });
  }
  console.log("\n== check summary");
  for (const r of results) console.log(`${r.name.padEnd(8)} ${r.ok ? "ok" : "FAIL"} (${r.seconds}s)`);
  const red = results.some((r) => !r.ok);
  console.log(red ? "== check red" : "== check green");
  return red ? 1 : 0;
}

// Run, not imported (a test imports STEPS and wanted).
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exit(main(process.argv.slice(2)));
}
