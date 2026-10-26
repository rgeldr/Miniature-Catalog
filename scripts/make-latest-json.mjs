import { readFileSync, writeFileSync, existsSync } from "node:fs";
import { join } from "node:path";

const conf = JSON.parse(readFileSync("frontend/src-tauri/tauri.conf.json", "utf8"));
const version = conf.version;
const name = conf.productName ?? "Miniature Catalog";
const dir = join("frontend", "src-tauri", "target", "release", "bundle", "nsis");
const exe = `${name}_${version}_x64-setup.exe`;
const sigPath = join(dir, `${exe}.sig`);

if (!existsSync(sigPath)) {
  console.error(`Missing ${sigPath}. Build first with signing keys set.`);
  process.exit(1);
}

// GitHub turns spaces in uploaded asset names into dots
const asset = exe.replace(/ /g, ".");
const manifest = {
  version,
  notes: process.env.RELEASE_NOTES ?? `Release ${version}`,
  pub_date: new Date().toISOString(),
  platforms: {
    "windows-x86_64-nsis": {
      signature: readFileSync(sigPath, "utf8").trim(),
      url: `https://github.com/rgeldr/Miniature-Catalog/releases/download/${version}/${asset}`,
    },
  },
};

writeFileSync(join(dir, "latest.json"), JSON.stringify(manifest, null, 2));
console.log(`Wrote ${join(dir, "latest.json")}`);
