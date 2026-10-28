import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { readdirSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const frontend = join(root, "frontend");
const pkg = JSON.parse(readFileSync(join(frontend, "package.json"), "utf8"));
const declared = { ...pkg.dependencies, ...pkg.devDependencies };

assert.equal(typeof declared["@tauri-apps/api"], "string", "File → Exit imports @tauri-apps/api/window");
assert.match(declared["@tauri-apps/api"], /^[\^~]?2(\.|$)/);
assert.match(declared["@tauri-apps/cli"], /^[\^~]?2(\.|$)/);
const lock = JSON.parse(readFileSync(join(frontend, "package-lock.json"), "utf8"));
assert.equal(
  lock.packages["node_modules/@tauri-apps/api"].version,
  lock.packages["node_modules/@tauri-apps/cli"].version,
);

const require = createRequire(join(frontend, "package.json"));
const resolved = require.resolve("@tauri-apps/api/window");
assert.match(resolved, /@tauri-apps[/\\]api[/\\]window\.(js|cjs)$/);

function packageName(specifier) {
  if (specifier.startsWith(".") || specifier.startsWith("/")) return null;
  if (specifier.startsWith("@")) return specifier.split("/").slice(0, 2).join("/");
  return specifier.split("/")[0];
}

function specifiers(source) {
  const found = [];
  for (const match of source.matchAll(/from\s+["']([^"']+)["']|import\(\s*["']([^"']+)["']/g)) {
    found.push(match[1] ?? match[2]);
  }
  return found;
}

function typescriptFiles(dir) {
  const files = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) {
      if (entry.name === "node_modules" || entry.name === "dist" || entry.name === "src-tauri") continue;
      files.push(...typescriptFiles(path));
    } else if (entry.name.endsWith(".ts")) {
      files.push(path);
    }
  }
  return files;
}

const missing = [];
for (const file of typescriptFiles(frontend)) {
  for (const specifier of specifiers(readFileSync(file, "utf8"))) {
    const name = packageName(specifier);
    if (name && !(name in declared)) missing.push(`${file} imports ${name}`);
  }
}
assert.deepEqual(missing, []);

console.log("frontend-deps.test.mjs ok");
