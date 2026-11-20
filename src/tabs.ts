import { computed, ref, watch } from "vue";
import type { RouteLocationNormalizedLoaded, Router } from "vue-router";
import { captures, closeCapture } from "./store";

/**
 * Home is pinned. Each open capture gets an Editor tab ("editor:<id>");
 * Settings and Change Log open on demand, like Recon's tabs.
 */
export type TabId = string;
type UtilityTab = "settings" | "changelog";

const UTILITY: Record<UtilityTab, { title: string; path: string }> = {
  settings: { title: "Settings", path: "/settings/general" },
  changelog: { title: "Change Log", path: "/changelog" },
};

const open = ref<UtilityTab[]>([]);
/** Remember which settings section each visit left off on. */
const lastSettingsPath = ref(UTILITY.settings.path);

const EDITOR = "editor:";

export function editorTabId(captureId: string) {
  return `${EDITOR}${captureId}`;
}

function tabFor(path: string): TabId {
  if (path.startsWith("/settings")) return "settings";
  if (path.startsWith("/changelog")) return "changelog";
  const editor = /^\/editor\/(.+)$/.exec(path);
  if (editor) return editorTabId(decodeURIComponent(editor[1]));
  return "home";
}

export function useTabs(router: Router, route: RouteLocationNormalizedLoaded) {
  watch(
    () => route.path,
    (path) => {
      const tab = tabFor(path);
      if ((tab === "settings" || tab === "changelog") && !open.value.includes(tab)) open.value.push(tab);
      if (tab === "settings") lastSettingsPath.value = path;
    },
    { immediate: true },
  );

  const active = computed(() => tabFor(route.path));
  const tabs = computed(() => [
    ...captures.value.map((c) => ({ id: editorTabId(c.id), title: c.name, kind: "editor" as const })),
    ...open.value.map((id) => ({ id: id as TabId, title: UTILITY[id].title, kind: id })),
  ]);

  function activate(id: TabId) {
    if (id === "home") void router.push("/");
    else if (id.startsWith(EDITOR)) void router.push(`/editor/${id.slice(EDITOR.length)}`);
    else if (id === "settings") void router.push(lastSettingsPath.value);
    else if (id === "changelog") void router.push(UTILITY.changelog.path);
  }

  function close(id: TabId) {
    const ids = ["home", ...tabs.value.map((t) => t.id)];
    const index = ids.indexOf(id);
    const wasActive = active.value === id;
    if (id.startsWith(EDITOR)) closeCapture(id.slice(EDITOR.length));
    else open.value = open.value.filter((t) => t !== id);
    if (wasActive) {
      /* Land on the tab to the left, as Recon and Shipyard do. */
      activate(ids[index - 1] ?? "home");
    }
  }

  return { tabs, active, activate, close };
}
