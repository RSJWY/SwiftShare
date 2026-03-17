# Hook Guidelines

> How hooks are used in this project.

---

## Overview

SwiftShare uses React's built-in hooks extensively. Currently, there are **no custom hooks** - all logic is inline in `App.tsx`. The project uses standard React hooks: `useState`, `useEffect`, and `useRef`.

---

## Built-in Hooks Usage

### useState

Used for reactive UI state:

```typescript
const [devices, setDevices] = useState<DeviceInfo[]>([]);
const [activeDevice, setActiveDevice] = useState<DeviceInfo | null>(null);
const [progress, setProgress] = useState(0);
const [settingsOpen, setSettingsOpen] = useState(false);
```

### useEffect

Used for side effects - data fetching, event listeners, timers:

```typescript
// Tauri event listener
useEffect(() => {
  const unlistenPromise = listen<DeviceInfo[]>("device-list-updated", (event) => {
    setDevices(event.payload ?? []);
  });

  return () => {
    unlistenPromise.then((unlisten) => unlisten());
  };
}, []);

// Periodic refresh
useEffect(() => {
  const intervalId = setInterval(async () => {
    // refresh logic
  }, settings.discoveryIntervalMs);
  return () => clearInterval(intervalId);
}, [activeDevice, settings.discoveryIntervalMs]);
```

### useRef

Used for mutable values that don't trigger re-renders:

```typescript
// Speed calculation without re-renders
const speedRef = useRef<{ lastBytes: number; lastTime: number; speed: number }>({
  lastBytes: 0,
  lastTime: 0,
  speed: 0
});

// Prevent double-actions
const dropInProgressRef = useRef(false);

// Cache for directory file trees
const dirFilesRef = useRef<Map<string, DirFileInfo[]>>(new Map());
```

---

## Custom Hook Patterns

Currently no custom hooks exist, but here's the pattern to follow:

### When to Create Custom Hooks

Extract logic into custom hooks when:
- Same logic is used in multiple places
- A component becomes too large (> 300 lines)
- Logic can be reused across components

### Custom Hook Example

```typescript
// src/hooks/useDevices.ts
import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

type DeviceInfo = {
  machine_id: string;
  name: string;
  ip: string;
  port: number;
};

export function useDevices() {
  const [devices, setDevices] = useState<DeviceInfo[]>([]);

  useEffect(() => {
    const unlistenPromise = listen<DeviceInfo[]>("device-list-updated", (event) => {
      setDevices(event.payload ?? []);
    });

    return () => {
      unlistenPromise.then((unlisten) => unlisten());
    };
  }, []);

  return { devices };
}
```

---

## Data Fetching

### Tauri Command Invocation

Use `invoke()` from `@tauri-apps/api/core`:

```typescript
// Simple invocation
const machineId = await invoke<string>("get_local_machine_id_command");

// With parameters
await invoke("add_shared_command", { paths });
const list = await invoke<SharedEntry[]>("list_shared_command");

// Error handling
try {
  const result = await invoke<SharedEntry[]>("fetch_remote_list_command", {
    targetIp: activeDevice.ip,
    targetPort: activeDevice.port,
  });
  setRemoteList(result ?? []);
} catch (error) {
  console.error("Failed to fetch list:", error);
  setStatusMessage(`获取失败: ${String(error)}`);
}
```

### Polling Pattern

For periodic refreshes:

```typescript
useEffect(() => {
  if (!activeDevice) return;
  
  const intervalId = setInterval(async () => {
    if (pullingId) return; // Skip during transfer
    try {
      const list = await invoke<SharedEntry[]>("fetch_remote_list_command", {
        targetIp: activeDevice.ip,
        targetPort: activeDevice.port,
      });
      setRemoteList(list ?? []);
    } catch {
      // Device may have gone offline
    }
  }, settings.discoveryIntervalMs);
  
  return () => clearInterval(intervalId);
}, [activeDevice, settings.discoveryIntervalMs, pullingId]);
```

---

## Naming Conventions

| Pattern | Convention | Example |
|---------|------------|---------|
| Custom hooks | Prefix `use` | `useDevices`, `useTransfer` |
| State variables | Descriptive, camelCase | `devices`, `activeDevice` |
| Ref variables | Descriptive, camelCase | `speedRef`, `dirFilesRef` |
| Event handlers | `handle` prefix | `handleClick`, `handleSelect` |

---

## Common Mistakes

1. **Missing dependency array** - Always include all dependencies in useEffect
2. **Not cleaning up listeners** - Always return cleanup function
3. **Using state when ref is better** - Use useRef for values that shouldn't trigger re-renders
4. **Not handling loading states** - Add loading indicators for async operations
5. **Not handling errors** - Always wrap async calls in try/catch
6. **Stale closures** - Be careful with closures in useEffect callbacks

---

## Best Practices

1. **Clean up listeners** - Always unsubscribe from Tauri events
2. **Handle errors** - Show user-friendly error messages
3. **Type your state** - Always define types for useState
4. **Use refs for mutable values** - Don't trigger re-renders for internal state
5. **Extract when needed** - Create custom hooks for reusable logic

---

## Future Considerations

As the app grows, consider extracting:

1. `useDeviceDiscovery` - Device list management
2. `useFileTransfer` - Transfer state and progress
3. `useSettings` - Settings persistence
4. `useEventListener` - Generic Tauri event handling
