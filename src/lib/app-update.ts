import { invoke } from "@tauri-apps/api/core";

const CACHE_KEY = "ai-light:update-check";
const CACHE_TTL_MS = 6 * 60 * 60 * 1000;
const LEADING_V = /^v/;
const RELEASE_VERSION = /^v?\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/;

interface GitHubAsset {
  browser_download_url: string;
  digest?: string | null;
  name: string;
  size: number;
}

interface GitHubRelease {
  _asset_suffix?: string;
  assets: GitHubAsset[];
  body?: string | null;
  html_url: string;
  published_at?: string | null;
  tag_name: string;
}

export interface AppUpdateInfo {
  asset: GitHubAsset | null;
  currentVersion: string;
  latestVersion: string;
  notes: string;
  publishedAt: string | null;
  releaseUrl: string;
  updateAvailable: boolean;
}

function normalizedVersion(version: string): string {
  return version.trim().replace(LEADING_V, "").split("-")[0] ?? "0.0.0";
}

export function compareVersions(left: string, right: string): number {
  const a = normalizedVersion(left).split(".").map(Number);
  const b = normalizedVersion(right).split(".").map(Number);
  for (let index = 0; index < Math.max(a.length, b.length); index += 1) {
    const difference = (a[index] ?? 0) - (b[index] ?? 0);
    if (difference !== 0) {
      return difference;
    }
  }
  return 0;
}

function preferredAsset(release: Partial<GitHubRelease>): GitHubAsset | null {
  const suffix = release._asset_suffix;
  if (!(suffix && Array.isArray(release.assets))) {
    return null;
  }
  return release.assets.find((asset) => asset.name.endsWith(suffix)) ?? null;
}

function parseRelease(value: unknown, currentVersion: string): AppUpdateInfo {
  const release = value as Partial<GitHubRelease>;
  if (
    typeof release.tag_name !== "string" ||
    !RELEASE_VERSION.test(release.tag_name) ||
    typeof release.html_url !== "string" ||
    !Array.isArray(release.assets)
  ) {
    throw new Error("更新源返回了无效数据");
  }
  const latestVersion = normalizedVersion(release.tag_name);
  return {
    asset: preferredAsset(release),
    currentVersion: normalizedVersion(currentVersion),
    latestVersion,
    notes: typeof release.body === "string" ? release.body : "",
    publishedAt:
      typeof release.published_at === "string" ? release.published_at : null,
    releaseUrl: release.html_url,
    updateAvailable: compareVersions(latestVersion, currentVersion) > 0,
  };
}

function cached(currentVersion: string): AppUpdateInfo | null {
  try {
    const value = JSON.parse(localStorage.getItem(CACHE_KEY) ?? "null") as {
      checkedAt?: number;
      info?: AppUpdateInfo;
    } | null;
    if (
      value?.info?.currentVersion === normalizedVersion(currentVersion) &&
      typeof value.checkedAt === "number" &&
      Date.now() - value.checkedAt < CACHE_TTL_MS
    ) {
      return value.info;
    }
  } catch {
    localStorage.removeItem(CACHE_KEY);
  }
  return null;
}

export async function checkAppUpdate(
  currentVersion: string,
  force = false
): Promise<AppUpdateInfo> {
  if (!force) {
    const hit = cached(currentVersion);
    if (hit) {
      return hit;
    }
  }

  const info = parseRelease(
    await invoke<unknown>("fetch_latest_release"),
    currentVersion
  );
  localStorage.setItem(
    CACHE_KEY,
    JSON.stringify({ checkedAt: Date.now(), info })
  );
  return info;
}

export function resolveDownloadUrl(info: AppUpdateInfo): Promise<string> {
  if (!info.asset) {
    return Promise.resolve(info.releaseUrl);
  }
  return invoke<string>("resolve_update_download_url", {
    downloadUrl: info.asset.browser_download_url,
  });
}
