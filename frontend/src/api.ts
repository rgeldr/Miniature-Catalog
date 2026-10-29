export type Miniature = {
  id: number;
  name: string;
  notes: string;
  quantity: number;
  size: string;
  game: string | null;
  location_id: number;
  location_path: string;
  races: string[];
  roles: string[];
  combat: string[];
  thumbnail_url: string | null;
};

export type Location = {
  id: number;
  parent_id: number | null;
  name: string;
  kind: string;
  path: string;
};

export type Facets = {
  races: string[];
  roles: string[];
  combat: string[];
  sizes: string[];
  games: string[];
};

export type FilterEntry = {
  name: string;
  used_by: number;
  id?: number;
  parent_id?: number | null;
  kind?: string;
  path?: string;
  child_count?: number;
};

export type FilterGroup = {
  category: string;
  label: string;
  entries: FilterEntry[];
};

export type LocationReport = {
  id: number;
  parent_id: number | null;
  name: string;
  kind: string;
  path: string;
  direct_entries: number;
  direct_quantity: number;
  total_entries: number;
  total_quantity: number;
};

export type ApiSettings = {
  api_enabled: boolean;
  api_bind: string;
  listening: boolean;
  bound: string | null;
  managed: boolean;
};

export type ApiKey = {
  id: number;
  name: string;
  scope: string;
  created_at: string;
  revoked_at: string | null;
};

export type MiniatureInput = {
  name: string;
  notes: string;
  quantity: number;
  size: string;
  game: string | null;
  location_id: number;
  races: string[];
  roles: string[];
  combat: string[];
};

export async function readError(response: Response): Promise<string> {
  const text = await response.text();
  if (!text) return response.statusText;
  try {
    const body = JSON.parse(text) as { error?: string };
    return body.error || response.statusText;
  } catch {
    return text;
  }
}

export class CatalogApi {
  constructor(private token: string) {}

  private async request<T>(method: string, path: string, body?: unknown): Promise<T> {
    const headers: Record<string, string> = {};
    if (this.token) headers.Authorization = `Bearer ${this.token}`;
    let payload: string | undefined;
    if (body !== undefined) {
      headers["Content-Type"] = "application/json";
      payload = JSON.stringify(body);
    }
    const response = await fetch(path, { method, headers, body: payload });
    if (response.status === 204) return undefined as T;
    if (!response.ok) throw new Error(await readError(response));
    return (await response.json()) as T;
  }

  miniatures(query: string): Promise<{ miniatures: Miniature[] }> {
    return this.request("GET", `/api/v1/miniatures${query}`);
  }

  locations(): Promise<{ locations: Location[] }> {
    return this.request("GET", "/api/v1/locations");
  }

  facets(): Promise<Facets> {
    return this.request("GET", "/api/v1/facets");
  }

  filters(): Promise<{ groups: FilterGroup[] }> {
    return this.request("GET", "/api/v1/filters");
  }

  addFilter(input: {
    category: string;
    name: string;
    kind?: string;
    parent_id?: number | null;
  }): Promise<FilterEntry> {
    return this.request("POST", "/api/v1/filters", input);
  }

  renameFilter(input: {
    category: string;
    name?: string;
    new_name: string;
    id?: number;
  }): Promise<FilterEntry> {
    return this.request("PATCH", "/api/v1/filters", input);
  }

  deleteFilter(input: { category: string; name?: string; id?: number }): Promise<void> {
    return this.request("DELETE", "/api/v1/filters", input);
  }

  report(): Promise<{ locations: LocationReport[] }> {
    return this.request("GET", "/api/v1/reports/locations");
  }

  keys(): Promise<{ api_keys: ApiKey[] }> {
    return this.request("GET", "/api/v1/api-keys");
  }

  settings(): Promise<ApiSettings> {
    return this.request("GET", "/api/v1/settings");
  }

  updateSettings(apiEnabled: boolean, apiBind: string): Promise<ApiSettings> {
    return this.request("PUT", "/api/v1/settings", {
      api_enabled: apiEnabled,
      api_bind: apiBind,
    });
  }

  createKey(name: string, scope: string): Promise<ApiKey & { token: string }> {
    return this.request("POST", "/api/v1/api-keys", { name, scope });
  }

  revokeKey(id: number): Promise<void> {
    return this.request("DELETE", `/api/v1/api-keys/${id}`);
  }

  createLocation(input: { name: string; kind: string; parent_id: number | null }): Promise<Location> {
    return this.request("POST", "/api/v1/locations", input);
  }

  deleteLocation(id: number): Promise<void> {
    return this.request("DELETE", `/api/v1/locations/${id}`);
  }

  createMiniature(input: MiniatureInput): Promise<Miniature> {
    return this.request("POST", "/api/v1/miniatures", input);
  }

  updateMiniature(id: number, input: MiniatureInput): Promise<Miniature> {
    return this.request("PUT", `/api/v1/miniatures/${id}`, input);
  }

  deleteMiniature(id: number): Promise<void> {
    return this.request("DELETE", `/api/v1/miniatures/${id}`);
  }

  async uploadThumbnail(id: number, file: File): Promise<void> {
    const body = new FormData();
    body.append("file", file);
    const response = await fetch(`/api/v1/miniatures/${id}/thumbnail`, {
      method: "POST",
      headers: { Authorization: `Bearer ${this.token}` },
      body,
    });
    if (!response.ok) throw new Error(await readError(response));
  }

  importThumbnail(id: number, source: string): Promise<void> {
    return this.request("POST", `/api/v1/miniatures/${id}/thumbnail/import`, { source });
  }

  async thumbnail(id: number): Promise<Blob> {
    const response = await fetch(`/api/v1/miniatures/${id}/thumbnail`, {
      headers: { Authorization: `Bearer ${this.token}` },
    });
    if (!response.ok) throw new Error(await readError(response));
    return response.blob();
  }
}
