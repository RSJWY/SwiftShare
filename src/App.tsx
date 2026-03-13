import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open } from "@tauri-apps/plugin-dialog";
import { Store } from "@tauri-apps/plugin-store";
import { startDrag } from "@crabnebula/tauri-plugin-drag";
import { motion } from "framer-motion";
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
  relative_path: string;
  is_dir: boolean;
};

type PullProgress = {
  entry_id: string;
  name: string;
  received_bytes: number;
  total_bytes: number;
  entry_received_bytes: number;
  entry_total_bytes: number;
};

type DirFileInfo = {
  path: string;
  size: number;
};

type ConflictInfo = {
  has_conflict: boolean;
  conflicting_files: string[];
  total_conflict_size: number;
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

// Format file size to human-readable format
function formatFileSize(bytes: number): string {
  if (bytes === 0) return "0 B";
  const k = 1024;
  const sizes = ["B", "KB", "MB", "GB"];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  const value = bytes / Math.pow(k, i);

  // For sizes < 1 MB, show 0 decimals; for >= 1 MB, show 2 decimals
  const decimals = i < 2 ? 0 : 2;
  return value.toFixed(decimals) + " " + sizes[i];
}

// Format remaining time
function formatEta(seconds: number): string {
  if (seconds <= 0 || !isFinite(seconds)) return "";
  if (seconds < 60) return `${Math.ceil(seconds)}s`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m ${Math.ceil(seconds % 60)}s`;
  return `${Math.floor(seconds / 3600)}h ${Math.floor((seconds % 3600) / 60)}m`;
}

const SETTINGS_STORE_PATH = "settings.json";

function App() {
  const [devices, setDevices] = useState<DeviceInfo[]>([]);
  const [activeDevice, setActiveDevice] = useState<DeviceInfo | null>(null);
  const [progress, setProgress] = useState(0);
  const [sharedList, setSharedList] = useState<SharedEntry[]>([]);
  const [remoteList, setRemoteList] = useState<SharedEntry[]>([]);
  const [downloadDir, setDownloadDir] = useState<string>("");
  const [statusMessage, setStatusMessage] = useState<string>("");
  const [statusType, setStatusType] = useState<"success" | "error" | "info">("info");
  const [pullingId, setPullingId] = useState<string | null>(null);
  const [pullProgress, setPullProgress] = useState<PullProgress | null>(null);
  const speedRef = useRef<{ lastBytes: number; lastTime: number; speed: number }>({ lastBytes: 0, lastTime: 0, speed: 0 });
  const [draggingId, setDraggingId] = useState<string | null>(null);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [settings, setSettings] = useState<Settings>(DEFAULT_SETTINGS);
  const [settingsLoading, setSettingsLoading] = useState(true);
  const [dropZoneActive, setDropZoneActive] = useState(false);
  const [localMachineId, setLocalMachineId] = useState<string>("");
  const [localPort, setLocalPort] = useState<number>(0);
  // Current browse path for local shared list and remote list (array of folder names)
  const [localBrowsePath, setLocalBrowsePath] = useState<string[]>([]);
  const [remoteBrowsePath, setRemoteBrowsePath] = useState<string[]>([]);
  const dropInProgressRef = useRef(false);
  // Resizable panel state
  const [bottomHeight, setBottomHeight] = useState(224);
  const [leftColWidth, setLeftColWidth] = useState(280);
  const [rightColWidth, setRightColWidth] = useState(260);
  // Conflict dialog state
  const [conflictInfo, setConflictInfo] = useState<ConflictInfo | null>(null);
  const conflictCallbackRef = useRef<(() => void) | null>(null);

  // Build a virtual tree node list for browsing.
  // Returns { name, isDir, entry? } for each visible item at the given path.
  type TreeNode = {
    name: string;
    isDir: boolean;
    entry: SharedEntry | null; // null for virtual sub-folders
    size: number;
    childCount: number;
  };

  function buildTreeNodes(list: SharedEntry[], browsePath: string[]): TreeNode[] {
    const nodes = new Map<string, TreeNode>();

    if (browsePath.length === 0) {
      // Root level: return all top-level entries directly
      for (const entry of list) {
        const childCount = entry.is_dir
          ? (dirFilesRef.current.get(entry.id) ?? []).length
          : 0;
        nodes.set(entry.name, {
          name: entry.name,
          isDir: entry.is_dir,
          entry,
          size: entry.size,
          childCount,
        });
      }
      return Array.from(nodes.values()).sort((a, b) => {
        if (a.isDir !== b.isDir) return a.isDir ? -1 : 1;
        return a.name.localeCompare(b.name);
      });
    }

    // Inside a directory: find the root dir entry
    const rootName = browsePath[0];
    const dirEntry = list.find((e) => e.is_dir && e.name === rootName);
    if (!dirEntry) return [];

    // files contains {path, size} like {path: "MyFolder/sub/file.txt", size: 1234}
    // browsePath = ["MyFolder", "sub"] → fullPrefix = "MyFolder/sub/"
    const files = dirFilesRef.current.get(dirEntry.id) ?? [];
    const fullPrefix = browsePath.join("/") + "/";

    for (const fileInfo of files) {
      if (!fileInfo.path.startsWith(fullPrefix)) continue;
      const rest = fileInfo.path.slice(fullPrefix.length);
      if (!rest) continue;
      const slashIdx = rest.indexOf("/");
      if (slashIdx === -1) {
        // Direct file child
        if (!nodes.has(rest)) {
          nodes.set(rest, { name: rest, isDir: false, entry: dirEntry, size: fileInfo.size, childCount: 0 });
        }
      } else {
        // Sub-folder
        const subFolder = rest.slice(0, slashIdx);
        if (!nodes.has(subFolder)) {
          nodes.set(subFolder, { name: subFolder, isDir: true, entry: dirEntry, size: 0, childCount: 0 });
        }
        const node = nodes.get(subFolder)!;
        node.childCount++;
        node.size += fileInfo.size;
      }
    }
    return Array.from(nodes.values()).sort((a, b) => {
      if (a.isDir !== b.isDir) return a.isDir ? -1 : 1;
      return a.name.localeCompare(b.name);
    });
  }

  // dirFilesRef: maps dir entry id -> array of file info (path + size)
  const dirFilesRef = useRef<Map<string, DirFileInfo[]>>(new Map());

  useEffect(() => {
    invoke<string>("get_local_machine_id_command")
      .then(setLocalMachineId)
      .catch((err) => {
        console.warn("Failed to get local machine ID:", err);
        setLocalMachineId("");
      });
    invoke<number>("get_local_port_command")
      .then(setLocalPort)
      .catch((err) => {
        console.warn("Failed to get local port:", err);
        setLocalPort(0);
      });
  }, []);

  useEffect(() => {
    const unlistenPromise = listen<DeviceInfo[]>("device-list-updated", (event) => {
      setDevices(event.payload ?? []);
    });

    return () => {
      unlistenPromise.then((unlisten) => unlisten());
    };
  }, []);

  useEffect(() => {
    const win = getCurrentWindow();
    const unlistenPromise = win.onCloseRequested(async (event) => {
      event.preventDefault();
      await invoke("notify_offline_command").catch(() => {});
      await win.destroy();
    });
    return () => {
      unlistenPromise.then((unlisten) => unlisten());
    };
  }, []);

  useEffect(() => {
    const unlistenPromise = listen<PullProgress>("pull-progress", (event) => {
      setPullProgress(event.payload ?? null);
      if (event.payload) {
        const p = event.payload;
        // Overall progress based on entry totals
        const percent = p.entry_total_bytes > 0
          ? Math.min(100, Math.round((p.entry_received_bytes / p.entry_total_bytes) * 100))
          : 0;
        setProgress(percent);
        // Speed calculation (smoothed)
        const now = Date.now();
        const sr = speedRef.current;
        const dt = now - sr.lastTime;
        if (dt > 300 && sr.lastTime > 0) {
          const bytesPerSec = ((p.entry_received_bytes - sr.lastBytes) / dt) * 1000;
          sr.speed = sr.speed > 0 ? sr.speed * 0.7 + bytesPerSec * 0.3 : bytesPerSec;
          sr.lastBytes = p.entry_received_bytes;
          sr.lastTime = now;
        } else if (sr.lastTime === 0) {
          sr.lastBytes = p.entry_received_bytes;
          sr.lastTime = now;
        }
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
    setRemoteBrowsePath([]);
    invoke<SharedEntry[]>("fetch_remote_list_command", {
      targetIp: activeDevice.ip,
      targetPort: activeDevice.port,
    }).then(async (list) => {
      const entries = list ?? [];
      setRemoteList(entries);
      // Pre-fetch file trees for remote directory entries so we can browse them
      for (const entry of entries) {
        if (entry.is_dir) {
          try {
            const files = await invoke<DirFileInfo[]>("fetch_remote_dir_files_command", {
              entryId: entry.id,
              targetIp: activeDevice.ip,
              targetPort: activeDevice.port,
            });
            dirFilesRef.current.set(entry.id, files);
          } catch {
            // ignore
          }
        }
      }
    });
  }, [activeDevice]);

  // Auto-refresh remote shared list periodically
  useEffect(() => {
    if (!activeDevice) return;
    const intervalId = setInterval(async () => {
      // Skip refresh while pulling to avoid TCP connection contention
      if (pullingId) return;
      try {
        const list = await invoke<SharedEntry[]>("fetch_remote_list_command", {
          targetIp: activeDevice.ip,
          targetPort: activeDevice.port,
        });
        const entries = list ?? [];
        setRemoteList(entries);
        // Update dirFilesRef for directory entries, clean stale keys
        const newIds = new Set(entries.filter(e => e.is_dir).map(e => e.id));
        for (const key of dirFilesRef.current.keys()) {
          if (!newIds.has(key)) dirFilesRef.current.delete(key);
        }
        for (const entry of entries) {
          if (entry.is_dir) {
            try {
              const files = await invoke<DirFileInfo[]>("fetch_remote_dir_files_command", {
                entryId: entry.id,
                targetIp: activeDevice.ip,
                targetPort: activeDevice.port,
              });
              dirFilesRef.current.set(entry.id, files);
            } catch { /* ignore */ }
          }
        }
      } catch { /* device may have gone offline */ }
    }, settings.discoveryIntervalMs);
    return () => clearInterval(intervalId);
  }, [activeDevice, settings.discoveryIntervalMs]);

  const addSharedFiles = async (paths: string[]) => {
    if (paths.length === 0) return;
    try {
      await invoke<SharedEntry[]>("add_shared_command", { paths });
      const updated = await invoke<SharedEntry[]>("list_shared_command");
      const entries = updated ?? [];
      setSharedList(entries);
      // Pre-fetch file trees for local directory entries
      for (const entry of entries) {
        if (entry.is_dir && !dirFilesRef.current.has(entry.id)) {
          try {
            const files = await invoke<DirFileInfo[]>("list_dir_files_command", { entryId: entry.id });
            dirFilesRef.current.set(entry.id, files);
          } catch {
            // ignore
          }
        }
      }
      setStatusType("success");
      setStatusMessage(`已添加 ${paths.length} 个文件`);
    } catch (error) {
      console.error("add_shared failed:", error);
      setStatusType("error");
      setStatusMessage(`添加失败: ${String(error)}`);
    }
  };

  // Use Tauri's native drag-drop API to get file paths
  useEffect(() => {
    const webview = getCurrentWebview();
    const unlistenPromise = webview.onDragDropEvent((event) => {
      if (event.payload.type === "enter") {
        setDropZoneActive(true);
      } else if (event.payload.type === "leave") {
        setDropZoneActive(false);
      } else if (event.payload.type === "drop") {
        setDropZoneActive(false);
        if (dropInProgressRef.current) return;
        const paths = event.payload.paths;
        if (paths && paths.length > 0) {
          dropInProgressRef.current = true;
          addSharedFiles(paths).finally(() => { dropInProgressRef.current = false; });
        }
      }
    });
    return () => {
      unlistenPromise.then((unlisten) => unlisten());
    };
  }, [addSharedFiles]);

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
        entrySize: entry.size,
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

      <div className="relative mx-auto flex h-full w-full flex-col gap-4 px-6 py-6">
        {/* Header - Compact */}
        <header className="flex items-center justify-between">
          <div className="flex items-center gap-4">
            <div>
              <p className="text-xs uppercase tracking-[0.3em] text-indigo-200/70">SwiftShare</p>
              <h1 className="text-xl font-semibold text-white">局域网文件共享</h1>
            </div>
            <div className="flex items-center gap-2 text-xs text-white/50">
              <span className="flex h-2 w-2 rounded-full bg-emerald-400" />
              {(() => {
                const remoteDevices = localMachineId
                  ? devices.filter(d => !(d.machine_id === localMachineId && d.port === localPort))
                  : devices;
                return remoteDevices.length > 0
                  ? `${remoteDevices.length} 台设备在线`
                  : "等待设备加入";
              })()}
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
            className={`relative flex flex-1 flex-col overflow-hidden rounded-2xl border transition-colors ${
              dropZoneActive
                ? "border-indigo-400/60 bg-indigo-500/10"
                : "border-white/10 bg-white/5"
            }`}
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
                      setLocalBrowsePath([]);
                      dirFilesRef.current.clear();
                    }}
                  >
                    清空
                  </button>
                )}
              </div>
            </div>

            {/* Empty State / File List */}
            <div className="flex-1 overflow-y-auto p-2">
              {sharedList.length === 0 ? (
                <div className="flex h-full flex-col items-center justify-center gap-4 text-center">
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
                </div>
              ) : (
                <div className="space-y-1">
                  {/* Breadcrumb */}
                  {localBrowsePath.length > 0 && (
                    <div className="flex items-center gap-1 px-1 pb-2 text-[10px] text-white/40">
                      <button className="hover:text-white/70" onClick={() => setLocalBrowsePath([])}>
                        共享根目录
                      </button>
                      {localBrowsePath.map((seg, i) => (
                        <span key={i} className="flex items-center gap-1">
                          <span>/</span>
                          <button
                            className="hover:text-white/70"
                            onClick={() => setLocalBrowsePath(localBrowsePath.slice(0, i + 1))}
                          >
                            {seg}
                          </button>
                        </span>
                      ))}
                    </div>
                  )}
                  {buildTreeNodes(sharedList, localBrowsePath).map((node) => (
                    <div
                      key={node.name}
                      className={`flex items-center gap-2 rounded-lg px-2 py-2 transition ${
                        node.isDir ? "hover:bg-white/8 cursor-pointer" : "hover:bg-white/5"
                      }`}
                      onClick={() => {
                        if (node.isDir) setLocalBrowsePath([...localBrowsePath, node.name]);
                      }}
                    >
                      <div className={`flex h-7 w-7 shrink-0 items-center justify-center rounded-lg ${
                        node.isDir ? "bg-amber-500/20" : "bg-white/10"
                      }`}>
                        {node.isDir ? (
                          <svg className="h-4 w-4 text-amber-300" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.5} d="M3 7v10a2 2 0 002 2h14a2 2 0 002-2V9a2 2 0 00-2-2h-6l-2-2H5a2 2 0 00-2 2z" />
                          </svg>
                        ) : (
                          <svg className="h-3.5 w-3.5 text-white/50" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M9 12h6m-6 4h6m2 5H7a2 2 0 01-2-2V5a2 2 0 012-2h5.586a1 1 0 01.707.293l5.414 5.414a1 1 0 01.293.707V19a2 2 0 01-2 2z" />
                          </svg>
                        )}
                      </div>
                      <div className="min-w-0 flex-1">
                        <p className="truncate text-xs text-white/80">{node.name}</p>
                        <p className="text-[10px] text-white/40">
                          {node.isDir
                            ? `${node.childCount} 项${node.size > 0 ? " · " + formatFileSize(node.size) : ""}`
                            : formatFileSize(node.size)}
                        </p>
                      </div>
                      {node.isDir && (
                        <svg className="h-3.5 w-3.5 shrink-0 text-white/30" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                          <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M9 5l7 7-7 7" />
                        </svg>
                      )}
                    </div>
                  ))}
                </div>
              )}
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
          {/* Horizontal resize handle */}
          <div
            className="flex h-2 cursor-row-resize items-center justify-center group shrink-0"
            onMouseDown={(e) => {
              e.preventDefault();
              const startY = e.clientY;
              const startH = bottomHeight;
              const onMove = (ev: MouseEvent) => {
                const delta = startY - ev.clientY;
                setBottomHeight(Math.max(160, Math.min(480, startH + delta)));
              };
              const onUp = () => {
                window.removeEventListener('mousemove', onMove);
                window.removeEventListener('mouseup', onUp);
              };
              window.addEventListener('mousemove', onMove);
              window.addEventListener('mouseup', onUp);
            }}
          >
            <div className="h-0.5 w-12 rounded-full bg-white/20 transition group-hover:bg-white/50" />
          </div>
          <div
            className="grid gap-0 shrink-0"
            style={{ height: bottomHeight, gridTemplateColumns: `${leftColWidth}px 6px 1fr 6px ${rightColWidth}px` }}
          >
            {/* Left: Device List (Compact) */}
            <div className="flex flex-col overflow-hidden rounded-xl border border-white/10 bg-white/5">
              <div className="flex items-center justify-between border-b border-white/10 px-3 py-2">
                <span className="text-xs font-medium text-white/70">在线设备</span>
                <div className="flex items-center gap-2">
                  <span className="text-[10px] text-white/40">
                    {(() => {
                      const remoteDevices = localMachineId
                        ? devices.filter(d => !(d.machine_id === localMachineId && d.port === localPort))
                        : devices;
                      return remoteDevices.length;
                    })()}
                  </span>
                  <button
                    className="subtle-button py-0.5 px-1.5 text-[10px]"
                    onClick={() => invoke("refresh_discovery_command").catch(() => {})}
                    title="刷新"
                    type="button"
                  >
                    ↻
                  </button>
                </div>
              </div>
              <div className="flex-1 overflow-y-auto p-2">
                {(() => {
                  const remoteDevices = localMachineId
                    ? devices.filter(d => !(d.machine_id === localMachineId && d.port === localPort))
                    : devices;
                  return remoteDevices.length === 0 ? (
                    <div className="flex h-full flex-col items-center justify-center gap-2 text-center">
                      <div className="h-2 w-2 animate-pulse rounded-full bg-white/20" />
                      <p className="text-[10px] text-white/30">搜索中...</p>
                    </div>
                  ) : (
                    <div className="space-y-1">
                      {remoteDevices.map((device) => {
                        const active = activeDevice?.machine_id === device.machine_id && activeDevice?.port === device.port;
                        const hasDupName = remoteDevices.filter(d => d.name === device.name).length > 1;
                        return (
                          <button
                            key={`${device.machine_id}:${device.port}`}
                            onClick={() => setActiveDevice(device)}
                            className={`flex w-full items-center gap-2 rounded-lg px-2 py-2 text-left transition ${
                              active ? "bg-indigo-500/20" : "hover:bg-white/5"
                            }`}
                            title={`${device.name} (${device.machine_id.slice(0, 8)}...)`}
                          >
                            <span className="h-1.5 w-1.5 rounded-full bg-emerald-400" />
                            <div className="min-w-0 flex-1">
                              <p className={`truncate text-xs ${active ? "text-white" : "text-white/70"}`}>
                                {device.name}{hasDupName ? `:${device.port}` : ""}
                              </p>
                              <p className="truncate text-[10px] text-white/40">{device.ip}:{device.port}</p>
                            </div>
                          </button>
                        );
                      })}
                    </div>
                  );
                })()}
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

            {/* Vertical resize handle: left | middle */}
            <div
              className="flex cursor-col-resize items-center justify-center group"
              onMouseDown={(e) => {
                e.preventDefault();
                const startX = e.clientX;
                const startW = leftColWidth;
                const onMove = (ev: MouseEvent) => {
                  setLeftColWidth(Math.max(160, Math.min(400, startW + ev.clientX - startX)));
                };
                const onUp = () => {
                  window.removeEventListener('mousemove', onMove);
                  window.removeEventListener('mouseup', onUp);
                };
                window.addEventListener('mousemove', onMove);
                window.addEventListener('mouseup', onUp);
              }}
            >
              <div className="h-12 w-0.5 rounded-full bg-white/20 transition group-hover:bg-white/50" />
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
                    onClick={async () => {
                      if (!activeDevice) return;
                      setRemoteBrowsePath([]);
                      const list = await invoke<SharedEntry[]>("fetch_remote_list_command", {
                        targetIp: activeDevice.ip,
                        targetPort: activeDevice.port,
                      });
                      const entries = list ?? [];
                      setRemoteList(entries);
                      for (const entry of entries) {
                        if (entry.is_dir) {
                          try {
                            const files = await invoke<DirFileInfo[]>("fetch_remote_dir_files_command", {
                              entryId: entry.id,
                              targetIp: activeDevice.ip,
                              targetPort: activeDevice.port,
                            });
                            dirFilesRef.current.set(entry.id, files);
                          } catch { /* ignore */ }
                        }
                      }
                    }}
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
                    {/* Remote Breadcrumb */}
                    {remoteBrowsePath.length > 0 && (
                      <div className="flex items-center gap-1 px-1 pb-2 text-[10px] text-white/40">
                        <button className="hover:text-white/70" onClick={() => setRemoteBrowsePath([])}>
                          根目录
                        </button>
                        {remoteBrowsePath.map((seg, i) => (
                          <span key={i} className="flex items-center gap-1">
                            <span>/</span>
                            <button
                              className="hover:text-white/70"
                              onClick={() => setRemoteBrowsePath(remoteBrowsePath.slice(0, i + 1))}
                            >
                              {seg}
                            </button>
                          </span>
                        ))}
                      </div>
                    )}
                    {buildTreeNodes(remoteList, remoteBrowsePath).map((node) => (
                      <div
                        key={node.name}
                        className="flex items-center gap-2 rounded-lg bg-white/5 px-2 py-2 hover:bg-white/10"
                      >
                        <div className={`flex h-6 w-6 shrink-0 items-center justify-center rounded ${
                          node.isDir ? "bg-amber-500/20" : "bg-indigo-500/20"
                        }`}>
                          {node.isDir ? (
                            <svg className="h-3.5 w-3.5 text-amber-300" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                              <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.5} d="M3 7v10a2 2 0 002 2h14a2 2 0 002-2V9a2 2 0 00-2-2h-6l-2-2H5a2 2 0 00-2 2z" />
                            </svg>
                          ) : (
                            <svg className="h-3 w-3 text-indigo-300" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                              <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M9 12h6m-6 4h6m2 5H7a2 2 0 01-2-2V5a2 2 0 012-2h5.586a1 1 0 01.707.293l5.414 5.414a1 1 0 01.293.707V19a2 2 0 01-2 2z" />
                            </svg>
                          )}
                        </div>
                        <div className="min-w-0 flex-1">
                          <p className="truncate text-xs text-white/80">{node.name}</p>
                          <p className="text-[10px] text-white/40">
                            {node.isDir ? `${node.childCount} 项${node.size > 0 ? " · " + formatFileSize(node.size) : ""}` : formatFileSize(node.size)}
                          </p>
                        </div>
                        {node.isDir ? (
                          <div className="flex shrink-0 gap-1">
                            <button
                              className="shrink-0 rounded bg-indigo-500/80 px-2 py-1 text-[10px] text-white transition hover:bg-indigo-400 disabled:opacity-50"
                              onClick={() => setRemoteBrowsePath([...remoteBrowsePath, node.name])}
                            >
                              进入
                            </button>
                            {remoteBrowsePath.length === 0 && node.entry && (
                              <button
                                className="shrink-0 rounded border border-white/10 bg-white/5 px-2 py-1 text-[10px] text-white/70 transition hover:bg-white/10 disabled:opacity-50"
                                onClick={async () => {
                                  let dir = downloadDir;
                                  if (!dir) {
                                    const folder = await open({ directory: true, multiple: false });
                                    if (typeof folder !== "string") return;
                                    setSettings(prev => ({ ...prev, downloadDir: folder }));
                                    setDownloadDir(folder);
                                    dir = folder;
                                  }
                                  if (node.entry) {
                                    const doPull = async () => {
                                      setPullingId(node.entry!.id);
                                      setProgress(0);
                                      setStatusType("info");
                                      setStatusMessage(`正在拉取 ${node.entry!.name}...`);
                                      setPullProgress(null);
                                      speedRef.current = { lastBytes: 0, lastTime: 0, speed: 0 };
                                      try {
                                        await invoke("pull_file_command", {
                                          entryId: node.entry!.id,
                                          targetIp: activeDevice!.ip,
                                          targetPort: activeDevice!.port,
                                          destDir: dir,
                                          entrySize: node.entry!.size,
                                        });
                                        setStatusType("success");
                                        setStatusMessage(`已完成 ${node.entry!.name}`);
                                        setPullProgress(null);
                                        speedRef.current = { lastBytes: 0, lastTime: 0, speed: 0 };
                                      } catch (error) {
                                        setStatusType("error");
                                        setStatusMessage(`拉取失败: ${String(error)}`);
                                        setPullProgress(null);
                                        speedRef.current = { lastBytes: 0, lastTime: 0, speed: 0 };
                                      } finally {
                                        setPullingId(null);
                                      }
                                    };
                                    try {
                                      const conflict = await invoke<ConflictInfo>("check_pull_conflict_command", {
                                        entryName: node.entry.name,
                                        entryIsDir: node.entry.is_dir,
                                        entryId: node.entry.id,
                                        targetIp: activeDevice!.ip,
                                        targetPort: activeDevice!.port,
                                        destDir: dir,
                                      });
                                      if (conflict.has_conflict) {
                                        setConflictInfo(conflict);
                                        conflictCallbackRef.current = doPull;
                                      } else {
                                        await doPull();
                                      }
                                    } catch {
                                      await doPull();
                                    }
                                  }
                                }}
                                disabled={node.entry ? pullingId === node.entry.id : false}
                              >
                                {node.entry && pullingId === node.entry.id ? "..." : "拉取全部"}
                              </button>
                            )}
                          </div>
                        ) : (
                          <div className="flex shrink-0 gap-1">
                            <button
                              className="rounded bg-indigo-500/80 px-2 py-1 text-[10px] text-white transition hover:bg-indigo-400 disabled:opacity-50"
                              onClick={async () => {
                                let dir = downloadDir;
                                if (!dir) {
                                  const folder = await open({ directory: true, multiple: false });
                                  if (typeof folder !== "string") return;
                                  setSettings(prev => ({ ...prev, downloadDir: folder }));
                                  setDownloadDir(folder);
                                  dir = folder;
                                }
                                if (node.entry) {
                                  const doPull = async () => {
                                    setPullingId(node.entry!.id);
                                    setProgress(0);
                                    setStatusType("info");
                                    setStatusMessage(`正在拉取 ${node.entry!.name}...`);
                                    setPullProgress(null);
                                    speedRef.current = { lastBytes: 0, lastTime: 0, speed: 0 };
                                    try {
                                      await invoke("pull_file_command", {
                                        entryId: node.entry!.id,
                                        targetIp: activeDevice!.ip,
                                        targetPort: activeDevice!.port,
                                        destDir: dir,
                                        entrySize: node.entry!.size,
                                      });
                                      setStatusType("success");
                                      setStatusMessage(`已完成 ${node.entry!.name}`);
                                      setPullProgress(null);
                                      speedRef.current = { lastBytes: 0, lastTime: 0, speed: 0 };
                                    } catch (error) {
                                      setStatusType("error");
                                      setStatusMessage(`拉取失败: ${String(error)}`);
                                      setPullProgress(null);
                                      speedRef.current = { lastBytes: 0, lastTime: 0, speed: 0 };
                                    } finally {
                                      setPullingId(null);
                                    }
                                  };
                                  try {
                                    const conflict = await invoke<ConflictInfo>("check_pull_conflict_command", {
                                      entryName: node.entry.name,
                                      entryIsDir: node.entry.is_dir,
                                      entryId: node.entry.id,
                                      targetIp: activeDevice!.ip,
                                      targetPort: activeDevice!.port,
                                      destDir: dir,
                                    });
                                    if (conflict.has_conflict) {
                                      setConflictInfo(conflict);
                                      conflictCallbackRef.current = doPull;
                                    } else {
                                      await doPull();
                                    }
                                  } catch {
                                    await doPull();
                                  }
                                }
                              }}
                              disabled={node.entry ? pullingId === node.entry.id : false}
                            >
                              {node.entry && pullingId === node.entry.id ? "..." : "拉取"}
                            </button>
                            <button
                              className="rounded border border-white/10 bg-white/5 px-2 py-1 text-[10px] text-white/70 transition hover:bg-white/10"
                              onMouseDown={(e) => {
                                e.preventDefault();
                                if (node.entry) void dragRemoteEntry(node.entry);
                              }}
                              disabled={node.entry ? draggingId === node.entry.id : true}
                              title="拖出到文件夹"
                            >
                              {node.entry && draggingId === node.entry.id ? "..." : "拖出"}
                            </button>
                          </div>
                        )}
                      </div>
                    ))}
                  </div>
                )}
              </div>
            </div>

            {/* Vertical resize handle: middle | right */}
            <div
              className="flex cursor-col-resize items-center justify-center group"
              onMouseDown={(e) => {
                e.preventDefault();
                const startX = e.clientX;
                const startW = rightColWidth;
                const onMove = (ev: MouseEvent) => {
                  setRightColWidth(Math.max(180, Math.min(380, startW - (ev.clientX - startX))));
                };
                const onUp = () => {
                  window.removeEventListener('mousemove', onMove);
                  window.removeEventListener('mouseup', onUp);
                };
                window.addEventListener('mousemove', onMove);
                window.addEventListener('mouseup', onUp);
              }}
            >
              <div className="h-12 w-0.5 rounded-full bg-white/20 transition group-hover:bg-white/50" />
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
                    <div className="flex items-center justify-between text-[10px] text-white/40">
                      <span>
                        {formatFileSize(pullProgress.entry_received_bytes)} / {formatFileSize(pullProgress.entry_total_bytes)}
                        {speedRef.current.speed > 0 && ` · ${formatFileSize(speedRef.current.speed)}/s`}
                      </span>
                      <span>
                        {(() => {
                          const remaining = pullProgress.entry_total_bytes - pullProgress.entry_received_bytes;
                          const eta = speedRef.current.speed > 0 ? remaining / speedRef.current.speed : 0;
                          return eta > 0 ? formatEta(eta) : "";
                        })()}
                      </span>
                    </div>
                    <button
                      className="w-full rounded border border-rose-500/30 bg-rose-500/10 py-1 text-[10px] text-rose-300 transition hover:bg-rose-500/20"
                      onClick={async () => {
                        try {
                          await invoke("cancel_pull_command");
                          setStatusType("info");
                          setStatusMessage("已取消传输");
                          setPullProgress(null);
                          setPullingId(null);
                          speedRef.current = { lastBytes: 0, lastTime: 0, speed: 0 };
                        } catch { /* ignore */ }
                      }}
                    >
                      取消传输
                    </button>
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
      {dropZoneActive && (
        <div className="pointer-events-none absolute inset-0 flex items-center justify-center bg-white/5 backdrop-blur-sm">
          <div className="glass-card px-8 py-6 text-center">
            <p className="text-lg font-semibold text-white">松开即可发送</p>
            <p className="text-xs text-white/50">将文件添加到共享列表</p>
          </div>
        </div>
      )}

      {conflictInfo && (
        <div className="absolute inset-0 z-20 flex items-center justify-center bg-black/50 backdrop-blur-sm">
          <div className="glass-panel w-full max-w-md p-6">
            <h2 className="text-lg font-semibold text-white">文件冲突</h2>
            <p className="mt-3 text-sm text-white/70">
              目标目录中已存在 {conflictInfo.conflicting_files.length} 个同名文件
              （共 {formatFileSize(conflictInfo.total_conflict_size)}），继续拉取将覆盖这些文件。
            </p>
            <div className="mt-3 max-h-40 overflow-y-auto rounded bg-white/5 p-2">
              {conflictInfo.conflicting_files.slice(0, 50).map((f) => (
                <p key={f} className="truncate text-xs text-white/50">{f}</p>
              ))}
              {conflictInfo.conflicting_files.length > 50 && (
                <p className="text-xs text-white/40">...及另外 {conflictInfo.conflicting_files.length - 50} 个文件</p>
              )}
            </div>
            <div className="mt-4 flex justify-end gap-2">
              <button
                className="subtle-button"
                onClick={() => {
                  setConflictInfo(null);
                  conflictCallbackRef.current = null;
                }}
              >
                取消
              </button>
              <button
                className="glow-button"
                onClick={() => {
                  setConflictInfo(null);
                  const cb = conflictCallbackRef.current;
                  conflictCallbackRef.current = null;
                  if (cb) cb();
                }}
              >
                覆盖
              </button>
            </div>
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
