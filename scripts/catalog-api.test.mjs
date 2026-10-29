import assert from "node:assert/strict";
import { CatalogApi, readError } from "../frontend/src/api.ts";

const calls = [];
let next = [];

globalThis.fetch = async (url, init = {}) => {
  calls.push({ url: String(url), init });
  const planned = next.shift();
  if (!planned) throw new Error(`unexpected fetch ${url}`);
  return planned;
};

function json(status, body) {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json" },
  });
}

const empty = new Response(null, { status: 204, statusText: "No Content" });
const api = new CatalogApi("secret-token");
assert.ok(api instanceof CatalogApi);

const blank = new Response("", { status: 500, statusText: "Server Error" });
assert.equal(await readError(blank), "Server Error");
const explained = json(409, { error: "a race named \"goblin\" already exists" });
assert.equal(await readError(explained), 'a race named "goblin" already exists');
const raw = new Response("not json", { status: 400, statusText: "Bad Request" });
assert.equal(await readError(raw), "not json");

next.push(json(201, { name: "Scout", used_by: 0 }));
const added = await api.addFilter({ category: "role", name: "Scout" });
assert.equal(added.name, "Scout");
assert.equal(calls.at(-1).init.method, "POST");
assert.equal(calls.at(-1).url, "/api/v1/filters");
assert.equal(calls.at(-1).init.headers.Authorization, "Bearer secret-token");
assert.deepEqual(JSON.parse(calls.at(-1).init.body), { category: "role", name: "Scout" });

next.push(json(200, { name: "Pathfinder", used_by: 0 }));
const renamed = await api.renameFilter({ category: "role", name: "Scout", new_name: "Pathfinder" });
assert.equal(renamed.name, "Pathfinder");
assert.equal(calls.at(-1).init.method, "PATCH");

next.push(empty);
assert.equal(await api.deleteFilter({ category: "role", name: "Pathfinder" }), undefined);
assert.equal(calls.at(-1).init.method, "DELETE");

next.push(json(409, { error: "2 miniatures still use this size. Reassign them before deleting." }));
await assert.rejects(api.deleteFilter({ category: "size", name: "32mm" }), /Reassign/);

next.push(empty);
assert.equal(await api.deleteLocation(4), undefined);
assert.equal(calls.at(-1).url, "/api/v1/locations/4");

next.push(empty);
assert.equal(await api.deleteMiniature(9), undefined);
assert.equal(calls.at(-1).url, "/api/v1/miniatures/9");

next.push(empty);
assert.equal(await api.revokeKey(3), undefined);
assert.equal(calls.at(-1).url, "/api/v1/api-keys/3");

next.push(json(200, { api_enabled: true, api_bind: "127.0.0.1:9090", listening: true, bound: "127.0.0.1:9090", managed: true }));
const settings = await api.updateSettings(true, "127.0.0.1:9090");
assert.equal(settings.listening, true);
assert.equal(calls.at(-1).init.method, "PUT");
assert.deepEqual(JSON.parse(calls.at(-1).init.body), { api_enabled: true, api_bind: "127.0.0.1:9090" });

next.push(json(201, { id: 8, name: "reader", scope: "read", token: "mc_abc" }));
const key = await api.createKey("reader", "read");
assert.equal(key.token, "mc_abc");
assert.equal(calls.at(-1).init.method, "POST");
assert.equal(calls.at(-1).url, "/api/v1/api-keys");

next.push(json(201, { id: 2, name: "Drawer", kind: "drawer", parent_id: 1, path: "Desk > Drawer" }));
const location = await api.createLocation({ name: "Drawer", kind: "drawer", parent_id: 1 });
assert.equal(location.path, "Desk > Drawer");

const input = {
  name: "Hero",
  notes: "",
  quantity: 1,
  size: "28mm",
  game: null,
  location_id: 2,
  races: ["elf"],
  roles: [],
  combat: ["melee"],
};
next.push(json(201, { id: 11, ...input, location_path: "Desk > Drawer", thumbnail_url: null }));
const created = await api.createMiniature(input);
assert.equal(created.id, 11);
next.push(json(200, { id: 11, ...input, name: "Veteran", location_path: "Desk > Drawer", thumbnail_url: null }));
const updated = await api.updateMiniature(11, { ...input, name: "Veteran" });
assert.equal(updated.name, "Veteran");
assert.equal(calls.at(-1).url, "/api/v1/miniatures/11");
assert.equal(calls.at(-1).init.method, "PUT");

next.push(empty);
await api.importThumbnail(11, "https://drive.google.com/file/d/1AbC-def_9012/view");
assert.equal(calls.at(-1).url, "/api/v1/miniatures/11/thumbnail/import");
assert.deepEqual(JSON.parse(calls.at(-1).init.body), {
  source: "https://drive.google.com/file/d/1AbC-def_9012/view",
});

const file = new File([Uint8Array.from([1, 2, 3])], "hero.jpg", { type: "image/jpeg" });
next.push(json(200, { thumbnail_url: "/api/v1/miniatures/11/thumbnail" }));
await api.uploadThumbnail(11, file);
const upload = calls.at(-1);
assert.equal(upload.url, "/api/v1/miniatures/11/thumbnail");
assert.equal(upload.init.method, "POST");
assert.equal(upload.init.headers.Authorization, "Bearer secret-token");
assert.ok(upload.init.body instanceof FormData);
assert.equal(upload.init.body.get("file"), file);

next.push(json(400, { error: "could not read upload" }));
await assert.rejects(api.uploadThumbnail(11, file), /could not read upload/);

assert.equal(next.length, 0);
console.log("catalog api tests passed");
