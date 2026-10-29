import assert from "node:assert/strict";
import { parseHTML } from "linkedom";

const { window } = parseHTML("<!doctype html><html><head></head><body><div id=\"app\"></div></body></html>");
const store = new Map();
const storage = {
  getItem(key) {
    return store.has(key) ? store.get(key) : null;
  },
  setItem(key, value) {
    store.set(key, String(value));
  },
  removeItem(key) {
    store.delete(key);
  },
};
let narrow = false;
let prefersDark = false;
window.matchMedia = (query) => ({
  matches: query.includes("max-width") ? narrow : query.includes("dark") ? prefersDark : false,
  media: query,
  addEventListener() {},
  removeEventListener() {},
});
window.innerWidth = 1280;
window.getComputedStyle = (element) => ({
  fontSize: "16px",
  getPropertyValue(name) {
    const value = element.style?.getPropertyValue?.(name) ?? "";
    return value;
  },
});
window.HTMLElement.prototype.getBoundingClientRect = function rect() {
  return { left: 10, right: 110, top: 10, bottom: 80, width: 100, height: 70 };
};
window.HTMLElement.prototype.showModal = function showModal() {
  this.open = true;
  this.setAttribute("open", "");
};
window.HTMLElement.prototype.close = function close() {
  this.open = false;
  this.removeAttribute("open");
  this.dispatchEvent(new window.Event("close"));
};
let active = null;
window.HTMLElement.prototype.focus = function focus() {
  active = this;
};
Object.defineProperty(window.HTMLElement.prototype, "tabIndex", {
  configurable: true,
  get() {
    if (this.hasAttribute("tabindex")) return Number(this.getAttribute("tabindex"));
    const tag = this.tagName;
    return tag === "BUTTON" || tag === "INPUT" || tag === "SELECT" || tag === "TEXTAREA" ? 0 : -1;
  },
  set(value) {
    this.setAttribute("tabindex", String(value));
  },
});
Object.defineProperty(window.document, "activeElement", { get: () => active, configurable: true });
window.confirm = () => true;
window.URL.createObjectURL = () => "blob:thumb";
globalThis.window = window;
globalThis.document = window.document;
globalThis.HTMLElement = window.HTMLElement;
globalThis.Node = window.Node;
globalThis.Element = window.Element;
globalThis.localStorage = storage;
globalThis.confirm = window.confirm;
globalThis.getComputedStyle = window.getComputedStyle;
globalThis.fetch = async (url, init = {}) => {
  const path = String(url);
  const method = init.method ?? "GET";
  if (path.endsWith("/api/v1/desktop-session")) {
    return json({ token: "admin-token-value-ok" });
  }
  if (path.endsWith("/api/v1/facets")) {
    return json({ races: ["orc"], roles: ["warrior"], combat: ["melee"], sizes: ["28mm"], games: ["Quest"] });
  }
  if (path.endsWith("/api/v1/locations")) {
    return json({ locations: [location] });
  }
  if (path.endsWith("/api/v1/reports/locations")) {
    return json({ locations: [report] });
  }
  if (path.includes("/thumbnail")) {
    if (method === "GET") return new Response(Uint8Array.from([0xff, 0xd8]), { status: 200 });
    return json({ thumbnail_url: path });
  }
  if (path.startsWith("/api/v1/miniatures")) {
    return json({ miniatures: [miniature] });
  }
  if (path.endsWith("/api/v1/settings")) {
    return json({
      api_enabled: false,
      api_bind: "127.0.0.1:8080",
      listening: false,
      bound: null,
      managed: true,
    });
  }
  if (path.endsWith("/api/v1/filters")) {
    if (method === "GET") return json({ groups });
    if (method === "DELETE") return new Response(null, { status: 204 });
    return json({ name: "Scout", used_by: 0 });
  }
  if (path.endsWith("/api/v1/api-keys")) {
    if (method === "GET") {
      return json({
        api_keys: [{ id: 3, name: "reader", scope: "read", created_at: "t", revoked_at: null }],
      });
    }
    return new Response(null, { status: 204 });
  }
  return new Response(`missing ${method} ${path}`, { status: 500 });
};

function keyEvent(type, key, extra = {}) {
  const event = new window.Event(type, { bubbles: true, cancelable: true });
  event.key = key;
  for (const [name, value] of Object.entries(extra)) event[name] = value;
  return event;
}

function json(body) {
  return new Response(JSON.stringify(body), {
    status: 200,
    headers: { "content-type": "application/json" },
  });
}

const location = { id: 1, parent_id: null, name: "Desk", kind: "room", path: "Desk" };
const report = {
  ...location,
  direct_entries: 1,
  direct_quantity: 2,
  total_entries: 1,
  total_quantity: 2,
};
const miniature = {
  id: 7,
  name: "Orc Archer",
  notes: "bow",
  quantity: 2,
  size: "28mm",
  game: "Quest",
  location_id: 1,
  location_path: "Desk",
  races: ["orc"],
  roles: ["archer"],
  combat: ["ranged"],
  thumbnail_url: "/api/v1/miniatures/7/thumbnail",
};
const groups = [
  {
    category: "race",
    label: "Race",
    entries: [{ name: "orc", used_by: 2 }],
  },
  {
    category: "location",
    label: "Location",
    entries: [{ name: "Desk", used_by: 1, id: 1, parent_id: null, kind: "room", path: "Desk", child_count: 0 }],
  },
];

const ui = await import("../frontend/src/main.ts");
await new Promise((resolve) => setTimeout(resolve, 30));

const fileButton = document.querySelector("#file-menu-button");
const fileMenu = document.querySelector("#file-menu");
const helpButton = document.querySelector("#help-menu-button");
fileButton.click();
assert.equal(fileMenu.hidden, false, "wireMenu opens the File menu from its button");
assert.equal(fileButton.getAttribute("aria-expanded"), "true", "openMenu marks the menu expanded");
assert.equal(document.querySelector("#file-menu [role=separator]").getAttribute("role"), "separator");
fileButton.dispatchEvent(keyEvent("keydown", "ArrowRight"));
assert.equal(helpButton.getAttribute("aria-expanded"), "true", "siblingMenu moves from File to Help");
document.querySelector("#help-menu").dispatchEvent(keyEvent("keydown", "Escape"));
assert.equal(document.querySelector("#help-menu").hidden, true, "closeMenus hides the open menu");

assert.equal(document.querySelector("#token-form").hidden, true);
assert.equal(document.querySelector("#desktop-panel").hidden, false);
assert.equal(document.querySelector("#admin-token").textContent, "admin-token-value-ok");
assert.equal(document.querySelector("#about-version").textContent, "26.10.1");
assert.match(document.querySelector("#results").textContent, /Orc Archer/);
assert.match(document.querySelector("#filters").textContent, /orc/);
assert.match(document.querySelector("#report").textContent, /Desk/);
assert.match(document.querySelector("#keys").textContent, /reader/);
assert.equal(document.querySelector("#theme-toggle").textContent, "Dark theme");

assert.equal(ui.esc(`<a & ">`), "&lt;a &amp; &quot;&gt;");
assert.deepEqual(ui.uniqueTags(["Orc", "orc", "Elf"]), ["Orc", "Elf"]);
assert.deepEqual(ui.splitExtra(" wizard, mage, "), ["wizard", "mage"]);
assert.equal(ui.remPx(2), 32);
assert.equal(ui.filtersMax(), 640);
assert.equal(ui.clampFilters(10), ui.remPx(12));
assert.equal(ui.clampFilters(5000), 640);
assert.equal(ui.narrowLayout(), false);
ui.writeFiltersWidth(400);
assert.equal(ui.readFiltersWidth(), 400);
ui.commitFiltersWidth(320);
assert.equal(ui.readFiltersWidth(), 320);
ui.layoutFilters();
assert.equal(document.querySelector("#pane-resize").hidden, false, "wireDivider shows the handle on a wide window");
const before = ui.shownFiltersWidth();
document.querySelector("#pane-resize").dispatchEvent(keyEvent("keydown", "ArrowRight"));
assert.equal(ui.readFiltersWidth(), Math.round(ui.clampFilters(before + 16)));
document.querySelector("#pane-resize").dispatchEvent(keyEvent("keydown", "Home"));
assert.equal(ui.readFiltersWidth(), Math.round(ui.remPx(12)));
narrow = true;
ui.layoutFilters();
assert.equal(document.querySelector("#pane-resize").hidden, true);
assert.equal(ui.narrowLayout(), true);
narrow = false;
ui.applyShownWidth(300);
assert.match(document.querySelector("main").style.getPropertyValue("--filters-width"), /300px/);

document.querySelector("#q").value = "archer";
const race = document.querySelector("#filters input[name=race]");
race.checked = true;
race.setAttribute("checked", "");
assert.equal(ui.checked("race").join(","), "orc");
assert.match(ui.queryFromFilters(), /q=archer/);
assert.match(ui.queryFromFilters(), /race=orc/);
assert.match(ui.checks("size", ["28mm"], ["28MM"]), /checked/);
assert.match(ui.checks("size", [], []), /None yet/);

const row = ui.filterRow(groups[0], groups[0].entries[0]);
assert.match(row, /data-rename/);
assert.match(row, /data-delete-filter/);
assert.match(row, /2 in use/);
ui.renderFilterEditor(groups);
assert.match(document.querySelector("#filter-editor").textContent, /Race/);
assert.match(document.querySelector("#filter-editor").textContent, /Inside/);
ui.renderFilters(
  { races: ["goblin"], roles: [], combat: [], sizes: ["32mm"], games: [] },
  [location],
);
assert.match(document.querySelector("#filters").textContent, /goblin/);
ui.renderEditor([location], { races: ["orc"], roles: ["warrior"], combat: ["melee"], sizes: ["28mm"], games: ["Quest"] });
assert.equal(document.querySelector("#editor-title").textContent, "Add a miniature");
ui.renderReport([]);
assert.match(document.querySelector("#report").textContent, /No locations yet/);
ui.renderReport([report]);
assert.match(document.querySelector("#report").textContent, /Including inside|Desk/);
ui.renderLocationForm([location]);
assert.match(document.querySelector("#location-form").innerHTML, /Desk/);
ui.renderKeys([]);
assert.match(document.querySelector("#keys").textContent, /No keys yet/);
ui.renderKeys([{ id: 3, name: "reader", scope: "read", revoked_at: null }]);
assert.match(document.querySelector("#keys").textContent, /active/);

ui.filterSay("kept the old name");
assert.equal(document.querySelector("#filter-message").textContent, "kept the old name");
await ui.changeFilter(async () => {
  throw new Error("a race named \"goblin\" already exists");
});
assert.match(document.querySelector("#filter-message").textContent, /goblin/);
await ui.changeFilter(async () => {});
assert.equal(document.querySelector("#filter-message").textContent, "");

assert.equal(ui.loopback("127.0.0.1:8080"), true);
assert.equal(ui.loopback("[::1]:8080"), true);
assert.equal(ui.loopback("0.0.0.0:8080"), false);
ui.showBindWarning("0.0.0.0:8080");
assert.match(document.querySelector("#api-warning").textContent, /not limited to this computer/);
ui.renderApiStatus({
  api_enabled: true,
  api_bind: "127.0.0.1:9090",
  listening: true,
  bound: "127.0.0.1:9090",
  managed: true,
});
assert.match(document.querySelector("#api-status").textContent, /127.0.0.1:9090/);
ui.showBindWarning("127.0.0.1:9090");
assert.equal(document.querySelector("#api-warning").textContent, "");

const dialog = document.querySelector("#settings-dialog");
ui.showDialog(dialog, "#theme-toggle");
assert.equal(dialog.hasAttribute("open"), true);
assert.ok(ui.focusable(dialog).some((item) => item.id === "theme-toggle"));
dialog.dispatchEvent(keyEvent("keydown", "Tab"));
assert.equal(dialog.open, true, "wireDialog keeps the dialog open while tabbing inside it");
dialog.dispatchEvent(keyEvent("cancel", "Escape"));
assert.equal(dialog.open, false, "wireDialog closes on cancel");

ui.paintThemeToggle();
assert.equal(document.querySelector("#theme-toggle").textContent, "Dark theme");
document.querySelector("#theme-toggle").click();
assert.equal(document.documentElement.dataset.theme, "dark");
assert.equal(document.querySelector("#theme-toggle").textContent, "Light theme");
assert.equal(localStorage.getItem("mc.theme"), "dark");

ui.openMiniature(miniature);
assert.equal(document.querySelector("#mini-dialog").open, true);
assert.match(document.querySelector("#editor-title").textContent, /Orc Archer/);
document.querySelector("#menu-add").click();
assert.equal(document.querySelector("#editor-title").textContent, "Add a miniature", "openMenu's add item starts a blank miniature");

await ui.showResults([]);
assert.match(document.querySelector("#results").textContent, /No miniatures match/);
await ui.showResults([miniature]);
assert.equal(document.querySelector("#results img").alt, "Orc Archer");
document.querySelector("#q").value = "wizard";
await ui.searchMinis();
assert.match(document.querySelector("#results").textContent, /Orc Archer/);
await ui.loadKeys();
assert.match(document.querySelector("#keys").textContent, /reader/);
await ui.loadApiSettings();
assert.match(document.querySelector("#api-status").textContent, /API is off/);
await ui.openFilters();
assert.equal(document.querySelector("#filter-dialog").open, true);
await ui.reloadFilterEditor();
assert.match(document.querySelector("#filter-editor").textContent, /Race/);
await ui.refresh();
assert.match(document.querySelector("#results").textContent, /Orc Archer/);
await ui.boot();
assert.equal(document.querySelector("#admin-token").textContent, "admin-token-value-ok");

await ui.exitApp();
assert.match(document.querySelector("#message").textContent, /Close this browser tab/);

console.log("ui tests passed");
