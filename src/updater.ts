import { ask, confirm } from "@tauri-apps/plugin-dialog";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-shell";
import { fetch } from "@tauri-apps/plugin-http";

const DEFAULT_MIRROR = "https://ghfast.top/";
const GITHUB_LATEST_JSON = "https://github.com/RSJWY/SwiftShare/releases/latest/download/latest.json";
const GITHUB_RELEASES_PAGE = "https://github.com/RSJWY/SwiftShare/releases";

interface UpdateInfo {
  version: string;
  date?: string;
  body?: string;
}

/**
 * 从镜像或原始 URL 获取 latest.json
 * 镜像格式：{mirror}{original_url}
 * 例如：https://ghfast.top/https://github.com/RSJWY/SwiftShare/releases/latest/download/latest.json
 */
async function fetchLatestJson(mirrorUrl: string): Promise<UpdateInfo | null> {
  // 构建请求 URL：镜像前缀 + 原始完整 URL
  let jsonUrl: string;
  if (mirrorUrl && mirrorUrl.trim() !== "") {
    // 使用镜像地址：{mirror}{original_url}
    const mirror = mirrorUrl.trim().replace(/\/$/, "");
    jsonUrl = `${mirror}/${GITHUB_LATEST_JSON}`;
  } else {
    // 直接访问 GitHub
    jsonUrl = GITHUB_LATEST_JSON;
  }

  console.log(`[Updater] Checking for updates at: ${jsonUrl}`);

  try {
    const response = await fetch(jsonUrl, {
      method: "GET",
      headers: {
        Accept: "application/json",
      },
    });

    if (!response.ok) {
      console.error(`[Updater] HTTP error: ${response.status}`);
      return null;
    }

    const data = await response.json();
    console.log("[Updater] Received update info:", data);

    return {
      version: data.version,
      date: data.pub_date,
      body: data.notes,
    };
  } catch (e) {
    console.error("[Updater] Failed to fetch latest.json:", e);
    return null;
  }
}

/**
 * 比较版本号，返回 true 表示 newVersion > currentVersion
 */
function isNewerVersion(newVersion: string, currentVersion: string): boolean {
  const parseVersion = (v: string) => v.split(".").map((n) => parseInt(n, 10) || 0);
  const newParts = parseVersion(newVersion);
  const currentParts = parseVersion(currentVersion);

  for (let i = 0; i < Math.max(newParts.length, currentParts.length); i++) {
    const newPart = newParts[i] || 0;
    const currentPart = currentParts[i] || 0;
    if (newPart > currentPart) return true;
    if (newPart < currentPart) return false;
  }
  return false; // 版本相同
}

/**
 * 获取当前应用版本
 */
async function getCurrentVersion(): Promise<string> {
  try {
    const { getVersion } = await import("@tauri-apps/api/app");
    return await getVersion();
  } catch {
    return await invoke<string>("get_app_version_command");
  }
}

/**
 * 导出 Releases 页面 URL 供设置页使用
 */
export const RELEASES_PAGE_URL = GITHUB_RELEASES_PAGE;

export async function checkForAppUpdates(
  userInitiated: boolean,
  options?: { mirrorUrl?: string; onChecking?: (checking: boolean) => void }
) {
  try {
    // 通知开始检测
    options?.onChecking?.(true);

    const mirrorUrl = options?.mirrorUrl || DEFAULT_MIRROR;
    const currentVersion = await getCurrentVersion();

    console.log(`[Updater] Current version: ${currentVersion}, Mirror: ${mirrorUrl}`);

    // 从镜像获取更新信息
    const updateInfo = await fetchLatestJson(mirrorUrl);

    // 通知检测完成
    options?.onChecking?.(false);

    if (!updateInfo) {
      if (userInitiated) {
        await ask("无法获取更新信息，请检查网络连接或镜像地址是否正确。", {
          title: "SwiftShare 更新",
          kind: "error",
          okLabel: "好的",
          cancelLabel: "关闭",
        });
      }
      return;
    }

    // 检查是否有新版本
    if (!isNewerVersion(updateInfo.version, currentVersion)) {
      if (userInitiated) {
        await ask(`当前已是最新版本 (${currentVersion})。`, {
          title: "SwiftShare 更新",
          kind: "info",
          okLabel: "好的",
          cancelLabel: "关闭",
        });
      }
      return;
    }

    // 有新版本可用
    console.log(`[Updater] New version available: ${updateInfo.version}`);

    // 显示更新提示（便携版和安装版统一处理）
    const message = `发现新版本 ${updateInfo.version}\n\n更新内容：\n${updateInfo.body ?? "无"}`;

    const yes = await confirm(message, {
      title: "SwiftShare 更新",
      okLabel: "前往下载",
      cancelLabel: "稍后",
    });

    if (yes) {
      // 直接跳转到 GitHub Releases 页面
      await open(GITHUB_RELEASES_PAGE);
    }
    // 用户点击"稍后"或叉号：直接关闭，不跳转
  } catch (e) {
    // 通知检测完成（即使出错）
    options?.onChecking?.(false);

    console.error("[Updater] Update check failed:", e);

    if (userInitiated) {
      await ask(`检查更新失败：${e}`, {
        title: "SwiftShare 更新",
        kind: "error",
        okLabel: "好的",
        cancelLabel: "关闭",
      });
    }
  }
}
