// Writes the JavaScript the extensions ship from the TypeScript upstream
// keeps: each NAME.ts listed here becomes NAME.js beside it, types removed in
// place (Node's type stripping), so line numbers match the .ts. Edit the .ts,
// then run `node tools/strip-types.mjs`; CI fails when a .js is stale.
//
//   node tools/strip-types.mjs [--check]

import { readFileSync, writeFileSync } from "node:fs";
import { stripTypeScriptTypes } from "node:module";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const files = ["ext/http/00_serve.ts", "ext/http/02_websocket.ts"];
const check = process.argv.includes("--check");

let stale = 0;
for (const ts of files) {
  const js = ts.replace(/\.ts$/, ".js");
  const source = readFileSync(join(root, ts), "utf8");
  // The note joins the first line, so the .js keeps the .ts line numbers and
  // still ends with the IIFE call core's script loader expects.
  const stripped = stripTypeScriptTypes(source, { mode: "strip" });
  const newline = stripped.indexOf("\n");
  const note = ` Generated from ${ts.split("/").pop()} by tools/strip-types.mjs; do not edit.`;
  const out = stripped.slice(0, newline) + note + stripped.slice(newline);
  if (check) {
    let current = "";
    try {
      current = readFileSync(join(root, js), "utf8");
    } catch {}
    if (current !== out) {
      console.error(`${js} is stale: run node tools/strip-types.mjs`);
      stale++;
    }
  } else {
    writeFileSync(join(root, js), out);
  }
}
process.exit(stale ? 1 : 0);
