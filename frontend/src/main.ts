import { CatalogApi, type Facets, type Location, type LocationReport, type Miniature } from "./api";

const TOKEN_KEY = "mc.token";
const COMBAT = ["melee", "ranged", "magic"];

const app = document.querySelector<HTMLDivElement>("#app");
if (!app) throw new Error("missing app root");

let token = localStorage.getItem(TOKEN_KEY) ?? "";
let editing: Miniature | null = null;
const thumbs = new Map<number, string>();

app.innerHTML = `
  <header>
    <div>
      <h1>Miniature Catalog</h1>
      <p>Where each figure is, and which box to open.</p>
    </div>
    <form id="token-form" class="token">
      <input id="token" type="password" placeholder="Admin or read token" autocomplete="current-password" />
      <button type="submit">Save token</button>
    </form>
  </header>
  <div id="message"></div>
  <main>
    <section class="panel">
      <h2>Find</h2>
      <label class="block" for="q">Name</label>
      <input id="q" type="search" placeholder="wizard, orc warrior" />
      <div id="filters"></div>
      <div class="row" style="margin-top:0.8rem">
        <button id="search" type="button">Search</button>
        <button id="clear" class="quiet" type="button">Clear</button>
      </div>
    </section>
    <section class="panel">
      <h2>Figures</h2>
      <div id="results" class="results"></div>
    </section>
    <section class="panel wide">
      <h2 id="editor-title">Add a miniature</h2>
      <form id="mini-form" class="stack"></form>
    </section>
    <section class="panel wide">
      <h2>Storage</h2>
      <form id="location-form" class="row"></form>
      <div id="report"></div>
    </section>
    <section class="panel wide">
      <h2>API keys</h2>
      <p class="empty">Create a read key for the encounter generator. The token is shown once.</p>
      <form id="key-form" class="row"></form>
      <div id="new-key"></div>
      <div id="keys"></div>
    </section>
  </main>
`;

const message = must<HTMLDivElement>("#message");
const tokenInput = must<HTMLInputElement>("#token");
tokenInput.value = token;

function must<T extends Element>(selector: string): T {
  const node = document.querySelector<T>(selector);
  if (!node) throw new Error(`missing ${selector}`);
  return node;
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
      const on = selected.includes(value) ? "checked" : "";
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
  const combat = [...new Set([...COMBAT, ...facets.combat])].sort();
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

function renderEditor(locations: Location[], facets: Facets): void {
  const current = editing;
  must<HTMLHeadingElement>("#editor-title").textContent = current ? `Edit ${current.name}` : "Add a miniature";
  const locationOptions = locations
    .map((location) => {
      const selected = current?.location_id === location.id ? "selected" : "";
      return `<option value="${location.id}" ${selected}>${esc(location.path)}</option>`;
    })
    .join("");
  const combatValues = [...new Set([...COMBAT, ...facets.combat, ...(current?.combat ?? [])])].sort();
  must<HTMLFormElement>("#mini-form").innerHTML = `
    <div class="row">
      <label>Name<input name="name" type="text" required value="${esc(current?.name ?? "")}" /></label>
      <label>Quantity<input name="quantity" type="number" min="0" required value="${current?.quantity ?? 1}" /></label>
      <label>Size<input name="size" type="text" required placeholder="28mm" value="${esc(current?.size ?? "")}" /></label>
      <label>Game<input name="game" type="text" placeholder="optional" value="${esc(current?.game ?? "")}" /></label>
    </div>
    <label class="block">Notes<textarea name="notes">${esc(current?.notes ?? "")}</textarea></label>
    <label class="block">Stored in<select name="location_id" required>${locationOptions || `<option value="">Add a location first</option>`}</select></label>
    <div class="row">
      <div><label class="block">Race / type</label>${checks("edit-race", facets.races, current?.races ?? [])}<input name="extra-race" type="text" placeholder="add more, comma separated" /></div>
      <div><label class="block">Combat</label>${checks("edit-combat", combatValues, current?.combat ?? [])}</div>
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
      editing = miniatures.find((mini) => mini.id === Number(button.dataset.edit)) ?? null;
      void refresh()
        .then(() => document.querySelector("#editor-title")?.scrollIntoView({ behavior: "smooth" }))
        .catch((error: Error) => say(error.message));
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
    combat: checked("edit-combat"),
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

if (token) {
  void refresh().catch((error: Error) => say(error.message));
} else {
  say("Save a token to load the catalog. Use the admin token from MC_ADMIN_TOKEN.");
}
