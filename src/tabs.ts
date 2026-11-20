import { computed, ref, watch } from "vue";
import type { RouteLocationNormalizedLoaded, Router } from "vue-router";
import { closeCapture, current } from "./store";

/**
 * Home is pinned. Editor shows only while a capture is open; Settings and
 * Change Log open on demand, like Recon's tabs.
 */
export type TabId = "home" | "editor" | "settings" | "changelog";
type UtilityTab = "settings" | "changelog";

const UTILITY: Record<UtilityTab, { title: string; path: string }> = {
  settings: { title: "Settings", path: "/settings/general" },
  changelog: { title: "Change Log", path: "/changelog" },
};

const open = ref<UtilityTab[]>([]);
/** Remember which settings section each visit left off on. */
const lastSettingsPath = ref(UTILITY.settings.path);

function tabFor(path: string): TabId {
  if (path.startsWith("/settings")) return "settings";
  if (path.startsWith("/changelog")) return "changelog";
  if (path.startsWith("/editor")) return "editor";
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
    ...(current.value ? [{ id: "editor" as TabId, title: "Editor" }] : []),
    ...open.value.map((id) => ({ id: id as TabId, title: UTILITY[id].title })),
  ]);

  function activate(id: TabId) {
    if (id === "home") void router.push("/");
    else if (id === "editor") void router.push("/editor");
    else void router.push(id === "settings" ? lastSettingsPath.value : UTILITY[id].path);
  }

  function close(id: TabId) {
    const ids = ["home" as TabId, ...tabs.value.map((t) => t.id)];
    const index = ids.indexOf(id);
    if (id === "editor") closeCapture();
    else open.value = open.value.filter((t) => t !== id);
    if (active.value === id) {
      /* Land on the tab to the left, as Recon and Shipyard do. */
      activate(ids[index - 1] ?? "home");
    }
  }

  return { tabs, active, activate, close };
}
