import { ipc, type Settings } from "./ipc";
import { askDialog, useStore } from "../store";
import { t, currentLang, type Lang } from "./i18n";

/**
 * Update-check bij start: één call naar de GitHub Releases API (geen library, gewoon
 * fetch). Is er een nieuwere versie, dan een huisstijl-popup met één pakkende regel uit
 * de release-tekst, een downloadknop, "Later" en "10 starts niet tonen".
 *
 * De pakkende regel staat per taal in de release-tekst als marker, bv.
 * `#NL#Versie 0.3 heeft een ingebouwde DHCP-server##`. Terugval: EN, dan de eerste
 * marker, dan een generieke tekst.
 */

const RELEASES_API = "https://api.github.com/repos/turn8io/T8-Lan/releases/latest";
const RELEASES_PAGE = "https://github.com/turn8io/T8-Lan/releases/latest";
/** Zoveel starts blijft de melding weg na "niet tonen". Helemaal uitzetten kan bewust niet. */
export const SNOOZE_STARTS = 10;
/** Pas na de eerste schermopbouw checken; de start mag er niet trager van worden. */
const START_DELAY_MS = 3000;
const FETCH_TIMEOUT_MS = 8000;

type Release = {
  tag_name: string;
  body: string | null;
  html_url: string;
  assets: { name: string; browser_download_url: string }[];
};

export type UpdateInfo = {
  version: string;
  headline: string | null;
  downloadUrl: string;
};

/** "v0.3.0" / "0.3.0" → [0,3,0]; ongeldig → null. */
export function parseVersion(s: string): number[] | null {
  const m = s.trim().replace(/^v/i, "").match(/^(\d+)\.(\d+)(?:\.(\d+))?/);
  if (!m) return null;
  return [Number(m[1]), Number(m[2]), Number(m[3] ?? 0)];
}

export function isNewer(candidate: string, current: string): boolean {
  const a = parseVersion(candidate);
  const b = parseVersion(current);
  if (!a || !b) return false;
  for (let i = 0; i < 3; i++) {
    if (a[i] !== b[i]) return a[i] > b[i];
  }
  return false;
}

/** Pakkende regel voor `lang` uit de release-tekst (markers `#XX#...##`). */
export function headlineFor(body: string | null | undefined, lang: Lang): string | null {
  if (!body) return null;
  const found = new Map<string, string>();
  for (const m of body.matchAll(/#([A-Za-z]{2})#([\s\S]*?)##/g)) {
    const text = m[2].trim();
    if (text) found.set(m[1].toLowerCase(), text);
  }
  return found.get(lang) ?? found.get("en") ?? found.values().next().value ?? null;
}

export function toUpdateInfo(release: Release, currentVersion: string, lang: Lang): UpdateInfo | null {
  if (!isNewer(release.tag_name, currentVersion)) return null;
  const installer = release.assets.find((a) => /setup\.exe$/i.test(a.name)) ?? release.assets[0];
  return {
    version: release.tag_name.replace(/^v/i, ""),
    headline: headlineFor(release.body, lang),
    downloadUrl: installer?.browser_download_url ?? release.html_url ?? RELEASES_PAGE,
  };
}

async function fetchLatest(): Promise<Release | null> {
  const ctrl = new AbortController();
  const timer = window.setTimeout(() => ctrl.abort(), FETCH_TIMEOUT_MS);
  try {
    const res = await fetch(RELEASES_API, {
      headers: { Accept: "application/vnd.github+json" },
      signal: ctrl.signal,
    });
    if (!res.ok) return null;
    return (await res.json()) as Release;
  } catch {
    return null; // offline, geblokkeerd of rate-limit: stil overslaan
  } finally {
    window.clearTimeout(timer);
  }
}

let checked = false;

/** Eénmalig per proces; roept zichzelf na `START_DELAY_MS` aan. */
export function scheduleUpdateCheck() {
  if (checked) return;
  checked = true;
  window.setTimeout(() => void runUpdateCheck(), START_DELAY_MS);
}

async function runUpdateCheck() {
  const settings = useStore.getState().settings;
  if (!settings) return;
  const [release, currentVersion] = await Promise.all([fetchLatest(), ipc.getAppVersion().catch(() => null)]);
  if (!release || !currentVersion) return;
  const info = toUpdateInfo(release, currentVersion, currentLang());
  if (!info) return;

  // Snooze: alleen voor precies deze versie; een nieuwere versie reset de teller.
  if (settings.update_snooze_version === info.version && settings.update_snooze_remaining > 0) {
    await persist({ ...settings, update_snooze_remaining: settings.update_snooze_remaining - 1 });
    return;
  }

  // De app start verborgen in het systeemvak: venster tonen zodat de melding gezien wordt.
  await ipc.showMainWindow().catch(() => {});
  const choice = await askDialog({
    title: t("update.title").replace("{version}", info.version),
    body: info.headline ?? t("update.generic"),
    confirmLabel: t("update.download"),
    cancelLabel: t("update.later"),
    altLabel: t("update.snooze").replace("{n}", String(SNOOZE_STARTS)),
  });
  if (choice === "confirm") {
    await ipc.openExternal(info.downloadUrl).catch(() => {});
  } else if (choice === "alt") {
    const latest = useStore.getState().settings ?? settings;
    await persist({ ...latest, update_snooze_version: info.version, update_snooze_remaining: SNOOZE_STARTS });
  }
}

async function persist(next: Settings) {
  useStore.getState().setSettings(next);
  await ipc.saveSettings(next).catch(() => {});
}
