import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { ask } from "@tauri-apps/plugin-dialog";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-shell";

const RELEASES_URL = "https://github.com/RSJWY/SwiftShare/releases";

async function isPortableMode(): Promise<boolean> {
  try {
    return await invoke<boolean>("is_portable_mode_command");
  } catch {
    return false;
  }
}

export async function checkForAppUpdates(userInitiated: boolean) {
  try {
    const update = await check();

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
