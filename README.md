# Miniature Catalog

Desktop catalog of a physical tabletop miniature collection: what each figure is, how many you have, and the exact drawer, box, or tray it lives in.

Launching the app opens a window. The catalog UI runs inside that window. You do not start a browser, and you do not set environment variables for normal use.

The catalog is its own application. An encounter generator (or anything else) can ask it for a prep list over HTTP while this app is running. Nothing in this repo imports or links against that other app, and this app starts with no generator running.

## Windows (PowerShell)

Install once:

- [rustup](https://rustup.rs/) with the MSVC toolchain, which needs the Visual Studio C++ build tools
- Node.js 22 or newer
- WebView2, which current Windows 10 and 11 already include

From the repo:

```powershell
cd D:\GitHub\Miniature-Catalog\frontend
npm install
npm run tauri dev
```

That builds the UI and opens the Miniature Catalog window.

A release build, from the repo root:

```powershell
cd D:\GitHub\Miniature-Catalog
npm run release
```

That checks the version and builds the desktop app. It then writes `latest.json` next to the NSIS installer, and stops if that signature file is missing. It does not create a git tag and it does not publish a GitHub release. See [Version and release](#version-and-release).

The Windows setup program is written under `frontend\src-tauri\target\release\bundle\nsis\`. An MSI is written under `bundle\msi\` when that bundler runs. `npm run tauri build` from `frontend` still builds those installers on its own.

No OpenSSL, no `export`, and no bash.

### First launch

The app creates its files in `%APPDATA%\MiniatureCatalog\MiniatureCatalog\data` (`C:\Users\<you>\AppData\Roaming\MiniatureCatalog\MiniatureCatalog\data`):

| File | What it is |
| --- | --- |
| `catalog.db` | SQLite database |
| `thumbs\` | Small JPEG thumbnails |
| `admin.token` | Admin token, so you can read it again if the window is closed |
| `settings.json` | Whether the encounter-generator API is on, and where it listens |

The window shows the admin token under **Encounter generator**. Copy it from there. The API is off until you turn it on. The default listen address is `127.0.0.1:8080`, which is this computer only.

If the database is empty, the first launch loads an example collection (a garage cabinet and an office shelf). Later launches leave your figures alone. Delete the miniatures in the app, or delete that data folder, to start blank.

### Connect an encounter generator

1. Open Miniature Catalog.
2. Check **Allow the encounter generator to connect**.
3. Leave the address as `127.0.0.1:8080` unless you have a reason to change it. An address that is not `127.0.0.1` or `[::1]` is reachable beyond this computer; the window warns you.
4. Click **Save API settings**. The status line shows the URL when the port is actually listening.
5. Point the generator at `http://127.0.0.1:8080` and either the admin token or a read key from **API keys**.

The API is a normal HTTP socket in the catalog process. The window is only the UI. Turning the API off does not close the window. Closing the app stops the API. `GET /health` on that port answers while the API is on, with or without you looking at the window.

```powershell
$token = "<admin token or read key>"
Invoke-RestMethod -Uri http://127.0.0.1:8080/health
Invoke-RestMethod -Method Post -Uri http://127.0.0.1:8080/api/v1/api-keys `
  -Headers @{ Authorization = "Bearer $token" } `
  -ContentType "application/json" `
  -Body '{"name":"encounter-generator","scope":"read"}'
```

## Other systems

The same development commands, from `frontend`:

```bash
npm install
npm run tauri dev
npm run tauri build
```

`npm run version:bump` and `npm run release` run from the repo root on every system. See [Version and release](#version-and-release).

Linux needs the WebKitGTK libraries Tauri links against. On Linux the data folder is `~/.local/share/miniaturecatalog`. On macOS it is under `~/Library/Application Support/`.

## Version and release

Versions are `YY.MM.N` (for example `26.10.1`). The same number is written in every place the app records its version:

- `package.json` and `package-lock.json`
- `frontend/package.json` and `frontend/package-lock.json`
- `Cargo.toml` and `Cargo.lock` (`miniature-catalog`)
- `frontend/src-tauri/Cargo.toml` and `frontend/src-tauri/Cargo.lock` (`miniature-catalog` and `miniature-catalog-desktop`)
- `frontend/src-tauri/tauri.conf.json`

`openapi.yaml` stays on its own version. That field is the HTTP API contract, not the desktop release.

From the repo root in PowerShell:

```powershell
npm run version:check
npm run version:bump
npm test
```

`npm run version:check` fails unless `YY.MM` is the current month. `npm run version:bump` adds one to `N` in the current month, or starts at `.1` when the month changes. `npm test` runs `scripts/version.test.mjs`. The same bump and check commands are `node scripts/version.mjs bump` and `node scripts/version.mjs check`.

`26.10.1` is a valid semver. A January version written as `26.01.1` is not, because of the leading zero. That is the same rule 3D File Library uses. `npm run tauri build` runs `npm run version:check` before it compiles, so a stale month fails the build.

```powershell
npm run release
```

`npm run release` runs `npm run tauri build` in `frontend`, then `node scripts/make-latest-json.mjs`. The second step reads `Miniature Catalog_<version>_x64-setup.exe.sig` under `frontend\src-tauri\target\release\bundle\nsis\` and writes `latest.json` beside it. The signature is the whole `.sig` file. The download URL uses the version as the tag, with no `v` prefix. Spaces in the file name become dots in that URL, because GitHub does that to uploaded asset names.

Signing keys are not in the repo. Set them in the same PowerShell session when you want `latest.json`:

```powershell
$env:TAURI_SIGNING_PRIVATE_KEY = Get-Content -Raw "C:\path\to\miniature-catalog.key"
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = "the password for that key"
npm run release
```

`TAURI_SIGNING_PRIVATE_KEY` may be the key file contents, as above, or the path to the key file. Without those variables the installer can still be built with `npm run tauri build`, and `npm run release` stops at the missing `.sig` file.

This catalog does not check for updates. Nothing in the app reads `latest.json`, and there is no updater public key. The file is only the same release artifact 3D File Library writes. `npm run release` does not create a git tag or a GitHub release.

## Tests

```powershell
npm test
cargo test
cd frontend
npm run typecheck
```

## Headless API

`cargo run` from the repo root starts only the HTTP API. It does not open a window or a browser. It uses the same data folder, creates the admin token if needed, and prints the token. It listens on `127.0.0.1:8080`.

```powershell
cargo run
```

Optional variables for that process. The desktop app does not need them.

| Variable | Default | Purpose |
| --- | --- | --- |
| `MC_ADMIN_TOKEN` | unset | When set (16 characters or more), this process uses it instead of `admin.token`. |
| `MC_DATA_DIR` | the app data folder above | Database, thumbnails, token, and settings |
| `MC_BIND` | `127.0.0.1:8080` | Headless listen address. An IP and port, not a host name. |
| `MC_STATIC_DIR` | `frontend/dist` | Built UI for the headless process. If it is missing, the API still serves. |
| `MC_CORS_ORIGINS` | unset | Comma-separated browser origins, only if a web page on another origin calls the API. A desktop generator does not need this. |

## How the collection is modeled

One catalog row is one pile of identical miniatures in one place. The quantity is how many are in that pile. The same sculpt in two boxes is two rows.

A miniature has a name, notes, quantity, one size (such as `28mm`), an optional source game, and any number of tags:

- race / type (`orc`, `goblin`, `human`, `undead`, …)
- combat style (`melee`, `ranged`, `magic`, and any other label you add)
- roles it can stand in for (`wizard`, `mage`, `sorcerer`, …)

Locations are a tree: room, cabinet, drawer, box, container, tray, shelf, or any other short label. The UI shows the full path, for example `Garage > Cabinet A > Drawer 3 > Box 2`. The storage report lists how many figures are directly in each place and how many are in it once you count everything nested inside.

Search combines filters with AND across fields and OR inside a field. Choosing location "Cabinet A" includes every drawer and box inside it. Name search is by word prefix: `wiz` matches "Human Wizard", and `orc` does not match "Sorcerer".

## Thumbnails

Upload a JPEG, PNG, or GIF, or paste a Google Drive file id / `drive.google.com` link for a file shared as **anyone with the link**.

The server downscales the image to a JPEG that fits in 256×256 and stores that file next to the database, under `thumbs\`. The original is discarded. The database stores the file name, not the image bytes.

Google Photos links are rejected. Photos does not give a stable image download without OAuth, and a full Google OAuth client is the wrong amount of machinery for a single-user catalog. Put the photo in Drive and share that file, or upload it from disk.

## API authentication

Every `/api/v1` route on the encounter-generator port requires `Authorization: Bearer <token>`.

- The admin token shown in the window can change miniatures, locations, thumbnails, keys, and the API listener.
- Read tokens can list miniatures, tags, and locations, and can call `POST /api/v1/prep`. They cannot write or change the listener.
- `GET /api/v1/desktop-session` exists only on the window's own localhost port. It is how the window learns the admin token. That route is not on the encounter-generator port.

Create a read token for another app (this returns the secret once):

```powershell
Invoke-RestMethod -Method Post -Uri http://127.0.0.1:8080/api/v1/api-keys `
  -Headers @{ Authorization = "Bearer $token" } `
  -ContentType "application/json" `
  -Body '{"name":"encounter-generator","scope":"read"}'
```

Revoke it with `DELETE /api/v1/api-keys/{id}` and the admin token. The same screen exists in the window under API keys. Revoked keys and unknown keys both get `401`. A read key that calls a write route gets `403`.

Keys in the database are stored as SHA-256 hashes. The admin token is not in the database. It is kept in `admin.token` so the window can show it again. Delete that file while the app is closed to generate a new one on the next launch. Extra keys are revoked from the API keys screen.

`GET /health` and `GET /openapi.yaml` are public and contain no catalog data.

The contract is `openapi.yaml` at the repo root, also served at `/openapi.yaml`.

## Prep list

Run this catalog and the generator as two processes. Point the generator at this base URL and a read token (or the admin token). Do not import this code into the generator.

Requests are filled in the order given. Earlier requests keep the figures they take. The reply groups picks by location path so you can pull them in one walk per container. `shortages` lists anything wanted but missing or short, including a complete miss (`available` is 0).

```powershell
$body = @'
{
  "requests": [
    {"label":"front line","races":["orc"],"combat":["melee"],"size":"28mm","quantity":6},
    {"label":"caster","roles":["wizard","mage"],"quantity":2}
  ]
}
'@
Invoke-RestMethod -Method Post -Uri http://127.0.0.1:8080/api/v1/prep `
  -Headers @{ Authorization = "Bearer $token" } `
  -ContentType "application/json" `
  -Body $body
```

Each request needs a quantity and at least one of: `name`, `races`, `roles`, `combat`, `tags`, `size`, `game`. Several values in one field mean "any of these". Fields you omit are wildcards. `tags` matches a race, role, or combat tag.

Other reads the generator can use:

- `GET /api/v1/miniatures?q=&race=&role=&combat=&size=&game=&location_id=`
- `GET /api/v1/miniatures/{id}`
- `GET /api/v1/tags`
- `GET /api/v1/locations`
- `GET /api/v1/facets`
- `GET /api/v1/reports/locations`

Repeated values in a query are comma-separated: `race=orc,goblin`.

Changing the listen address to something other than this computer is possible, but the API then accepts bearer tokens from anyone who can reach that socket. Leave it on `127.0.0.1` unless both programs are on different machines and you accept that.

## Assumptions that affect behavior

- Prep allocation is greedy and stable (location path, then id). It is not a global optimizer. Put the scarcest request first if two requests can use the same pile.
- A miniature has one size and one storage location. Combat style can be more than one tag, because a figure can be both melee and magic.
- Location kind is free text with suggested values, so a shelf or binder does not need a schema change.
- The desktop app stores the admin token in a file because the window has to show it again. Other client keys stay hashed.
- The encounter-generator port defaults to off and to `127.0.0.1`. A host name such as `localhost` is rejected; use `127.0.0.1` or `[::1]`.
- An empty database is filled with the example collection on launch so the first window is not blank.
