import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { ask, confirm } from "@tauri-apps/plugin-dialog";
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
      // 便携版：使用 confirm，"确定"前往下载，"取消"关闭对话框
      if (portable) {
        const yes = await confirm(
          `发现新版本 ${update.version}，是否前往下载页面？\n\n更新内容：\n${update.body ?? "无"}`,
          {
            title: "SwiftShare 更新",
            okLabel: "前往下载",
            cancelLabel: "稍后",
          },
        );
        if (yes) {
          await open(RELEASES_URL);
        }
        // 用户点击"稍后"或叉号：直接关闭，不跳转
      } else {
        // 安装版：使用 ask 提供三个选项
        // ask 返回 true 表示点击 okLabel，false 表示点击 cancelLabel
        const shouldUpdate = await ask(
          `发现新版本 ${update.version}\n\n更新内容：\n${update.body ?? "无"}`,
          {
            title: "SwiftShare 更新",
            kind: "info",
            okLabel: "更新",
            cancelLabel: "关闭",
          },
        );

        if (shouldUpdate) {
          // 用户选择自动更新
          // 询问是否在更新失败时手动下载
          try {
            await update.downloadAndInstall();
            await relaunch();
          } catch (installError) {
            const manualDownload = await confirm(
              `自动更新失败：${installError}\n\n是否前往 GitHub 手动下载？`,
              {
                title: "更新失败",
                okLabel: "前往下载",
                cancelLabel: "取消",
              },
            );
            if (manualDownload) {
              await open(RELEASES_URL);
            }
          }
        }
        // 用户点击"关闭"或叉号：直接关闭，不跳转
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
