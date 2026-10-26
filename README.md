# Miniature Catalog

Self-hosted catalog of a physical tabletop miniature collection: what each figure is, how many you have, and the exact drawer, box, or tray it lives in.

The catalog is its own application. An encounter generator (or anything else) can ask it for a prep list over HTTP. Nothing in this repo imports or links against that other app, and this app starts with no generator running.

## What you need

- [rustup](https://rustup.rs/) (the `rust-toolchain.toml` in this repo selects the compiler)
- Node.js 22 or newer, only to build the search UI
- A browser on the same machine

## Run it

```bash
cd frontend
npm install
npm run build
cd ..

export MC_ADMIN_TOKEN="$(openssl rand -hex 24)"
export MC_SEED=1
cargo run
```

Open http://127.0.0.1:8080 and paste `MC_ADMIN_TOKEN` into the token field.

`MC_SEED=1` loads an example collection the first time the database is empty (a garage cabinet and an office shelf). Later runs leave your data alone. Delete the `data/` directory to start over.

### While changing the UI

Terminal 1:

```bash
export MC_ADMIN_TOKEN="local-dev-token-value"
export MC_SEED=1
cargo run
```

Terminal 2:

```bash
cd frontend
npm install
npm run dev
```

The dev server is http://127.0.0.1:5173 and proxies `/api` to the Rust process.

### Tests

```bash
cargo test
cd frontend && npm run typecheck
```

## Configuration

| Variable | Default | Purpose |
| --- | --- | --- |
| `MC_ADMIN_TOKEN` | required, at least 16 characters | Bootstrap admin bearer token. It is kept only in the environment. Restart with a new value to rotate it. |
| `MC_DATA_DIR` | `data` | SQLite file and thumbnail files |
| `MC_BIND` | `127.0.0.1:8080` | Listen address. It binds to localhost so it is not exposed on the network unless you change this. |
| `MC_STATIC_DIR` | `frontend/dist` | Built UI. If the directory is missing, the API still serves. |
| `MC_SEED` | unset | `1` inserts the example collection when the database has no locations |
| `MC_CORS_ORIGINS` | unset | Comma-separated browser origins, only if another web app calls this API from a page. A server-side client does not need CORS. |

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

The server downscales the image to a JPEG that fits in 256×256 and stores that file under `data/thumbs/`. The original is discarded. The database stores the file name, not the image bytes.

Google Photos links are rejected. Photos does not give a stable image download without OAuth, and a full Google OAuth client is the wrong amount of machinery for a single-user catalog. Put the photo in Drive and share that file, or upload it from disk.

## API authentication

Every `/api/v1` route requires `Authorization: Bearer <token>`.

- The `MC_ADMIN_TOKEN` value is an admin token.
- Admin tokens can change miniatures, locations, thumbnails, and keys.
- Read tokens can list miniatures, tags, and locations, and can call `POST /api/v1/prep`. They cannot write.

Create a read token for another app (this returns the secret once):

```bash
curl -s -X POST http://127.0.0.1:8080/api/v1/api-keys \
  -H "Authorization: Bearer $MC_ADMIN_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"name":"encounter-generator","scope":"read"}'
```

Revoke it:

```bash
curl -s -X DELETE http://127.0.0.1:8080/api/v1/api-keys/1 \
  -H "Authorization: Bearer $MC_ADMIN_TOKEN"
```

The same screens exist in the UI under API keys. Revoked keys and unknown keys both get `401`. A read key that calls a write route gets `403`.

Keys in the database are stored as SHA-256 hashes. The environment admin token is not in the database, so it is not revoked by the DELETE route; change the variable and restart.

`GET /health` and `GET /openapi.yaml` are public and contain no catalog data.

The contract is `openapi.yaml` at the repo root, also served at `/openapi.yaml`.

## Connect an encounter generator

Run this catalog and the generator as two processes. Point the generator at this base URL and the read token. Do not import this code into the generator.

Example prep call. Requests are filled in the order given. Earlier requests keep the figures they take. The reply groups picks by location path so you can pull them in one walk per container. `shortages` lists anything wanted but missing or short, including a complete miss (`available` is 0).

```bash
curl -s -X POST http://127.0.0.1:8080/api/v1/prep \
  -H "Authorization: Bearer $CATALOG_READ_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "requests": [
      {"label":"front line","races":["orc"],"combat":["melee"],"size":"28mm","quantity":6},
      {"label":"caster","roles":["wizard","mage"],"quantity":2}
    ]
  }'
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

If the generator is a server, that is the whole connection. If it is a browser page on another origin, set `MC_CORS_ORIGINS` to that origin. Prefer a server-side call so the read token is not shipped to a browser.

## Assumptions that affect behavior

- Prep allocation is greedy and stable (location path, then id). It is not a global optimizer. Put the scarcest request first if two requests can use the same pile.
- A miniature has one size and one storage location. Combat style can be more than one tag, because a figure can be both melee and magic.
- Location kind is free text with suggested values, so a shelf or binder does not need a schema change.
