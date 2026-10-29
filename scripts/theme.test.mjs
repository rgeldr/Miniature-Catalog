import assert from "node:assert/strict";
import { applyTheme, currentTheme, isThemeName, readTheme, THEME_KEY, themeFromSystem } from "../frontend/src/theme.ts";

const store = new Map();
let storageThrows = false;
globalThis.localStorage = {
  getItem(key) {
    if (storageThrows) throw new Error("private mode");
    return store.has(key) ? store.get(key) : null;
  },
  setItem(key, value) {
    if (storageThrows) throw new Error("private mode");
    store.set(key, String(value));
  },
  removeItem(key) {
    store.delete(key);
  },
};
const dataset = {};
globalThis.document = { documentElement: { dataset } };
let prefersDark = false;
globalThis.window = {
  matchMedia(query) {
    assert.equal(query, "(prefers-color-scheme: dark)");
    return { matches: prefersDark };
  },
};

assert.equal(isThemeName("light"), true);
assert.equal(isThemeName("dark"), true);
assert.equal(isThemeName("sepia"), false);
assert.equal(isThemeName(null), false);

prefersDark = true;
assert.equal(themeFromSystem(), "dark");
prefersDark = false;
assert.equal(themeFromSystem(), "light");

store.delete(THEME_KEY);
assert.equal(readTheme(), "light");
prefersDark = true;
assert.equal(readTheme(), "dark");
store.set(THEME_KEY, "light");
assert.equal(readTheme(), "light");
store.set(THEME_KEY, "nope");
assert.equal(readTheme(), "dark");
storageThrows = true;
assert.equal(readTheme(), "dark");
storageThrows = false;

applyTheme("light", false);
assert.equal(dataset.theme, "light");
assert.equal(currentTheme(), "light");
assert.equal(store.get(THEME_KEY), "nope");
applyTheme("dark", true);
assert.equal(dataset.theme, "dark");
assert.equal(currentTheme(), "dark");
assert.equal(store.get(THEME_KEY), "dark");
storageThrows = true;
applyTheme("light", true);
assert.equal(dataset.theme, "light");
assert.equal(currentTheme(), "light");
assert.equal(store.get(THEME_KEY), "dark");

console.log("theme tests passed");
