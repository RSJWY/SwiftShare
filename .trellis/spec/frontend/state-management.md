# State Management

> How state is managed in this project.

---

## Overview

SwiftShare uses **local component state** with React's built-in hooks. There's no global state management library (Redux, Zustand, etc.) - all state is local to the `App` component. This works well for the current small application size.

---

## State Categories

### 1. UI State

State that controls the visual appearance and user interaction:

```typescript
// Modal/panel visibility
const [settingsOpen, setSettingsOpen] = useState(false);
const [conflictInfo, setConflictInfo] = useState<ConflictInfo | null>(null);

// Loading states
const [settingsLoading, setSettingsLoading] = useState(true);
const [pullingId, setPullingId] = useState<string | null>(null);

// Visual feedback
const [dropZoneActive, setDropZoneActive] = useState(false);
const [draggingId, setDraggingId] = useState<string | null>(null);
```

### 2. Domain State

State representing the application's core data:

```typescript
// Devices
const [devices, setDevices] = useState<DeviceInfo[]>([]);
const [activeDevice, setActiveDevice] = useState<DeviceInfo | null>(null);

// Files
const [sharedList, setSharedList] = useState<SharedEntry[]>([]);
const [remoteList, setRemoteList] = useState<SharedEntry[]>([]);

// Transfer
const [progress, setProgress] = useState(0);
const [pullProgress, setPullProgress] = useState<PullProgress | null>(null);
```

### 3. Navigation State

State for UI navigation (browsing paths):

```typescript
// Breadcrumb navigation
const [localBrowsePath, setLocalBrowsePath] = useState<string[]>([]);
const [remoteBrowsePath, setRemoteBrowsePath] = useState<string[]>([]);
```

### 4. Configuration State

Application settings:

```typescript
const [settings, setSettings] = useState<Settings>(DEFAULT_SETTINGS);
const [downloadDir, setDownloadDir] = useState<string>("");
```

---

## State Patterns

### Ref vs State

| Use `useState` when... | Use `useRef` when... |
|------------------------|---------------------|
| Value affects UI rendering | Value is for internal computation |
| Need to trigger re-render | Don't want to trigger re-render |
| User-visible data | Temporary mutable values |

```typescript
// useState - triggers re-render
const [progress, setProgress] = useState(0);

// useRef - no re-render, persistent across renders
const speedRef = useRef<{ lastBytes: number; lastTime: number; speed: number }>({
  lastBytes: 0,
  lastTime: 0,
  speed: 0
});
```

### Derived State

Compute values during render rather than storing:

```typescript
// Computed inline - no need for separate state
const remoteDevices = localMachineId
  ? devices.filter(d => !(d.machine_id === localMachineId && d.port === localPort))
  : devices;

// Progress percentage computed from pullProgress
const percent = p.entry_total_bytes > 0
  ? Math.min(100, Math.round((p.entry_received_bytes / p.entry_total_bytes) * 100))
  : 0;
```

---

## When to Use Global State

**Current pattern**: All state is local to App component.

Promote to global (Context) when:
- Multiple components need the same data
- Prop drilling becomes excessive (>3 levels)
- State is truly application-wide (auth, theme)

For this small app, local state is appropriate.

---

## Server State (Tauri Backend)

### Tauri Command Invocation

Data from Rust backend is fetched via Tauri commands:

```typescript
// Fetch from backend
const list = await invoke<SharedEntry[]>("list_shared_command");
setSharedList(list ?? []);

// Fetch with parameters
const list = await invoke<SharedEntry[]>("fetch_remote_list_command", {
  targetIp: activeDevice.ip,
  targetPort: activeDevice.port,
});
```

### Event-Based Updates

Use Tauri event listeners for real-time updates:

```typescript
useEffect(() => {
  const unlistenPromise = listen<DeviceInfo[]>("device-list-updated", (event) => {
    setDevices(event.payload ?? []);
  });

  return () => {
    unlistenPromise.then((unlisten) => unlisten());
  };
}, []);
```

### Polling Pattern

For periodic refreshes:

```typescript
useEffect(() => {
  if (!activeDevice) return;
  
  const intervalId = setInterval(async () => {
    if (pullingId) return;
    try {
      const list = await invoke<SharedEntry[]>("fetch_remote_list_command", {
        targetIp: activeDevice.ip,
        targetPort: activeDevice.port,
      });
      setRemoteList(list ?? []);
    } catch { /* device may be offline */ }
  }, settings.discoveryIntervalMs);
  
  return () => clearInterval(intervalId);
}, [activeDevice, settings.discoveryIntervalMs, pullingId]);
```

---

## Persistence

### Settings with Tauri Store

Use `@tauri-apps/plugin-store` for persistent settings:

```typescript
const SETTINGS_STORE_PATH = "settings.json";

const DEFAULT_SETTINGS: Settings = {
  downloadDir: "",
  maxConcurrent: 2,
  maxMbps: 0,
  discoveryIntervalMs: 5000,
  sameSubnetOnly: false,
};

// Load settings
useEffect(() => {
  const loadSettings = async () => {
    try {
      const store = await Store.load(SETTINGS_STORE_PATH);
      const saved = await store.get<Settings>("settings");
      if (saved) {
        const merged = { ...DEFAULT_SETTINGS, ...saved };
        setSettings(merged);
        setDownloadDir(merged.downloadDir || "");
      }
    } catch (error) {
      console.error("Failed to load settings", error);
    } finally {
      setSettingsLoading(false);
    }
  };
  loadSettings();
}, []);

// Save settings
const saveSettings = async (next: Settings) => {
  const store = await Store.load(SETTINGS_STORE_PATH);
  await store.set("settings", next);
  await store.save();
  setSettings(next);
};
```

---

## Common Mistakes

1. **Don't use state for everything** - Use refs for values that don't need re-renders
2. **Don't duplicate derived state** - Compute values during render
3. **Don't forget to initialize** - Always provide initial state values
4. **Don't mutate state directly** - Always use setter functions
5. **Don't forget cleanup** - Clean up event listeners and timers
