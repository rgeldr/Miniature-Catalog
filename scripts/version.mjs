import { readFileSync, writeFileSync } from "node:fs";
import { pathToFileURL } from "node:url";

const versionPattern = /^(\d{2})\.(0[1-9]|1[0-2])\.([1-9]\d*)$/;

const rootName = "miniature-catalog";
const uiName = "miniature-catalog-ui";
const desktopName = "miniature-catalog-desktop";

const files = {
  pkg: new URL("../package.json", import.meta.url),
  lock: new URL("../package-lock.json", import.meta.url),
  uiPkg: new URL("../frontend/package.json", import.meta.url),
  uiLock: new URL("../frontend/package-lock.json", import.meta.url),
  tauri: new URL("../frontend/src-tauri/tauri.conf.json", import.meta.url),
  cargo: new URL("../Cargo.toml", import.meta.url),
  cargoLock: new URL("../Cargo.lock", import.meta.url),
  desktopCargo: new URL("../frontend/src-tauri/Cargo.toml", import.meta.url),
  desktopCargoLock: new URL("../frontend/src-tauri/Cargo.lock", import.meta.url),
};

function read(url) {
  return readFileSync(url, "utf8");
}

function currentPrefix() {
  const now = new Date();
  const year = String(now.getFullYear() % 100).padStart(2, "0");
  const month = String(now.getMonth() + 1).padStart(2, "0");
  return `${year}.${month}`;
}

function cargoVersion(text) {
  return text.match(/^version = "([^"]+)"/m)?.[1];
}

function namedVersions(text, pattern) {
  return [...text.matchAll(pattern)].map((match) => match[1]);
}

function oneVersion(found) {
  if (found.length === 0) return undefined;
  const unique = new Set(found);
  if (unique.size !== 1) return undefined;
  return found[0];
}

function npmLockVersion(text, name) {
  const pattern = new RegExp(`"name": "${escapeRegExp(name)}",\\r?\\n\\s+"version": "([^"]+)"`, "g");
  const found = namedVersions(text, pattern);
  if (found.length < 2) return undefined;
  return oneVersion(found);
}

function cargoLockVersion(text, name) {
  const pattern = new RegExp(`name = "${escapeRegExp(name)}"\\r?\\nversion = "([^"]+)"`, "g");
  const found = namedVersions(text, pattern);
  if (found.length !== 1) return undefined;
  return found[0];
}

function versions() {
  const uiLock = read(files.uiLock);
  const rootLock = read(files.lock);
  const cargoLock = read(files.cargoLock);
  const desktopLock = read(files.desktopCargoLock);
  return {
    "package.json": JSON.parse(read(files.pkg)).version,
    "package-lock.json": npmLockVersion(rootLock, rootName),
    "frontend/package.json": JSON.parse(read(files.uiPkg)).version,
    "frontend/package-lock.json": npmLockVersion(uiLock, uiName),
    "frontend/src-tauri/tauri.conf.json": JSON.parse(read(files.tauri)).version,
    "Cargo.toml": cargoVersion(read(files.cargo)),
    "Cargo.lock": cargoLockVersion(cargoLock, rootName),
    "frontend/src-tauri/Cargo.toml": cargoVersion(read(files.desktopCargo)),
    "frontend/src-tauri/Cargo.lock (miniature-catalog)": cargoLockVersion(desktopLock, rootName),
    "frontend/src-tauri/Cargo.lock (miniature-catalog-desktop)": cargoLockVersion(desktopLock, desktopName),
  };
}

function fail(message) {
  console.error(message);
  process.exit(1);
}

function agreedVersion() {
  const found = versions();
  const values = Object.values(found);
  if (values.some((value) => value == null)) {
    fail(`Could not read a project version.\n${Object.entries(found).map(([file, value]) => `  ${file}: ${value ?? "missing"}`).join("\n")}`);
  }
  const unique = new Set(values);
  if (unique.size !== 1) {
    fail(`Versions differ:\n${Object.entries(found).map(([file, value]) => `  ${file}: ${value}`).join("\n")}`);
  }
  const version = values[0];
  if (!versionPattern.test(version)) {
    fail(`${version} is not YY.MM.N. Example: 26.10.1`);
  }
  return version;
}

function check() {
  const version = agreedVersion();
  const prefix = version.slice(0, 5);
  const expected = currentPrefix();
  if (prefix !== expected) {
    fail(`${version} is not this month (${expected}). Run npm run version:bump`);
  }
  console.log(version);
}

export function escapeRegExp(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

export function replaceVersionNear(text, anchor, current, next, maxGapLines = 6) {
  const newline = text.includes("\r\n") ? "\r\n" : "\n";
  const lines = text.split(/\r?\n/);
  const anchorIndex = lines.findIndex((line) => line.includes(anchor));
  if (anchorIndex < 0) {
    throw new Error(`Could not find ${JSON.stringify(anchor)}`);
  }
  const needle = `"version": "${current}"`;
  const limit = Math.min(lines.length, anchorIndex + maxGapLines + 1);
  for (let i = anchorIndex; i < limit; i += 1) {
    if (!lines[i].includes(needle)) continue;
    lines[i] = lines[i].replace(needle, `"version": "${next}"`);
    return lines.join(newline);
  }
  throw new Error(`Could not find version ${current} near ${JSON.stringify(anchor)}`);
}

export function replaceOnce(text, from, to) {
  const pattern = from.split("\n").map(escapeRegExp).join("\\r?\\n");
  const match = new RegExp(pattern).exec(text);
  if (!match) {
    throw new Error(`Could not find ${JSON.stringify(from)}`);
  }
  const newline = match[0].includes("\r\n") ? "\r\n" : "\n";
  const replacement = to.split("\n").join(newline);
  return text.slice(0, match.index) + replacement + text.slice(match.index + match[0].length);
}

function updateNpmLock(text, name, current, next) {
  let lock = replaceOnce(
    text,
    `"name": "${name}",\n  "version": "${current}"`,
    `"name": "${name}",\n  "version": "${next}"`,
  );
  lock = replaceOnce(
    lock,
    `"name": "${name}",\n      "version": "${current}"`,
    `"name": "${name}",\n      "version": "${next}"`,
  );
  return lock;
}

function bump() {
  const current = agreedVersion();
  const match = versionPattern.exec(current);
  const prefix = currentPrefix();
  const next = `${match[1]}.${match[2]}` === prefix
    ? `${prefix}.${Number(match[3]) + 1}`
    : `${prefix}.1`;

  let updates;
  try {
    const desktopLock = replaceOnce(
      read(files.desktopCargoLock),
      `name = "${rootName}"\nversion = "${current}"`,
      `name = "${rootName}"\nversion = "${next}"`,
    );
    updates = [
      [files.pkg, replaceVersionNear(read(files.pkg), `"name": "${rootName}"`, current, next)],
      [files.lock, updateNpmLock(read(files.lock), rootName, current, next)],
      [files.uiPkg, replaceVersionNear(read(files.uiPkg), `"name": "${uiName}"`, current, next)],
      [files.uiLock, updateNpmLock(read(files.uiLock), uiName, current, next)],
      [files.tauri, replaceOnce(
        read(files.tauri),
        `"productName": "Miniature Catalog",\n  "version": "${current}"`,
        `"productName": "Miniature Catalog",\n  "version": "${next}"`,
      )],
      [files.cargo, replaceOnce(read(files.cargo), `version = "${current}"`, `version = "${next}"`)],
      [files.desktopCargo, replaceOnce(read(files.desktopCargo), `version = "${current}"`, `version = "${next}"`)],
      [files.cargoLock, replaceOnce(
        read(files.cargoLock),
        `name = "${rootName}"\nversion = "${current}"`,
        `name = "${rootName}"\nversion = "${next}"`,
      )],
      [files.desktopCargoLock, replaceOnce(
        desktopLock,
        `name = "${desktopName}"\nversion = "${current}"`,
        `name = "${desktopName}"\nversion = "${next}"`,
      )],
    ];
  } catch (err) {
    fail(err instanceof Error ? err.message : String(err));
  }
  for (const [url, text] of updates) writeFileSync(url, text);
  console.log(`${current} -> ${next}`);
}

const isMain = process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href;
if (isMain) {
  const command = process.argv[2];
  if (command === "check") check();
  else if (command === "bump") bump();
  else fail("Use: node scripts/version.mjs check|bump");
}
