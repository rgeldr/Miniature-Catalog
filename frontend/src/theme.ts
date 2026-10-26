export type ThemeName = "light" | "dark";

/** App-specific storage. The CSS variable names are shared with the other apps. */
export const THEME_KEY = "mc.theme";

export function isThemeName(value: string | null): value is ThemeName {
  return value === "light" || value === "dark";
}

export function themeFromSystem(): ThemeName {
  return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

/** Saved choice wins. With nothing saved, follow the operating system. */
export function readTheme(): ThemeName {
  try {
    const saved = localStorage.getItem(THEME_KEY);
    if (isThemeName(saved)) return saved;
  } catch {
    /* private mode */
  }
  return themeFromSystem();
}

export function applyTheme(theme: ThemeName, persist: boolean): void {
  document.documentElement.dataset.theme = theme;
  if (!persist) return;
  try {
    localStorage.setItem(THEME_KEY, theme);
  } catch {
    /* private mode */
  }
}

export function currentTheme(): ThemeName {
  return document.documentElement.dataset.theme === "dark" ? "dark" : "light";
}
