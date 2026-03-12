import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { Store } from "@tauri-apps/plugin-store";
import { startDrag } from "@crabnebula/tauri-plugin-drag";
import { motion, AnimatePresence } from "framer-motion";
import "./App.css";

type DeviceInfo = {
  machine_id: string;
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

type FileManifest = {
  ip: string;
  port: number;
  files: SharedEntry[];
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
  const [draggingId, setDraggingId] = useState<string | null>(null);
const [settingsOpen, setSettingsOpen] = useState(false);
  const [settings, setSettings] = useState<Settings>(DEFAULT_SETTINGS);
  const [settingsLoading, setSettingsLoading] = useState(true);
  const [dropZoneActive, setDropZoneActive] = useState(false);
  const sharedListRef = useRef<HTMLDivElement>(null);

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
    const unlistenPromise = listen<SharedEntry[]>("shared-list-updated", (event) => {
      setSharedList(event.payload ?? []);
    });
    return () => {
      unlistenPromise.then((unlisten) => unlisten());
    };
  }, []);

  useEffect(() => {
    const unlistenPromise = listen<FileManifest>("remote-manifest-updated", (event) => {
      const manifest = event.payload;
      if (!manifest || !activeDevice) return;
      if (manifest.ip === activeDevice.ip && manifest.port === activeDevice.port) {
        setRemoteList(manifest.files ?? []);
      }
    });
    return () => {
      unlistenPromise.then((unlisten) => unlisten());
    };
  }, [activeDevice]);

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

  const dragRemoteEntry = async (entry: SharedEntry) => {
    if (!activeDevice || draggingId) return;
    setDraggingId(entry.id);
    setStatusType("info");
    setStatusMessage(`准备拖出 ${entry.name}...`);
    try {
      const path = await invoke<string>("pull_to_temp_command", {
        entryId: entry.id,
        targetIp: activeDevice.ip,
        targetPort: activeDevice.port,
      });
      await startDrag({
        item: [path],
        icon: path,
      });
      setStatusType("success");
      setStatusMessage(`已准备拖出 ${entry.name}`);
    } catch (error) {
      console.error(error);
      setStatusType("error");
      setStatusMessage(`拖出失败: ${String(error)}`);
    } finally {
      setDraggingId(null);
    }
  };

return (
    <main className="relative h-full overflow-hidden">
      {/* Background Effects */}
      <div className="pointer-events-none absolute inset-0 opacity-40">
        <div className="absolute -left-24 -top-24 h-72 w-72 rounded-full bg-indigo-500/40 blur-3xl" />
        <div className="absolute right-0 top-1/3 h-96 w-96 rounded-full bg-cyan-400/30 blur-3xl" />
      </div>

      <div className="relative mx-auto flex h-full max-w-7xl flex-col gap-4 px-6 py-6">
        {/* Header - Compact */}
        <header className="flex items-center justify-between">
          <div className="flex items-center gap-4">
            <div>
              <p className="text-xs uppercase tracking-[0.3em] text-indigo-200/70">SwiftShare</p>
              <h1 className="text-xl font-semibold text-white">局域网文件共享</h1>
            </div>
            <div className="flex items-center gap-2 text-xs text-white/50">
              <span className="flex h-2 w-2 rounded-full bg-emerald-400" />
              {devices.length > 0 ? `${devices.length} 台设备在线` : "等待设备加入"}
            </div>
          </div>
          <div className="flex items-center gap-2">
            <button
              className="subtle-button text-xs"
              onClick={() => setSettingsOpen(true)}
              type="button"
            >
              设置
            </button>
          </div>
        </header>

{/* Main Layout - Single Shared List Focus */}
        <section className="glass-panel flex flex-1 flex-col gap-4 overflow-hidden p-5">
          {/* My Shared Files - Primary Focus */}
          <div
            ref={sharedListRef}
            className={`relative flex flex-1 flex-col overflow-hidden rounded-2xl border transition-colors ${
              dropZoneActive
                ? "border-indigo-400/60 bg-indigo-500/10"
                : "border-white/10 bg-white/5"
            }`}
            onDragEnter={(e) => {
              e.preventDefault();
              e.stopPropagation();
              setDropZoneActive(true);
            }}
            onDragOver={(e) => {
              e.preventDefault();
              e.stopPropagation();
              setDropZoneActive(true);
            }}
            onDragLeave={(e) => {
              e.preventDefault();
              e.stopPropagation();
              // Check if we're actually leaving the element
              if (!sharedListRef.current?.contains(e.relatedTarget as Node)) {
                setDropZoneActive(false);
              }
            }}
            onDrop={(e) => {
              e.preventDefault();
              e.stopPropagation();
              setDropZoneActive(false);
              onDropFiles(e.dataTransfer.files);
            }}
          >
            {/* List Header */}
            <div className="flex items-center justify-between border-b border-white/10 px-4 py-3">
              <div className="flex items-center gap-3">
                <h2 className="text-sm font-semibold text-white">共享文件</h2>
                <span className="rounded-full bg-indigo-500/20 px-2 py-0.5 text-xs text-indigo-200">
                  {sharedList.length} 项
                </span>
              </div>
              <div className="flex items-center gap-2">
                <span className="text-xs text-white/40">
                  {sharedList.length === 0 ? "拖入文件到此处" : "局域网内所有设备可访问"}
                </span>
                {sharedList.length > 0 && (
                  <button
                    className="subtle-button text-xs"
                    onClick={async () => {
                      await invoke("clear_shared_command");
                      setSharedList([]);
                    }}
                  >
                    清空
                  </button>
                )}
              </div>
            </div>

            {/* Empty State / File List */}
            <div className="flex-1 overflow-y-auto p-2">
              <AnimatePresence>
                {sharedList.length === 0 ? (
                  <motion.div
                    initial={{ opacity: 0, y: 10 }}
                    animate={{ opacity: 1, y: 0 }}
                    exit={{ opacity: 0, y: -10 }}
                    className="flex h-full flex-col items-center justify-center gap-4 text-center"
                  >
                    <div className={`flex h-20 w-20 items-center justify-center rounded-2xl border-2 border-dashed transition-colors ${
                      dropZoneActive ? "border-indigo-400 bg-indigo-500/20" : "border-white/20 bg-white/5"
                    }`}>
                      <svg className="h-8 w-8 text-white/40" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                        <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.5} d="M7 16a4 4 0 01-.88-7.903A5 5 0 1115.9 6L16 6a5 5 0 011 9.9M15 13l-3-3m0 0l-3 3m3-3v12" />
                      </svg>
                    </div>
                    <div>
                      <p className="text-sm font-medium text-white/80">拖入文件到此区域</p>
                      <p className="text-xs text-white/40 mt-1">支持多文件、文件夹</p>
                    </div>
                  </motion.div>
                ) : (
                  <motion.div
                    initial="hidden"
                    animate="visible"
                    variants={{
                      hidden: { opacity: 0 },
                      visible: { opacity: 1, transition: { staggerChildren: 0.05 } }
                    }}
                    className="grid grid-cols-1 gap-2 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4"
                  >
                    {sharedList.map((entry) => (
                      <motion.div
                        key={entry.id}
                        variants={{
                          hidden: { opacity: 0, scale: 0.95 },
                          visible: { opacity: 1, scale: 1 }
                        }}
                        className="group relative flex flex-col rounded-xl border border-white/10 bg-white/5 p-3 transition hover:border-white/20 hover:bg-white/10"
                      >
                        <div className="flex items-start justify-between">
                          <div className="flex min-w-0 flex-1 items-center gap-2">
                            <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded-lg bg-indigo-500/20">
                              <svg className="h-5 w-5 text-indigo-300" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.5} d="M9 12h6m-6 4h6m2 5H7a2 2 0 01-2-2V5a2 2 0 012-2h5.586a1 1 0 01.707.293l5.414 5.414a1 1 0 01.293.707V19a2 2 0 01-2 2z" />
                              </svg>
                            </div>
                            <div className="min-w-0 flex-1">
                              <p className="truncate text-sm font-medium text-white">{entry.name}</p>
                              <p className="text-xs text-white/40">
                                {(entry.size / 1024 / 1024).toFixed(2)} MB
                              </p>
                            </div>
                          </div>
                        </div>
                        <p className="mt-2 truncate text-[10px] text-white/30">{entry.path}</p>
                      </motion.div>
                    ))}
                  </motion.div>
                )}
              </AnimatePresence>
            </div>

            {/* Drop Overlay */}
            {dropZoneActive && (
              <motion.div
                initial={{ opacity: 0 }}
                animate={{ opacity: 1 }}
                className="pointer-events-none absolute inset-0 flex items-center justify-center bg-indigo-500/10 backdrop-blur-sm"
              >
                <div className="rounded-2xl border border-indigo-400/60 bg-indigo-500/20 px-8 py-6 text-center">
                  <p className="text-lg font-semibold text-white">松开添加文件</p>
                  <p className="text-sm text-indigo-200/70">将共享给局域网内所有设备</p>
                </div>
              </motion.div>
            )}
          </div>

          {/* Bottom Area: Devices + Remote List + Progress */}
          <div className="grid grid-cols-[220px_1fr_280px] gap-4">
            {/* Left: Device List (Compact) */}
            <div className="flex flex-col overflow-hidden rounded-xl border border-white/10 bg-white/5">
              <div className="flex items-center justify-between border-b border-white/10 px-3 py-2">
                <span className="text-xs font-medium text-white/70">在线设备</span>
                <span className="text-[10px] text-white/40">{devices.length}</span>
              </div>
              <div className="flex-1 overflow-y-auto p-2">
                {devices.length === 0 ? (
                  <div className="flex h-full flex-col items-center justify-center gap-2 text-center">
                    <div className="h-2 w-2 animate-pulse rounded-full bg-white/20" />
                    <p className="text-[10px] text-white/30">搜索中...</p>
                  </div>
                ) : (
                  <div className="space-y-1">
                    {devices.map((device) => {
                      const active = activeDevice?.machine_id === device.machine_id;
                      return (
                        <button
                          key={device.machine_id}
                          onClick={() => setActiveDevice(device)}
                          className={`flex w-full items-center gap-2 rounded-lg px-2 py-2 text-left transition ${
                            active ? "bg-indigo-500/20" : "hover:bg-white/5"
                          }`}
                        >
                          <span className="h-1.5 w-1.5 rounded-full bg-emerald-400" />
                          <div className="min-w-0 flex-1">
                            <p className={`truncate text-xs ${active ? "text-white" : "text-white/70"}`}>
                              {device.name}
                            </p>
                            <p className="truncate text-[10px] text-white/40">{device.ip}</p>
                          </div>
                        </button>
                      );
                    })}
                  </div>
                )}
              </div>
              {/* Download Dir */}
              <div className="border-t border-white/10 p-2">
                <p className="text-[10px] text-white/40">下载目录</p>
                <div className="mt-1 flex items-center gap-1">
                  <button
                    className="subtle-button py-1 px-2 text-[10px]"
                    onClick={chooseDownloadDir}
                  >
                    选择
                  </button>
                  <span className="truncate text-[10px] text-white/40">
                    {downloadDir ? downloadDir.split(/[\\/]/).pop() : "未设置"}
                  </span>
                </div>
              </div>
            </div>

            {/* Middle: Remote Shared List */}
            <div className="flex flex-col overflow-hidden rounded-xl border border-white/10 bg-white/5">
              <div className="flex items-center justify-between border-b border-white/10 px-3 py-2">
                <span className="text-xs font-medium text-white/70">
                  {activeDevice ? `来自「${activeDevice.name}」` : "选择设备查看共享"}
                </span>
                {activeDevice && (
                  <button
                    className="subtle-button py-1 px-2 text-[10px]"
                    onClick={() =>
                      invoke<SharedEntry[]>("fetch_remote_list_command", {
                        targetIp: activeDevice.ip,
                        targetPort: activeDevice.port,
                      }).then((list) => setRemoteList(list ?? []))
                    }
                  >
                    刷新
                  </button>
                )}
              </div>
              <div className="flex-1 overflow-y-auto p-2">
                {!activeDevice ? (
                  <div className="flex h-full items-center justify-center text-center">
                    <p className="text-xs text-white/30">点击左侧设备查看其共享</p>
                  </div>
                ) : remoteList.length === 0 ? (
                  <div className="flex h-full items-center justify-center text-center">
                    <p className="text-xs text-white/30">该设备暂无共享文件</p>
                  </div>
                ) : (
                  <div className="space-y-1">
                    {remoteList.map((entry) => (
                      <div
                        key={entry.id}
                        className="flex items-center gap-2 rounded-lg bg-white/5 px-2 py-2 hover:bg-white/10"
                      >
                        <div className="flex h-6 w-6 shrink-0 items-center justify-center rounded bg-indigo-500/20">
                          <svg className="h-3 w-3 text-indigo-300" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M9 12h6m-6 4h6m2 5H7a2 2 0 01-2-2V5a2 2 0 012-2h5.586a1 1 0 01.707.293l5.414 5.414a1 1 0 01.293.707V19a2 2 0 01-2 2z" />
                          </svg>
                        </div>
                        <div className="min-w-0 flex-1">
                          <p className="truncate text-xs text-white/80">{entry.name}</p>
                          <p className="text-[10px] text-white/40">{(entry.size / 1024 / 1024).toFixed(2)} MB</p>
                        </div>
                        <div className="flex shrink-0 gap-1">
                          <button
                            className="rounded bg-indigo-500/80 px-2 py-1 text-[10px] text-white transition hover:bg-indigo-400 disabled:opacity-50"
                            onClick={() => pullEntry(entry)}
                            disabled={!downloadDir || pullingId === entry.id}
                          >
                            {pullingId === entry.id ? "..." : "拉取"}
                          </button>
                          <button
                            className="rounded border border-white/10 bg-white/5 px-2 py-1 text-[10px] text-white/70 transition hover:bg-white/10"
                            onMouseDown={(e) => {
                              e.preventDefault();
                              void dragRemoteEntry(entry);
                            }}
                            disabled={draggingId === entry.id || !activeDevice}
                            title="拖出到文件夹"
                          >
                            {draggingId === entry.id ? "..." : "拖出"}
                          </button>
                        </div>
                      </div>
                    ))}
                  </div>
                )}
              </div>
            </div>

            {/* Right: Transfer Progress (Simplified) */}
            <div className="flex flex-col overflow-hidden rounded-xl border border-white/10 bg-white/5">
              <div className="flex items-center border-b border-white/10 px-3 py-2">
                <span className="text-xs font-medium text-white/70">传输状态</span>
              </div>
              <div className="flex flex-1 flex-col justify-center gap-3 p-3">
                {/* Status Message */}
                {statusMessage && (
                  <div
                    className={`rounded-lg px-3 py-2 text-[11px] ${
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

                {/* Current Transfer */}
                {pullProgress ? (
                  <div className="space-y-2">
                    <div className="flex items-center justify-between text-xs text-white/50">
                      <span className="truncate">{pullProgress.name}</span>
                      <span>{progress}%</span>
                    </div>
                    <div className="h-1.5 overflow-hidden rounded-full bg-white/10">
                      <motion.div
                        className="h-1.5 rounded-full bg-gradient-to-r from-indigo-500 to-cyan-400"
                        initial={{ width: 0 }}
                        animate={{ width: `${progress}%` }}
                        transition={{ duration: 0.3, ease: "easeOut" }}
                      />
                    </div>
                    <div className="text-[10px] text-white/40">
                      {(pullProgress.received_bytes / 1024 / 1024).toFixed(2)} / {" "}
                      {(pullProgress.total_bytes / 1024 / 1024).toFixed(2)} MB
                    </div>
                  </div>
                ) : (
                  <div className="text-center text-white/30">
                    <p className="text-xs">空闲中</p>
                  </div>
                )}
              </div>
            </div>
          </div>
        </section>
      </div>

      {/* Legacy Drop Overlay (for whole window) */}
      {dropActive && (
        <div className="pointer-events-none absolute inset-0 flex items-center justify-center bg-white/5 backdrop-blur-sm">
          <div className="glass-card px-8 py-6 text-center">
            <p className="text-lg font-semibold text-white">松开即可发送</p>
            <p className="text-xs text-white/50">将文件添加到共享列表</p>
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
