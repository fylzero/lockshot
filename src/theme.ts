import { computed, ref, watch } from "vue";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { ResolvedTheme, ThemePreference } from "./types";

export const THEME_OPTIONS: { id: ThemePreference; label: string }[] = [
  { id: "system", label: "System" },
  { id: "light", label: "Light" },
  { id: "dark", label: "Dark" },
];

/** The last saved theme, so the window opens in it before the settings load. */
const STORAGE_KEY = "theme";

export function sanitizeTheme(value: unknown): ThemePreference {
  return value === "light" || value === "dark" ? value : "system";
}

function readStoredTheme() {
  try {
    return sanitizeTheme(localStorage.getItem(STORAGE_KEY));
  } catch {
    return "system";
  }
}

const systemQuery = window.matchMedia("(prefers-color-scheme: light)");
const systemTheme = ref<ResolvedTheme>(systemQuery.matches ? "light" : "dark");
systemQuery.addEventListener("change", (event) => {
  systemTheme.value = event.matches ? "light" : "dark";
});

export const themePreference = ref<ThemePreference>(readStoredTheme());

export const resolvedTheme = computed<ResolvedTheme>(() =>
  themePreference.value === "system" ? systemTheme.value : themePreference.value,
);

watch(
  resolvedTheme,
  (theme) => {
    document.documentElement.dataset.theme = theme;
  },
  { immediate: true },
);

async function applyWindowTheme(preference: ThemePreference) {
  try {
    await getCurrentWindow().setTheme(preference === "system" ? null : preference);
  } catch {
    /* not every platform can theme the native window */
  }
}

watch(
  themePreference,
  (preference) => {
    try {
      localStorage.setItem(STORAGE_KEY, preference);
    } catch {
      /* the cached theme only avoids a flash on launch */
    }
    void applyWindowTheme(preference);
  },
  { immediate: true },
);
