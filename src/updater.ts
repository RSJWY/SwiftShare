import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { ask } from "@tauri-apps/plugin-dialog";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-shell";

const RELEASES_URL = "https://github.com/RSJWY/SwiftShare/releases";
const DEFAULT_MIRROR = "https://ghfast.top/";

async function isPortableMode(): Promise<boolean> {
  try {
    return await invoke<boolean>("is_portable_mode_command");
  } catch {
    return false;
  }
}

export async function checkForAppUpdates(
  userInitiated: boolean,
  options?: { mirrorUrl?: string; onChecking?: (checking: boolean) => void }
) {
  try {
    // 通知开始检测
    options?.onChecking?.(true);

    // 使用镜像地址包装 check 函数
    const mirrorUrl = options?.mirrorUrl || DEFAULT_MIRROR;
    
    // 暂时修改 endpoint 使用镜像
    const originalFetch = window.fetch;
    const checkWithMirror = async () => {
      if (mirrorUrl) {
        // 劫持 fetch 来重写 URL
        window.fetch = async (input: RequestInfo | URL, init?: RequestInit) => {
          const url = input.toString();
          // 只重写 GitHub 相关的请求
          if (url.includes('github.com') && !url.includes(mirrorUrl)) {
            const newUrl = url.replace(/^https:\/\/github\.com/, mirrorUrl.replace(/\/$/, ''));
            return originalFetch(newUrl, init);
          }
          return originalFetch(input, init);
        };
      }
      
      try {
        return await check();
      } finally {
        // 恢复原始 fetch
        window.fetch = originalFetch;
      }
    };

    const update = await checkWithMirror();

    // 通知检测完成
    options?.onChecking?.(false);

    if (update?.available) {
      const portable = await isPortableMode();
      // 便携版：只提供前往下载选项
      if (portable) {
        const yes = await ask(
          `发现新版本 ${update.version}，是否前往下载页面？\n\n更新内容：\n${update.body ?? "无"}`,
          {
            title: "SwiftShare 更新",
            kind: "info",
            okLabel: "前往下载",
            cancelLabel: "稍后",
          },
        );
        if (yes) {
          await open(RELEASES_URL);
        }
      } else {
        // 安装版：提供"更新"和"手动下载"两个选项
        const yes = await ask(
          `发现新版本 ${update.version}\n\n更新内容：\n${update.body ?? "无"}\n\n点击「更新」自动下载安装，或点击「手动下载」前往 GitHub 下载。`,
          {
            title: "SwiftShare 更新",
            kind: "info",
            okLabel: "更新",
            cancelLabel: "手动下载",
          },
        );

        if (yes) {
          // 用户选择自动更新
          await update.downloadAndInstall();
          await relaunch();
        } else {
          // 用户选择手动下载
          await open(RELEASES_URL);
        }
      }
    } else if (userInitiated) {
      await ask("当前已是最新版本。", {
        title: "SwiftShare 更新",
        kind: "info",
        okLabel: "好的",
        cancelLabel: "关闭",
      });
    }
  } catch (e) {
    // 通知检测完成（即使出错）
    options?.onChecking?.(false);
    
    if (userInitiated) {
      await ask(`检查更新失败：${e}`, {
        title: "SwiftShare 更新",
        kind: "error",
        okLabel: "好的",
        cancelLabel: "关闭",
      });
    } else {
      console.error("Auto update check failed:", e);
    }
  }
}
