import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Store } from "@tauri-apps/plugin-store";
import { motion } from "framer-motion";
import "./App.css";

type DeviceInfo = {
  name: string;
  ip: string;
  port: number;
};

type SharedEntry = {
  id: string;
  name: string;
  path: string;
  size: number;
  modified: number;
};

type PullProgress = {
  entry_id: string;
  name: string;
  received_bytes: number;
  total_bytes: number;
};

type Settings = {
  downloadDir: string;
  maxConcurrent: number;
  maxMbps: number;
  discoveryIntervalMs: number;
  sameSubnetOnly: boolean;
};

const DEFAULT_SETTINGS: Settings = {
  downloadDir: "",
  maxConcurrent: 2,
  maxMbps: 0,
  discoveryIntervalMs: 5000,
  sameSubnetOnly: false,
};

const SETTINGS_STORE_PATH = "settings.json";

function App() {
  const [devices, setDevices] = useState<DeviceInfo[]>([]);
  const [activeDevice, setActiveDevice] = useState<DeviceInfo | null>(null);
  const [dropActive, setDropActive] = useState(false);
  const [progress, setProgress] = useState(0);
  const [sharedList, setSharedList] = useState<SharedEntry[]>([]);
  const [remoteList, setRemoteList] = useState<SharedEntry[]>([]);
  const [downloadDir, setDownloadDir] = useState<string>("");
  const [statusMessage, setStatusMessage] = useState<string>("");
  const [statusType, setStatusType] = useState<"success" | "error" | "info">("info");
  const [pullingId, setPullingId] = useState<string | null>(null);
  const [pullProgress, setPullProgress] = useState<PullProgress | null>(null);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [settings, setSettings] = useState<Settings>(DEFAULT_SETTINGS);
  const [settingsLoading, setSettingsLoading] = useState(true);
  const emptyState = useMemo(() => devices.length === 0, [devices.length]);

  useEffect(() => {
    const unlistenPromise = listen<DeviceInfo[]>("device-list-updated", (event) => {
      setDevices(event.payload ?? []);
    });

    return () => {
      unlistenPromise.then((unlisten) => unlisten());
    };
  }, []);

  useEffect(() => {
    const unlistenPromise = listen<PullProgress>("pull-progress", (event) => {
      setPullProgress(event.payload ?? null);
      if (event.payload) {
        const percent = event.payload.total_bytes > 0
          ? Math.min(100, Math.round((event.payload.received_bytes / event.payload.total_bytes) * 100))
          : 0;
        setProgress(percent);
      }
    });

    return () => {
      unlistenPromise.then((unlisten) => unlisten());
    };
  }, []);

  useEffect(() => {
    invoke<SharedEntry[]>("list_shared_command").then((list) => setSharedList(list ?? []));
  }, []);

  useEffect(() => {
    let store: Store | null = null;
    let cancelled = false;
    const loadSettings = async () => {
      try {
        store = await Store.load(SETTINGS_STORE_PATH);
        const saved = await store.get<Settings>("settings");
        if (!cancelled && saved) {
          const merged = { ...DEFAULT_SETTINGS, ...saved };
          setSettings(merged);
          setDownloadDir(merged.downloadDir || "");
        }
      } catch (error) {
        console.error("Failed to load settings", error);
      } finally {
        if (!cancelled) {
          setSettingsLoading(false);
        }
      }
    };
    loadSettings();
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    if (!activeDevice) return;
    invoke<SharedEntry[]>("fetch_remote_list_command", {
      targetIp: activeDevice.ip,
      targetPort: activeDevice.port,
    }).then((list) => setRemoteList(list ?? []));
  }, [activeDevice]);

  const onDropFiles = async (files: FileList | null) => {
    if (!files || files.length === 0) {
      setDropActive(false);
      return;
    }

    const paths: string[] = [];
    for (const file of Array.from(files)) {
      const path = (file as File & { path?: string }).path;
      if (path) {
        paths.push(path);
      }
    }

    if (paths.length > 0) {
      const updated = await invoke<SharedEntry[]>("add_shared_command", {
        paths,
      });
      setSharedList((prev) => [...updated, ...prev]);
    }
    setDropActive(false);
  };

  const chooseDownloadDir = async () => {
    const folder = await open({
      directory: true,
      multiple: false,
    });
    if (typeof folder === "string") {
      setSettings((prev) => ({ ...prev, downloadDir: folder }));
      setDownloadDir(folder);
    }
  };

  const saveSettings = async (next: Settings) => {
    const store = await Store.load(SETTINGS_STORE_PATH);
    await store.set("settings", next);
    await store.save();
    setSettings(next);
    setDownloadDir(next.downloadDir || "");
    await invoke("update_settings_command", {
      maxConcurrent: next.maxConcurrent,
      maxMbps: next.maxMbps,
      discoveryIntervalMs: next.discoveryIntervalMs,
      sameSubnetOnly: next.sameSubnetOnly,
    });
  };

  const resetSettings = async () => {
    const store = await Store.load(SETTINGS_STORE_PATH);
    await store.clear();
    await store.save();
    setSettings(DEFAULT_SETTINGS);
    setDownloadDir("");
    await invoke("update_settings_command", {
      maxConcurrent: DEFAULT_SETTINGS.maxConcurrent,
      maxMbps: DEFAULT_SETTINGS.maxMbps,
      discoveryIntervalMs: DEFAULT_SETTINGS.discoveryIntervalMs,
      sameSubnetOnly: DEFAULT_SETTINGS.sameSubnetOnly,
    });
  };

  const pullEntry = async (entry: SharedEntry) => {
    if (!activeDevice || !downloadDir) return;
    setPullingId(entry.id);
    setProgress(0);
    setStatusType("info");
    setStatusMessage(`正在拉取 ${entry.name}...`);
    setPullProgress(null);
    try {
      await invoke("pull_file_command", {
        entryId: entry.id,
        targetIp: activeDevice.ip,
        targetPort: activeDevice.port,
        destDir: downloadDir,
      });
      setStatusType("success");
      setStatusMessage(`已完成 ${entry.name}`);
      setPullProgress(null);
      const list = await invoke<SharedEntry[]>("fetch_remote_list_command", {
        targetIp: activeDevice.ip,
        targetPort: activeDevice.port,
      });
      setRemoteList(list ?? []);
    } catch (error) {
      console.error(error);
      setStatusType("error");
      setStatusMessage(`拉取失败: ${String(error)}`);
      setPullProgress(null);
    } finally {
      setPullingId(null);
    }
  };

  return (
    <main
      className="relative h-full overflow-hidden"
      onDragOver={(event) => {
        event.preventDefault();
        setDropActive(true);
      }}
      onDragLeave={() => setDropActive(false)}
      onDrop={(event) => {
        event.preventDefault();
        onDropFiles(event.dataTransfer.files);
      }}
    >
      <div className="pointer-events-none absolute inset-0 opacity-40">
        <div className="absolute -left-24 -top-24 h-72 w-72 rounded-full bg-indigo-500/40 blur-3xl" />
        <div className="absolute right-0 top-1/3 h-96 w-96 rounded-full bg-cyan-400/30 blur-3xl" />
      </div>

      <div className="relative mx-auto flex h-full max-w-6xl flex-col gap-6 px-6 py-8">
        <header className="flex items-center justify-between" data-tauri-drag-region>
          <div>
            <p className="text-xs uppercase tracking-[0.4em] text-indigo-200/70">SwiftShare</p>
            <h1 className="text-3xl font-semibold text-white">局域网极速传输</h1>
            <p className="text-sm text-white/60">拖拽文件到窗口，极速发送</p>
          </div>
          <div className="flex items-center gap-2">
            <button
              className="subtle-button"
              data-tauri-drag-region="false"
              onClick={() => setSettingsOpen(true)}
              type="button"
            >
              设置
            </button>
            <button className="glow-button" data-tauri-drag-region="false">新建传输</button>
            <button
              className="close-button"
              data-tauri-drag-region="false"
              onClick={() => {
                void getCurrentWindow().close();
              }}
              aria-label="关闭窗口"
              title="关闭"
              type="button"
            >
              ×
            </button>
          </div>
        </header>

        <section className="glass-panel grid flex-1 grid-cols-[280px_1fr] gap-6 p-6">
          <aside className="flex flex-col gap-4">
            <div className="flex items-center justify-between">
              <h2 className="text-sm font-semibold text-white">在线设备</h2>
              <span className="rounded-full bg-white/10 px-3 py-1 text-xs text-white/60">
                {devices.length} 在线
              </span>
            </div>

            <div className="glass-card flex-1 space-y-2 p-3">
              {emptyState && (
                <div className="flex h-full flex-col items-center justify-center gap-3 text-center text-white/40">
                  <div className="h-12 w-12 rounded-full border border-dashed border-white/20" />
                  <p className="text-sm">等待设备加入局域网</p>
                </div>
              )}

              {devices.map((device) => {
                const active = activeDevice?.ip === device.ip;
                return (
                  <button
                    key={`${device.ip}:${device.port}`}
                    onClick={() => setActiveDevice(device)}
                    className={`flex w-full items-center justify-between rounded-2xl px-3 py-3 text-left text-sm transition ${
                      active ? "bg-indigo-500/20 text-white" : "bg-white/5 text-white/70 hover:bg-white/10"
                    }`}
                  >
                    <div>
                      <p className="font-medium">{device.name.replace("._swiftshare._tcp.local.", "")}</p>
                      <p className="text-xs text-white/50">
                        {device.ip}:{device.port}
                      </p>
                    </div>
                    <span className={`h-2 w-2 rounded-full ${active ? "bg-indigo-400" : "bg-emerald-400"}`} />
                  </button>
                );
              })}
            </div>
            <div className="glass-card p-4">
              <p className="text-xs text-white/50">下载目录</p>
              <div className="mt-2 flex items-center gap-2">
                <button className="subtle-button" onClick={chooseDownloadDir}>
                  选择目录
                </button>
                <span className="truncate text-xs text-white/60">{downloadDir || "未选择"}</span>
              </div>
            </div>
          </aside>

          <section className="flex flex-col gap-6">
            <div className="glass-card flex-1 p-6">
              <div className="flex items-start justify-between">
                <div>
                  <h3 className="text-lg font-semibold text-white">传输进度</h3>
                  <p className="text-sm text-white/60">连续流传输，聚合小文件提升速度</p>
                </div>
                <button className="subtle-button">查看历史</button>
              </div>

              {statusMessage && (
                <div
                  className={`mt-4 rounded-2xl px-4 py-3 text-sm ${
                    statusType === "success"
                      ? "bg-emerald-500/15 text-emerald-200"
                      : statusType === "error"
                        ? "bg-rose-500/15 text-rose-200"
                        : "bg-white/10 text-white/70"
                  }`}
                >
                  {statusMessage}
                </div>
              )}

              <div className="mt-6 space-y-4">
                <div className="flex items-center justify-between text-sm text-white/70">
                  <span>{pullProgress ? pullProgress.name : "当前任务"}</span>
                  <span>{progress}%</span>
                </div>
                <div className="h-3 rounded-full bg-white/10">
                  <motion.div
                    className="h-3 rounded-full bg-gradient-to-r from-indigo-500 to-cyan-400"
                    initial={{ width: 0 }}
                    animate={{ width: `${progress}%` }}
                    transition={{ duration: 0.6, ease: "easeOut" }}
                  />
                </div>
              </div>

              <div className="mt-8 grid grid-cols-2 gap-4">
                <div className="glass-card p-4">
                  <p className="text-xs text-white/50">目标设备</p>
                  <p className="text-sm font-medium text-white">
                    {activeDevice ? activeDevice.name : "未选择"}
                  </p>
                </div>
                <div className="glass-card p-4">
                  <p className="text-xs text-white/50">传输策略</p>
                  <p className="text-sm font-medium text-white">拉取 + 断线重连</p>
                </div>
              </div>
            </div>

            <div className="grid grid-cols-2 gap-4">
              <div className="glass-card p-5">
                <div className="flex items-center justify-between">
                  <p className="text-sm text-white/70">我的共享</p>
                  <div className="flex items-center gap-2">
                    <span className="text-xs text-white/40">{sharedList.length} 项</span>
                    <button
                      className="subtle-button"
                      onClick={async () => {
                        await invoke("clear_shared_command");
                        setSharedList([]);
                      }}
                    >
                      清空
                    </button>
                  </div>
                </div>
                <div className="mt-4 space-y-2">
                  {sharedList.length === 0 && (
                    <p className="text-xs text-white/40">拖拽文件进来即可共享</p>
                  )}
                  {sharedList.map((entry) => (
                    <div key={entry.id} className="flex items-center justify-between rounded-xl bg-white/5 px-3 py-2">
                      <div>
                        <p className="text-sm text-white/80">{entry.name}</p>
                        <p className="text-[11px] text-white/40">{entry.path}</p>
                      </div>
                      <span className="text-xs text-white/50">{Math.round(entry.size / 1024)} KB</span>
                    </div>
                  ))}
                </div>
              </div>

              <div className="glass-card p-5">
                <div className="flex items-center justify-between">
                  <p className="text-sm text-white/70">对方共享</p>
                  <button
                    className="subtle-button"
                    onClick={() =>
                      activeDevice &&
                      invoke<SharedEntry[]>("fetch_remote_list_command", {
                        targetIp: activeDevice.ip,
                        targetPort: activeDevice.port,
                      }).then((list) => setRemoteList(list ?? []))
                    }
                  >
                    刷新
                  </button>
                </div>
                <div className="mt-4 space-y-2">
                  {!activeDevice && <p className="text-xs text-white/40">先选择设备</p>}
                  {activeDevice && remoteList.length === 0 && (
                    <p className="text-xs text-white/40">暂无共享</p>
                  )}
                  {remoteList.map((entry) => (
                    <div key={entry.id} className="flex items-center justify-between rounded-xl bg-white/5 px-3 py-2">
                      <div>
                        <p className="text-sm text-white/80">{entry.name}</p>
                        <p className="text-[11px] text-white/40">{Math.round(entry.size / 1024)} KB</p>
                      </div>
                      <button
                        className="glow-button disabled:cursor-not-allowed disabled:opacity-50"
                        onClick={() => pullEntry(entry)}
                        disabled={!downloadDir || pullingId === entry.id}
                      >
                        {pullingId === entry.id ? "拉取中" : "拉取"}
                      </button>
                    </div>
                  ))}
                </div>
              </div>
            </div>
          </section>
        </section>
      </div>

      {dropActive && (
        <div className="pointer-events-none absolute inset-0 flex items-center justify-center bg-white/5 backdrop-blur-sm">
          <div className="glass-card px-8 py-6 text-center">
            <p className="text-lg font-semibold text-white">松开即可发送</p>
            <p className="text-xs text-white/50">将文件传输到选中的设备</p>
          </div>
        </div>
      )}

      {settingsOpen && (
        <div className="absolute inset-0 z-20 flex items-center justify-center bg-black/50 backdrop-blur-sm">
          <div className="glass-panel w-full max-w-xl p-6">
            <div className="flex items-center justify-between">
              <h2 className="text-lg font-semibold text-white">设置</h2>
              <button
                className="close-button"
                onClick={() => setSettingsOpen(false)}
                type="button"
              >
                ×
              </button>
            </div>

            {settingsLoading ? (
              <p className="mt-6 text-sm text-white/60">加载中...</p>
            ) : (
              <form
                className="mt-6 space-y-4"
                onSubmit={(event) => {
                  event.preventDefault();
                  void saveSettings(settings);
                  setSettingsOpen(false);
                }}
              >
                <div className="glass-card p-4">
                  <label className="text-xs text-white/60">下载目录</label>
                  <div className="mt-2 flex items-center gap-2">
                    <button className="subtle-button" type="button" onClick={chooseDownloadDir}>
                      选择目录
                    </button>
                    <span className="truncate text-xs text-white/60">
                      {settings.downloadDir || downloadDir || "未选择"}
                    </span>
                  </div>
                </div>

                <div className="glass-card p-4">
                  <label className="text-xs text-white/60">传输并发</label>
                  <input
                    className="mt-2 w-full rounded-xl border border-white/10 bg-white/5 px-3 py-2 text-sm text-white"
                    type="number"
                    min={1}
                    max={8}
                    value={settings.maxConcurrent}
                    onChange={(event) =>
                      setSettings((prev) => ({
                        ...prev,
                        maxConcurrent: Math.max(1, Number(event.target.value || 1)),
                      }))
                    }
                  />
                </div>

                <div className="glass-card p-4">
                  <label className="text-xs text-white/60">限速 (Mbps，0 表示不限速)</label>
                  <input
                    className="mt-2 w-full rounded-xl border border-white/10 bg-white/5 px-3 py-2 text-sm text-white"
                    type="number"
                    min={0}
                    value={settings.maxMbps}
                    onChange={(event) =>
                      setSettings((prev) => ({
                        ...prev,
                        maxMbps: Math.max(0, Number(event.target.value || 0)),
                      }))
                    }
                  />
                </div>

                <div className="glass-card p-4">
                  <label className="text-xs text-white/60">自动发现刷新 (毫秒)</label>
                  <input
                    className="mt-2 w-full rounded-xl border border-white/10 bg-white/5 px-3 py-2 text-sm text-white"
                    type="number"
                    min={1000}
                    step={500}
                    value={settings.discoveryIntervalMs}
                    onChange={(event) =>
                      setSettings((prev) => ({
                        ...prev,
                        discoveryIntervalMs: Math.max(1000, Number(event.target.value || 1000)),
                      }))
                    }
                  />
                </div>

                <div className="glass-card flex items-center justify-between p-4">
                  <div>
                    <p className="text-sm text-white">仅显示同网段设备</p>
                    <p className="text-xs text-white/50">过滤非本机网段的设备</p>
                  </div>
                  <input
                    type="checkbox"
                    checked={settings.sameSubnetOnly}
                    onChange={(event) =>
                      setSettings((prev) => ({
                        ...prev,
                        sameSubnetOnly: event.target.checked,
                      }))
                    }
                  />
                </div>

                <div className="flex items-center justify-between">
                  <button
                    className="subtle-button"
                    type="button"
                    onClick={() => void resetSettings()}
                  >
                    恢复默认
                  </button>
                  <div className="flex items-center gap-2">
                    <button
                      className="subtle-button"
                      type="button"
                      onClick={() => setSettingsOpen(false)}
                    >
                      取消
                    </button>
                    <button className="glow-button" type="submit">
                      保存
                    </button>
                  </div>
                </div>
              </form>
            )}
          </div>
        </div>
      )}
    </main>
  );
}

export default App;
