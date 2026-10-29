import {
  CatalogApi,
  type ApiSettings,
  type Facets,
  type FilterEntry,
  type FilterGroup,
  type Location,
  type LocationReport,
  type Miniature,
} from "./api";
import packageJson from "../package.json" with { type: "json" };
import { requestExit } from "./desktop-exit";
import { applyTheme, currentTheme, readTheme } from "./theme";

const version = packageJson.version;

applyTheme(readTheme(), false);

const TOKEN_KEY = "mc.token";
const COMBAT = ["melee", "ranged", "magic"];

const app = document.querySelector<HTMLDivElement>("#app");
if (!app) throw new Error("missing app root");

let token = localStorage.getItem(TOKEN_KEY) ?? "";
let editing: Miniature | null = null;
let locationsCache: Location[] = [];
let facetsCache: Facets | null = null;
let returnFocus: HTMLElement | null = null;
let restoreFocus = true;
const thumbs = new Map<number, string>();

app.innerHTML = `
  <nav class="menubar" role="menubar" aria-label="Main">
    <div class="menu-root">
      <button id="file-menu-button" type="button" role="menuitem" aria-haspopup="menu" aria-expanded="false" aria-controls="file-menu">File</button>
      <div id="file-menu" class="menu" role="menu" aria-labelledby="file-menu-button" hidden>
        <button id="menu-add" type="button" role="menuitem">Add a miniature</button>
        <button id="menu-filters" type="button" role="menuitem">Edit filters</button>
        <button id="menu-encounter" type="button" role="menuitem">Encounter generator</button>
        <button id="menu-settings" type="button" role="menuitem">Settings</button>
        <button id="menu-storage" type="button" role="menuitem">Storage</button>
        <div class="menu-sep" role="separator"></div>
        <button id="menu-exit" type="button" role="menuitem">Exit</button>
      </div>
    </div>
    <div class="menu-root">
      <button id="help-menu-button" type="button" role="menuitem" aria-haspopup="menu" aria-expanded="false" aria-controls="help-menu">Help</button>
      <div id="help-menu" class="menu" role="menu" aria-labelledby="help-menu-button" hidden>
        <button id="menu-about" type="button" role="menuitem">About</button>
      </div>
    </div>
  </nav>
  <header>
    <h1>Miniature Catalog</h1>
  </header>
  <div id="message"></div>
  <main>
    <section class="panel">
      <h2>Filters</h2>
      <label class="block" for="q">Name</label>
      <input id="q" type="search" placeholder="wizard, orc warrior" />
      <div id="filters" class="panel-scroll"></div>
      <div class="row" style="margin-top:0.8rem">
        <button id="search" type="button">Search</button>
        <button id="clear" class="quiet" type="button">Clear</button>
      </div>
    </section>
    <div id="pane-resize" role="separator" aria-orientation="vertical" aria-label="Resize filters" tabindex="0"></div>
    <section class="panel">
      <h2>Figures</h2>
      <div id="results" class="results panel-scroll"></div>
    </section>
  </main>
  <dialog id="mini-dialog" aria-labelledby="editor-title">
    <div class="dialog-head">
      <h2 id="editor-title">Add a miniature</h2>
      <button id="mini-close" class="quiet" type="button">Close</button>
    </div>
    <form id="mini-form" class="stack"></form>
  </dialog>
  <dialog id="storage-dialog" aria-labelledby="storage-title">
    <div class="dialog-head">
      <h2 id="storage-title">Storage</h2>
      <button id="storage-close" class="quiet" type="button">Close</button>
    </div>
    <form id="location-form" class="row"></form>
    <div id="report"></div>
  </dialog>
  <dialog id="encounter-dialog" aria-labelledby="encounter-title">
    <div class="dialog-head">
      <h2 id="encounter-title">Encounter generator</h2>
      <button id="encounter-close" class="quiet" type="button">Close</button>
    </div>
    <div class="stack" id="desktop-panel" hidden>
      <p class="empty">This window is the catalog. The generator calls a separate HTTP port on this computer, and only while that API is turned on.</p>
      <label class="block">Admin token</label>
      <div class="secret" id="admin-token"></div>
      <div class="row" style="margin-top:0.6rem">
        <button id="copy-token" class="quiet" type="button">Copy token</button>
      </div>
      <form id="api-form" class="stack">
        <label><input id="api-enabled" type="checkbox" /> Allow the encounter generator to connect</label>
        <label class="block" for="api-bind">Listen address</label>
        <input id="api-bind" type="text" spellcheck="false" autocomplete="off" />
        <p id="api-warning" class="warn"></p>
        <p id="api-status" class="empty"></p>
        <div><button type="submit">Save API settings</button></div>
      </form>
    </div>
    <h3>API keys</h3>
    <p class="empty">Create a read key for the encounter generator. The token is shown once.</p>
    <form id="key-form" class="row"></form>
    <div id="new-key"></div>
    <div id="keys"></div>
  </dialog>
  <dialog id="filter-dialog" aria-labelledby="filter-title">
    <div class="dialog-head">
      <h2 id="filter-title">Edit filters</h2>
      <button id="filter-close" class="quiet" type="button">Close</button>
    </div>
    <p class="empty">Renaming updates every miniature that uses that value. A name that already exists is refused. Deleting is blocked while miniatures still use it, or while a location still has something inside it.</p>
    <p id="filter-message" class="warn"></p>
    <div id="filter-editor"></div>
  </dialog>
  <dialog id="about-dialog" aria-labelledby="about-title">
    <div class="dialog-head">
      <h2 id="about-title">About</h2>
      <button id="about-close" class="quiet" type="button">Close</button>
    </div>
    <p><strong>Miniature Catalog</strong></p>
    <p>Version <span id="about-version"></span></p>
  </dialog>
  <dialog id="settings-dialog" aria-labelledby="settings-title">
    <div class="dialog-head">
      <h2 id="settings-title">Settings</h2>
      <button id="settings-close" class="quiet" type="button">Close</button>
    </div>
    <div class="stack">
      <button id="theme-toggle" class="quiet" type="button"></button>
      <form id="token-form" class="token">
        <input id="token" type="password" placeholder="Admin or read token" autocomplete="current-password" />
        <button type="submit">Save token</button>
      </form>
    </div>
  </dialog>
`;

paintThemeToggle();
must<HTMLButtonElement>("#theme-toggle").addEventListener("click", () => {
  applyTheme(currentTheme() === "dark" ? "light" : "dark", true);
  paintThemeToggle();
});

const menus: Menu[] = [
  { button: must<HTMLButtonElement>("#file-menu-button"), panel: must<HTMLDivElement>("#file-menu") },
  { button: must<HTMLButtonElement>("#help-menu-button"), panel: must<HTMLDivElement>("#help-menu") },
];
must<HTMLElement>("#about-version").textContent = version;
wireMenu();
for (const dialog of Array.from(document.querySelectorAll<HTMLDialogElement>("dialog"))) wireDialog(dialog);
must<HTMLButtonElement>("#menu-add").addEventListener("click", () => {
  openMiniature(null);
});
must<HTMLButtonElement>("#menu-storage").addEventListener("click", () => {
  showDialog(must<HTMLDialogElement>("#storage-dialog"), "#location-form input[name=name]");
});
must<HTMLButtonElement>("#menu-encounter").addEventListener("click", () => {
  showDialog(must<HTMLDialogElement>("#encounter-dialog"), "#encounter-close");
});
must<HTMLButtonElement>("#menu-filters").addEventListener("click", () => {
  void openFilters();
});
must<HTMLButtonElement>("#menu-settings").addEventListener("click", () => {
  showDialog(must<HTMLDialogElement>("#settings-dialog"), "#theme-toggle");
});
must<HTMLButtonElement>("#menu-exit").addEventListener("click", () => {
  void exitApp();
});
must<HTMLButtonElement>("#menu-about").addEventListener("click", () => {
  showDialog(must<HTMLDialogElement>("#about-dialog"), "#about-close");
});
must<HTMLButtonElement>("#mini-close").addEventListener("click", () => {
  must<HTMLDialogElement>("#mini-dialog").close();
});
must<HTMLButtonElement>("#storage-close").addEventListener("click", () => {
  must<HTMLDialogElement>("#storage-dialog").close();
});
must<HTMLButtonElement>("#encounter-close").addEventListener("click", () => {
  must<HTMLDialogElement>("#encounter-dialog").close();
});
must<HTMLButtonElement>("#filter-close").addEventListener("click", () => {
  must<HTMLDialogElement>("#filter-dialog").close();
});
must<HTMLButtonElement>("#about-close").addEventListener("click", () => {
  must<HTMLDialogElement>("#about-dialog").close();
});
must<HTMLButtonElement>("#settings-close").addEventListener("click", () => {
  must<HTMLDialogElement>("#settings-dialog").close();
});
wireDivider();

function paintThemeToggle(): void {
  const button = must<HTMLButtonElement>("#theme-toggle");
  const dark = currentTheme() === "dark";
  button.textContent = dark ? "Light theme" : "Dark theme";
  button.setAttribute("aria-pressed", dark ? "true" : "false");
}

const message = must<HTMLDivElement>("#message");
const tokenInput = must<HTMLInputElement>("#token");
tokenInput.value = token;

function must<T extends Element>(selector: string): T {
  const node = document.querySelector<T>(selector);
  if (!node) throw new Error(`missing ${selector}`);
  return node;
}

function focusable(root: ParentNode): HTMLElement[] {
  return Array.from(root.querySelectorAll<HTMLElement>("button, input, select, textarea, a[href]")).filter(
    (el) => !el.hasAttribute("disabled") && el.tabIndex !== -1 && !el.closest("[hidden]"),
  );
}

type Menu = { button: HTMLButtonElement; panel: HTMLElement };

function openMenu(menu: Menu): void {
  for (const other of menus) {
    const open = other === menu;
    other.panel.hidden = !open;
    other.button.setAttribute("aria-expanded", open ? "true" : "false");
  }
  menu.panel.querySelector<HTMLButtonElement>("[role=menuitem]")?.focus();
}

function closeMenus(focus?: HTMLElement): void {
  for (const menu of menus) {
    menu.panel.hidden = true;
    menu.button.setAttribute("aria-expanded", "false");
  }
  focus?.focus();
}

function wireMenu(): void {
  for (const menu of menus) {
    menu.button.addEventListener("click", () => {
      if (menu.panel.hidden) openMenu(menu);
      else closeMenus();
    });
    menu.button.addEventListener("keydown", (event) => {
      if (event.key === "ArrowDown" || event.key === "Enter" || event.key === " ") {
        event.preventDefault();
        openMenu(menu);
      } else if (event.key === "ArrowRight" || event.key === "ArrowLeft") {
        event.preventDefault();
        const next = siblingMenu(menu, event.key === "ArrowRight" ? 1 : -1);
        if (menu.panel.hidden) next.button.focus();
        else openMenu(next);
      }
    });
    menu.panel.addEventListener("keydown", (event) => {
      const items = Array.from(menu.panel.querySelectorAll<HTMLButtonElement>("[role=menuitem]"));
      const index = items.indexOf(document.activeElement as HTMLButtonElement);
      if (event.key === "Escape") {
        event.preventDefault();
        closeMenus(menu.button);
      } else if (event.key === "ArrowDown") {
        event.preventDefault();
        items[(index + 1) % items.length]?.focus();
      } else if (event.key === "ArrowUp") {
        event.preventDefault();
        items[(index - 1 + items.length) % items.length]?.focus();
      } else if (event.key === "ArrowRight" || event.key === "ArrowLeft") {
        event.preventDefault();
        openMenu(siblingMenu(menu, event.key === "ArrowRight" ? 1 : -1));
      } else if (event.key === "Home") {
        event.preventDefault();
        items[0]?.focus();
      } else if (event.key === "End") {
        event.preventDefault();
        items[items.length - 1]?.focus();
      } else if (event.key === "Tab") {
        closeMenus();
      }
    });
  }
  document.addEventListener("click", (event) => {
    if (menus.every((menu) => menu.panel.hidden)) return;
    const target = event.target;
    if (!(target instanceof Node)) return;
    if (menus.some((menu) => menu.button.contains(target) || menu.panel.contains(target))) return;
    closeMenus();
  });
  document.addEventListener("keydown", (event) => {
    if (!event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;
    if (document.querySelector("dialog[open]")) return;
    const key = event.key.toLowerCase();
    const menu = key === "f" ? menus[0] : key === "h" ? menus[1] : null;
    if (!menu) return;
    event.preventDefault();
    menu.button.focus();
    openMenu(menu);
  });
}

function siblingMenu(menu: Menu, step: number): Menu {
  const index = menus.indexOf(menu);
  return menus[(index + step + menus.length) % menus.length];
}

const FILTERS_WIDTH_KEY = "mc.filtersWidth";

function remPx(rem: number): number {
  const root = Number.parseFloat(getComputedStyle(document.documentElement).fontSize) || 16;
  return rem * root;
}

function readFiltersWidth(): number | null {
  try {
    const raw = localStorage.getItem(FILTERS_WIDTH_KEY);
    if (raw === null) return null;
    const value = Number(raw);
    return Number.isFinite(value) ? value : null;
  } catch {
    return null;
  }
}

function writeFiltersWidth(px: number): void {
  try {
    localStorage.setItem(FILTERS_WIDTH_KEY, String(Math.round(px)));
  } catch {
    /* private mode */
  }
}

function filtersMax(): number {
  return window.innerWidth / 2;
}

function clampFilters(px: number): number {
  return Math.min(filtersMax(), Math.max(remPx(12), px));
}

function narrowLayout(): boolean {
  return window.matchMedia("(max-width: 800px)").matches;
}

function shownFiltersWidth(): number {
  const raw = getComputedStyle(must<HTMLElement>("main")).getPropertyValue("--filters-width");
  const value = Number.parseFloat(raw);
  if (Number.isFinite(value)) return value;
  return clampFilters(readFiltersWidth() ?? remPx(18));
}

function applyShownWidth(px: number): void {
  const handle = must<HTMLElement>("#pane-resize");
  must<HTMLElement>("main").style.setProperty("--filters-width", `${px}px`);
  handle.setAttribute("aria-valuemin", String(Math.round(remPx(12))));
  handle.setAttribute("aria-valuemax", String(Math.round(filtersMax())));
  handle.setAttribute("aria-valuenow", String(Math.round(px)));
}

function layoutFilters(): void {
  const handle = must<HTMLElement>("#pane-resize");
  const main = must<HTMLElement>("main");
  if (narrowLayout()) {
    main.style.removeProperty("--filters-width");
    handle.hidden = true;
    handle.tabIndex = -1;
    return;
  }
  handle.hidden = false;
  handle.tabIndex = 0;
  const chosen = readFiltersWidth();
  applyShownWidth(clampFilters(chosen ?? remPx(18)));
}

function commitFiltersWidth(px: number): void {
  writeFiltersWidth(clampFilters(px));
  layoutFilters();
}

function wireDivider(): void {
  const handle = must<HTMLElement>("#pane-resize");
  let dragging = false;
  let startX = 0;
  let startWidth = 0;

  handle.addEventListener("pointerdown", (event) => {
    if (event.button !== 0 || narrowLayout()) return;
    event.preventDefault();
    dragging = true;
    startX = event.clientX;
    startWidth = shownFiltersWidth();
    document.body.classList.add("resizing");
  });
  window.addEventListener("pointermove", (event) => {
    if (!dragging) return;
    event.preventDefault();
    applyShownWidth(clampFilters(startWidth + event.clientX - startX));
  });
  const finishDrag = (): void => {
    if (!dragging) return;
    const moved = Math.round(shownFiltersWidth()) !== Math.round(startWidth);
    dragging = false;
    document.body.classList.remove("resizing");
    if (moved) writeFiltersWidth(shownFiltersWidth());
    layoutFilters();
  };
  window.addEventListener("pointerup", finishDrag);
  window.addEventListener("pointercancel", finishDrag);
  handle.addEventListener("keydown", (event) => {
    if (narrowLayout()) return;
    const shown = shownFiltersWidth();
    let next: number | null = null;
    if (event.key === "ArrowLeft") next = shown - 16;
    else if (event.key === "ArrowRight") next = shown + 16;
    else if (event.key === "Home") next = remPx(12);
    else if (event.key === "End") next = filtersMax();
    else return;
    event.preventDefault();
    const clamped = clampFilters(next);
    if (event.key !== "Home" && event.key !== "End" && Math.round(clamped) === Math.round(shown)) return;
    commitFiltersWidth(clamped);
  });
  window.addEventListener("resize", () => {
    if (!dragging) layoutFilters();
  });
  layoutFilters();
}

async function exitApp(): Promise<void> {
  closeMenus();
  try {
    await requestExit({
      inDesktop: "__TAURI_INTERNALS__" in window,
      say,
      closeWindow: async () => {
        const { getCurrentWindow } = await import("@tauri-apps/api/window");
        await getCurrentWindow().close();
      },
    });
  } catch (error) {
    say(error instanceof Error ? error.message : "Could not close the window.");
  }
}

function showDialog(dialog: HTMLDialogElement, focusSelector: string): void {
  const active = document.activeElement;
  returnFocus = menus[0].button;
  if (active instanceof HTMLElement) {
    const owner = menus.find((menu) => menu.panel.contains(active) || menu.button === active);
    returnFocus = owner ? owner.button : active;
  }
  closeMenus();
  restoreFocus = false;
  for (const other of Array.from(document.querySelectorAll<HTMLDialogElement>("dialog[open]"))) {
    if (other !== dialog) other.close();
  }
  restoreFocus = true;
  if (!dialog.open) dialog.showModal();
  const preferred = dialog.querySelector<HTMLElement>(focusSelector);
  (preferred ?? focusable(dialog)[0])?.focus();
}

function wireDialog(dialog: HTMLDialogElement): void {
  dialog.addEventListener("click", (event) => {
    if (event.target !== dialog) return;
    const rect = dialog.getBoundingClientRect();
    const outside =
      event.clientX < rect.left ||
      event.clientX > rect.right ||
      event.clientY < rect.top ||
      event.clientY > rect.bottom;
    if (outside) dialog.close();
  });
  dialog.addEventListener("cancel", (event) => {
    event.preventDefault();
    dialog.close();
  });
  dialog.addEventListener("keydown", (event) => {
    if (event.key !== "Tab") return;
    const items = focusable(dialog);
    if (!items.length) return;
    const first = items[0];
    const last = items[items.length - 1];
    const active = document.activeElement;
    if (event.shiftKey && (active === first || !dialog.contains(active))) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && active === last) {
      event.preventDefault();
      first.focus();
    }
  });
  dialog.addEventListener("close", () => {
    if (!restoreFocus) return;
    if (returnFocus && document.contains(returnFocus)) returnFocus.focus();
  });
}

function openMiniature(mini: Miniature | null): void {
  editing = mini;
  if (facetsCache) renderEditor(locationsCache, facetsCache);
  showDialog(must<HTMLDialogElement>("#mini-dialog"), "#mini-form input[name=name]");
}

function esc(value: string): string {
  return value
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;");
}

function api(): CatalogApi {
  return new CatalogApi(token);
}

function say(text: string): void {
  message.textContent = text;
}

function checked(name: string): string[] {
  return Array.from(
    document.querySelectorAll<HTMLInputElement>(`input[name="${name}"]:checked`),
  ).map((input) => input.value);
}

function splitExtra(value: string): string[] {
  return value
    .split(",")
    .map((part) => part.trim())
    .filter(Boolean);
}

function uniqueTags(values: string[]): string[] {
  const out: string[] = [];
  for (const value of values) {
    if (!out.some((existing) => existing.toLowerCase() === value.toLowerCase())) {
      out.push(value);
    }
  }
  return out;
}

function queryFromFilters(): string {
  const params = new URLSearchParams();
  const q = must<HTMLInputElement>("#q").value.trim();
  if (q) params.set("q", q);
  const race = checked("race");
  const role = checked("role");
  const combat = checked("combat");
  const size = checked("size");
  const game = checked("game");
  const location = checked("location");
  if (race.length) params.set("race", race.join(","));
  if (role.length) params.set("role", role.join(","));
  if (combat.length) params.set("combat", combat.join(","));
  if (size.length) params.set("size", size.join(","));
  if (game.length) params.set("game", game.join(","));
  if (location.length) params.set("location_id", location.join(","));
  const text = params.toString();
  return text ? `?${text}` : "";
}

function checks(name: string, values: string[], selected: string[]): string {
  if (!values.length) return `<p class="empty">None yet.</p>`;
  return `<div class="checks">${values
    .map((value) => {
      const on = selected.some((item) => item.toLowerCase() === value.toLowerCase()) ? "checked" : "";
      return `<label><input type="checkbox" name="${esc(name)}" value="${esc(value)}" ${on} /> ${esc(value)}</label>`;
    })
    .join("")}</div>`;
}

function renderFilters(facets: Facets, locations: Location[]): void {
  const selected = {
    race: checked("race"),
    role: checked("role"),
    combat: checked("combat"),
    size: checked("size"),
    game: checked("game"),
    location: checked("location"),
  };
  const combat = uniqueTags([...facets.combat, ...COMBAT]).sort();
  const locationChecks = locations.length
    ? `<div class="checks">${locations
        .map((location) => {
          const on = selected.location.includes(String(location.id)) ? "checked" : "";
          return `<label><input type="checkbox" name="location" value="${location.id}" ${on} /> ${esc(location.path)}</label>`;
        })
        .join("")}</div>`
    : `<p class="empty">None yet.</p>`;
  must<HTMLDivElement>("#filters").innerHTML = `
    <label class="block">Race / type</label>
    ${checks("race", facets.races, selected.race)}
    <label class="block">Combat style</label>
    ${checks("combat", combat, selected.combat)}
    <label class="block">Size</label>
    ${checks("size", facets.sizes, selected.size)}
    <label class="block">Role</label>
    ${checks("role", facets.roles, selected.role)}
    <label class="block">Game</label>
    ${checks("game", facets.games, selected.game)}
    <label class="block">Location, including everything inside it</label>
    ${locationChecks}
  `;
}

function filterSay(text: string): void {
  must<HTMLParagraphElement>("#filter-message").textContent = text;
}

async function openFilters(): Promise<void> {
  filterSay("");
  showDialog(must<HTMLDialogElement>("#filter-dialog"), "#filter-close");
  try {
    await reloadFilterEditor();
  } catch (error) {
    filterSay(error instanceof Error ? error.message : "Could not load filters.");
  }
}

async function reloadFilterEditor(): Promise<void> {
  const groups = (await api().filters()).groups;
  renderFilterEditor(groups);
  await refresh();
}

function renderFilterEditor(groups: FilterGroup[]): void {
  const locations = groups.find((group) => group.category === "location")?.entries ?? [];
  const parentOptions = [`<option value="">Top level</option>`]
    .concat(
      locations.map(
        (entry) => `<option value="${entry.id}">${esc(entry.path ?? entry.name)}</option>`,
      ),
    )
    .join("");
  must<HTMLDivElement>("#filter-editor").innerHTML = groups
    .map((group) => {
      const rows = group.entries
        .map((entry) => filterRow(group, entry))
        .join("");
      const add =
        group.category === "location"
          ? `<form class="row filter-add" data-add-filter="location">
              <label>Name<input name="name" type="text" required placeholder="Drawer 4" /></label>
              <label>Kind<input name="kind" type="text" required placeholder="drawer" /></label>
              <label>Inside<select name="parent_id">${parentOptions}</select></label>
              <button type="submit">Add</button>
            </form>`
          : `<form class="row filter-add" data-add-filter="${esc(group.category)}">
              <label>New ${esc(group.label.toLowerCase())}<input name="name" type="text" required /></label>
              <button type="submit">Add</button>
            </form>`;
      return `<section>
        <h3>${esc(group.label)}</h3>
        ${rows || `<p class="empty">None yet.</p>`}
        ${add}
      </section>`;
    })
    .join("");
  const host = must<HTMLDivElement>("#filter-editor");
  host.querySelectorAll<HTMLButtonElement>("[data-rename]").forEach((button) => {
    button.addEventListener("click", () => {
      const row = button.closest<HTMLElement>("[data-filter-row]");
      if (!row) return;
      const input = row.querySelector<HTMLInputElement>("input");
      if (!input) return;
      const id = row.dataset.id ? Number(row.dataset.id) : undefined;
      void changeFilter(() =>
        api().renameFilter({
          category: row.dataset.category ?? "",
          name: row.dataset.name,
          new_name: input.value,
          id,
        }),
      );
    });
  });
  host.querySelectorAll<HTMLButtonElement>("[data-delete-filter]").forEach((button) => {
    button.addEventListener("click", () => {
      const row = button.closest<HTMLElement>("[data-filter-row]");
      if (!row) return;
      const used = Number(row.dataset.used ?? "0");
      const children = Number(row.dataset.children ?? "0");
      const name = row.dataset.name ?? "";
      if (used === 0 && children === 0 && !confirm(`Delete ${name}? Nothing uses it.`)) return;
      const id = row.dataset.id ? Number(row.dataset.id) : undefined;
      void changeFilter(() =>
        api().deleteFilter({
          category: row.dataset.category ?? "",
          name,
          id,
        }),
      );
    });
  });
  host.querySelectorAll<HTMLFormElement>("[data-add-filter]").forEach((form) => {
    form.addEventListener("submit", (event) => {
      event.preventDefault();
      const data = new FormData(form);
      const category = form.dataset.addFilter ?? "";
      const parent = String(data.get("parent_id") ?? "");
      void changeFilter(() =>
        api().addFilter({
          category,
          name: String(data.get("name") ?? ""),
          kind: category === "location" ? String(data.get("kind") ?? "") : undefined,
          parent_id: category === "location" ? (parent ? Number(parent) : null) : undefined,
        }),
      );
    });
  });
}

function filterRow(group: FilterGroup, entry: FilterEntry): string {
  const id = entry.id === undefined ? "" : ` data-id="${entry.id}"`;
  const where = entry.path ? `<p class="filter-meta">${esc(entry.path)}</p>` : "";
  const kind = entry.kind ? ` (${esc(entry.kind)})` : "";
  return `<div class="filter-entry" data-filter-row data-category="${esc(group.category)}" data-name="${esc(entry.name)}" data-used="${entry.used_by}" data-children="${entry.child_count ?? 0}"${id}>
    <div>
      ${where}
      <input type="text" value="${esc(entry.name)}" aria-label="${esc(group.label)} ${esc(entry.name)}" />
    </div>
    <span class="used">${entry.used_by} in use${kind}</span>
    <button type="button" data-rename>Rename</button>
    <button class="danger" type="button" data-delete-filter>Delete</button>
  </div>`;
}

async function changeFilter(action: () => Promise<unknown>): Promise<void> {
  try {
    await action();
    filterSay("");
    await reloadFilterEditor();
  } catch (error) {
    filterSay(error instanceof Error ? error.message : "Could not update filters.");
  }
}

function renderEditor(locations: Location[], facets: Facets): void {
  const current = editing;
  must<HTMLHeadingElement>("#editor-title").textContent = current ? `Edit ${current.name}` : "Add a miniature";
  const locationOptions = locations
    .map((location) => {
      const selected = current?.location_id === location.id ? "selected" : "";
      return `<option value="${location.id}" ${selected}>${esc(location.path)}</option>`;
    })
    .join("");
  const combatValues = uniqueTags([
    ...(current?.combat ?? []),
    ...facets.combat,
    ...COMBAT,
  ]).sort();
  must<HTMLFormElement>("#mini-form").innerHTML = `
    <div class="row">
      <label>Name<input name="name" type="text" required value="${esc(current?.name ?? "")}" /></label>
      <label>Quantity<input name="quantity" type="number" min="0" required value="${current?.quantity ?? 1}" /></label>
      <label>Size<input name="size" type="text" required placeholder="28mm" value="${esc(current?.size ?? "")}" /></label>
      <label>Game<input name="game" type="text" placeholder="optional" value="${esc(current?.game ?? "")}" /></label>
    </div>
    <label class="block">Notes<textarea name="notes">${esc(current?.notes ?? "")}</textarea></label>
    <label class="block">Stored in<select name="location_id" required>${locationOptions || `<option value="">Add a location first</option>`}</select></label>
    <div class="row tags">
      <div><label class="block">Race / type</label>${checks("edit-race", facets.races, current?.races ?? [])}<input name="extra-race" type="text" placeholder="add more, comma separated" /></div>
      <div><label class="block">Combat</label>${checks("edit-combat", combatValues, current?.combat ?? [])}<input name="extra-combat" type="text" placeholder="add more, comma separated" /></div>
      <div><label class="block">Can stand in as</label>${checks("edit-role", facets.roles, current?.roles ?? [])}<input name="extra-role" type="text" placeholder="wizard, mage" /></div>
    </div>
    <div class="row">
      <label>Image file<input name="file" type="file" accept="image/jpeg,image/png,image/gif" /></label>
      <label>Google Drive link or file id<input name="drive" type="text" placeholder="anyone with the link" /></label>
    </div>
    <div class="row">
      <button type="submit">${current ? "Save changes" : "Add miniature"}</button>
      <button id="cancel-edit" class="quiet" type="button">Clear form</button>
      ${current ? `<button id="delete-mini" class="danger" type="button">Delete</button>` : ""}
    </div>
  `;
  must<HTMLButtonElement>("#cancel-edit").addEventListener("click", () => {
    editing = null;
    void refresh();
  });
  const deleteButton = document.querySelector<HTMLButtonElement>("#delete-mini");
  deleteButton?.addEventListener("click", () => {
    if (!current) return;
    if (!confirm(`Delete ${current.name}?`)) return;
    void api()
      .deleteMiniature(current.id)
      .then(() => {
        thumbs.delete(current.id);
        editing = null;
        must<HTMLDialogElement>("#mini-dialog").close();
        say(`Deleted ${current.name}.`);
        return refresh();
      })
      .catch((error: Error) => say(error.message));
  });
}

function renderReport(rows: LocationReport[]): void {
  if (!rows.length) {
    must<HTMLDivElement>("#report").innerHTML = `<p class="empty">No locations yet.</p>`;
    return;
  }
  must<HTMLDivElement>("#report").innerHTML = `
    <table>
      <thead><tr><th>Where</th><th>Kind</th><th>Here</th><th>Including inside</th><th></th></tr></thead>
      <tbody>
        ${rows
          .map(
            (row) => `<tr>
              <td>${esc(row.path)}</td>
              <td>${esc(row.kind)}</td>
              <td>${row.direct_quantity}</td>
              <td>${row.total_quantity}</td>
              <td><button class="quiet" type="button" data-delete-location="${row.id}">Delete</button></td>
            </tr>`,
          )
          .join("")}
      </tbody>
    </table>
  `;
  document.querySelectorAll<HTMLButtonElement>("[data-delete-location]").forEach((button) => {
    button.addEventListener("click", () => {
      const id = Number(button.dataset.deleteLocation);
      void api()
        .deleteLocation(id)
        .then(() => refresh())
        .catch((error: Error) => say(error.message));
    });
  });
}

function renderLocationForm(locations: Location[]): void {
  const options = [`<option value="">Top level</option>`]
    .concat(locations.map((location) => `<option value="${location.id}">${esc(location.path)}</option>`))
    .join("");
  must<HTMLFormElement>("#location-form").innerHTML = `
    <label>Name<input name="name" type="text" required placeholder="Drawer 3" /></label>
    <label>Kind<input name="kind" type="text" required list="kinds" placeholder="drawer" /></label>
    <datalist id="kinds">
      <option value="room"></option><option value="cabinet"></option><option value="drawer"></option>
      <option value="box"></option><option value="container"></option><option value="tray"></option>
      <option value="shelf"></option>
    </datalist>
    <label>Inside<select name="parent_id">${options}</select></label>
    <button type="submit">Add location</button>
  `;
}

function renderKeys(keys: { id: number; name: string; scope: string; revoked_at: string | null }[]): void {
  if (!keys.length) {
    must<HTMLDivElement>("#keys").innerHTML = `<p class="empty">No keys yet.</p>`;
    return;
  }
  must<HTMLDivElement>("#keys").innerHTML = `
    <table>
      <thead><tr><th>Name</th><th>Scope</th><th>Status</th><th></th></tr></thead>
      <tbody>
        ${keys
          .map(
            (key) => `<tr>
              <td>${esc(key.name)}</td>
              <td>${esc(key.scope)}</td>
              <td>${key.revoked_at ? "revoked" : "active"}</td>
              <td>${key.revoked_at ? "" : `<button class="danger" type="button" data-revoke="${key.id}">Revoke</button>`}</td>
            </tr>`,
          )
          .join("")}
      </tbody>
    </table>
  `;
  document.querySelectorAll<HTMLButtonElement>("[data-revoke]").forEach((button) => {
    button.addEventListener("click", () => {
      void api()
        .revokeKey(Number(button.dataset.revoke))
        .then(() => loadKeys())
        .catch((error: Error) => say(error.message));
    });
  });
}

must<HTMLFormElement>("#key-form").innerHTML = `
  <label>Name<input name="name" type="text" required placeholder="encounter-generator" /></label>
  <label>Scope<select name="scope"><option value="read">read</option><option value="admin">admin</option></select></label>
  <button type="submit">Create key</button>
`;

async function showResults(miniatures: Miniature[]): Promise<void> {
  const host = must<HTMLDivElement>("#results");
  if (!miniatures.length) {
    host.innerHTML = `<p class="empty">No miniatures match.</p>`;
    return;
  }
  host.innerHTML = miniatures
    .map(
      (mini) => `<article class="card" data-id="${mini.id}">
        <div class="placeholder" data-thumb="${mini.id}">${esc(mini.name.slice(0, 1))}</div>
        <div>
          <h3>${esc(mini.name)} <span class="qty">×${mini.quantity}</span></h3>
          <p class="path">${esc(mini.location_path)}</p>
          <div class="chips">
            ${[mini.size, mini.game ?? "", ...mini.races, ...mini.combat, ...mini.roles]
              .filter(Boolean)
              .map((chip) => `<span class="chip">${esc(chip)}</span>`)
              .join("")}
          </div>
          <div class="actions"><button class="quiet" type="button" data-edit="${mini.id}">Edit</button></div>
        </div>
      </article>`,
    )
    .join("");
  const client = api();
  await Promise.all(
    miniatures.map(async (mini) => {
      if (!mini.thumbnail_url) return;
      try {
        let url = thumbs.get(mini.id);
        if (!url) {
          url = URL.createObjectURL(await client.thumbnail(mini.id));
          thumbs.set(mini.id, url);
        }
        const slot = host.querySelector(`[data-thumb="${mini.id}"]`);
        if (slot) {
          const image = document.createElement("img");
          image.className = "thumb";
          image.alt = mini.name;
          image.src = url;
          slot.replaceWith(image);
        }
      } catch {
        // A missing file still leaves the initial letter.
      }
    }),
  );
  host.querySelectorAll<HTMLButtonElement>("[data-edit]").forEach((button) => {
    button.addEventListener("click", () => {
      openMiniature(miniatures.find((mini) => mini.id === Number(button.dataset.edit)) ?? null);
    });
  });
}

async function loadKeys(): Promise<void> {
  try {
    const body = await api().keys();
    renderKeys(body.api_keys);
  } catch (error) {
    const text = error instanceof Error ? error.message : "Could not load keys";
    must<HTMLDivElement>("#keys").innerHTML = `<p class="empty">${esc(text)}</p>`;
  }
}

async function searchMinis(): Promise<void> {
  const listed = await api().miniatures(queryFromFilters());
  await showResults(listed.miniatures);
}

async function refresh(): Promise<void> {
  if (!token) {
    say("Save a token to load the catalog. Use the admin token from MC_ADMIN_TOKEN.");
    return;
  }
  const client = api();
  const [facets, locations, report, listed] = await Promise.all([
    client.facets(),
    client.locations(),
    client.report(),
    client.miniatures(queryFromFilters()),
  ]);
  locationsCache = locations.locations;
  facetsCache = facets;
  renderFilters(facets, locations.locations);
  renderEditor(locations.locations, facets);
  renderLocationForm(locations.locations);
  renderReport(report.locations);
  await showResults(listed.miniatures);
  await loadKeys();
}

must<HTMLFormElement>("#token-form").addEventListener("submit", (event) => {
  event.preventDefault();
  token = tokenInput.value.trim();
  localStorage.setItem(TOKEN_KEY, token);
  thumbs.forEach((url) => URL.revokeObjectURL(url));
  thumbs.clear();
  say("");
  void refresh().catch((error: Error) => say(error.message));
});

must<HTMLButtonElement>("#search").addEventListener("click", () => {
  void searchMinis().catch((error: Error) => say(error.message));
});

must<HTMLInputElement>("#q").addEventListener("keydown", (event) => {
  if (event.key === "Enter") {
    event.preventDefault();
    void searchMinis().catch((error: Error) => say(error.message));
  }
});

must<HTMLButtonElement>("#clear").addEventListener("click", () => {
  must<HTMLInputElement>("#q").value = "";
  document.querySelectorAll<HTMLInputElement>("#filters input").forEach((input) => {
    input.checked = false;
  });
  void searchMinis().catch((error: Error) => say(error.message));
});

must<HTMLFormElement>("#mini-form").addEventListener("submit", (event) => {
  event.preventDefault();
  const form = event.currentTarget as HTMLFormElement;
  const data = new FormData(form);
  const locationRaw = String(data.get("location_id") ?? "");
  const input = {
    name: String(data.get("name") ?? ""),
    notes: String(data.get("notes") ?? ""),
    quantity: Number(data.get("quantity") ?? 0),
    size: String(data.get("size") ?? ""),
    game: String(data.get("game") ?? "").trim() || null,
    location_id: Number(locationRaw),
    races: [...checked("edit-race"), ...splitExtra(String(data.get("extra-race") ?? ""))],
    roles: [...checked("edit-role"), ...splitExtra(String(data.get("extra-role") ?? ""))],
    combat: [...checked("edit-combat"), ...splitExtra(String(data.get("extra-combat") ?? ""))],
  };
  const file = data.get("file");
  const drive = String(data.get("drive") ?? "").trim();
  const client = api();
  const save = editing ? client.updateMiniature(editing.id, input) : client.createMiniature(input);
  void save
    .then(async (mini) => {
      if (file instanceof File && file.size > 0) {
        await client.uploadThumbnail(mini.id, file);
        const previous = thumbs.get(mini.id);
        if (previous) URL.revokeObjectURL(previous);
        thumbs.delete(mini.id);
      } else if (drive) {
        await client.importThumbnail(mini.id, drive);
        const previous = thumbs.get(mini.id);
        if (previous) URL.revokeObjectURL(previous);
        thumbs.delete(mini.id);
      }
      editing = null;
      must<HTMLDialogElement>("#mini-dialog").close();
      say(`Saved ${mini.name}.`);
      await refresh();
    })
    .catch((error: Error) => say(error.message));
});

must<HTMLFormElement>("#location-form").addEventListener("submit", (event) => {
  event.preventDefault();
  const data = new FormData(event.currentTarget as HTMLFormElement);
  const parent = String(data.get("parent_id") ?? "");
  void api()
    .createLocation({
      name: String(data.get("name") ?? ""),
      kind: String(data.get("kind") ?? ""),
      parent_id: parent ? Number(parent) : null,
    })
    .then(() => {
      say("Location added.");
      return refresh();
    })
    .catch((error: Error) => say(error.message));
});

must<HTMLFormElement>("#key-form").addEventListener("submit", (event) => {
  event.preventDefault();
  const data = new FormData(event.currentTarget as HTMLFormElement);
  void api()
    .createKey(String(data.get("name") ?? ""), String(data.get("scope") ?? "read"))
    .then((created) => {
      must<HTMLDivElement>("#new-key").innerHTML = `
        <p>Copy this token now. It will not be shown again.</p>
        <div class="secret">${esc(created.token)}</div>
      `;
      (event.target as HTMLFormElement).reset();
      return loadKeys();
    })
    .catch((error: Error) => say(error.message));
});

must<HTMLButtonElement>("#copy-token").addEventListener("click", () => {
  const value = must<HTMLElement>("#admin-token").textContent ?? "";
  void navigator.clipboard.writeText(value).then(
    () => say("Admin token copied."),
    () => say("Could not copy automatically. Select the token and copy it."),
  );
});

must<HTMLFormElement>("#api-form").addEventListener("submit", (event) => {
  event.preventDefault();
  const enabled = must<HTMLInputElement>("#api-enabled").checked;
  const bind = must<HTMLInputElement>("#api-bind").value.trim();
  void api()
    .updateSettings(enabled, bind)
    .then((settings) => {
      renderApiStatus(settings);
      say(settings.listening ? "API is on." : "API is off.");
    })
    .catch((error: Error) => say(error.message));
});

must<HTMLInputElement>("#api-bind").addEventListener("input", () => {
  showBindWarning(must<HTMLInputElement>("#api-bind").value.trim());
});

void boot();

function loopback(bind: string): boolean {
  const host = bind.startsWith("[")
    ? bind.slice(1, bind.indexOf("]"))
    : bind.slice(0, bind.lastIndexOf(":"));
  return host === "127.0.0.1" || host === "::1";
}

function showBindWarning(bind: string): void {
  must<HTMLParagraphElement>("#api-warning").textContent = loopback(bind)
    ? ""
    : "This address is not limited to this computer. Other machines that can reach it may try to connect.";
}

function renderApiStatus(settings: ApiSettings): void {
  must<HTMLInputElement>("#api-enabled").checked = settings.api_enabled;
  must<HTMLInputElement>("#api-bind").value = settings.api_bind;
  showBindWarning(settings.api_bind);
  const status = must<HTMLParagraphElement>("#api-status");
  if (settings.listening && settings.bound) {
    status.textContent = `API is listening on http://${settings.bound}`;
  } else if (settings.api_enabled) {
    status.textContent = "API is set to on, but nothing is listening yet.";
  } else {
    status.textContent = "API is off. The encounter generator cannot connect.";
  }
}

async function loadApiSettings(): Promise<void> {
  renderApiStatus(await api().settings());
}

async function boot(): Promise<void> {
  const session = await fetch("/api/v1/desktop-session").then(
    async (response) => (response.ok ? ((await response.json()) as { token?: string }) : null),
    () => null,
  );
  if (session?.token) {
    token = session.token;
    localStorage.setItem(TOKEN_KEY, token);
    must<HTMLFormElement>("#token-form").hidden = true;
    must<HTMLElement>("#desktop-panel").hidden = false;
    must<HTMLElement>("#admin-token").textContent = token;
    try {
      await refresh();
      await loadApiSettings();
    } catch (error) {
      say(error instanceof Error ? error.message : "Could not load the catalog.");
    }
    return;
  }
  if (token) {
    await refresh().catch((error: Error) => say(error.message));
  } else {
    say("Save a token to load the catalog.");
  }
}

export {
  applyShownWidth,
  boot,
  changeFilter,
  checked,
  checks,
  clampFilters,
  closeMenus,
  commitFiltersWidth,
  esc,
  exitApp,
  filterRow,
  filterSay,
  filtersMax,
  focusable,
  layoutFilters,
  loadApiSettings,
  loadKeys,
  loopback,
  narrowLayout,
  openFilters,
  openMenu,
  openMiniature,
  paintThemeToggle,
  queryFromFilters,
  readFiltersWidth,
  refresh,
  reloadFilterEditor,
  remPx,
  renderApiStatus,
  renderEditor,
  renderFilterEditor,
  renderFilters,
  renderKeys,
  renderLocationForm,
  renderReport,
  searchMinis,
  showBindWarning,
  showDialog,
  showResults,
  shownFiltersWidth,
  siblingMenu,
  splitExtra,
  uniqueTags,
  wireDialog,
  wireDivider,
  wireMenu,
  writeFiltersWidth,
};
