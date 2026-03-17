# Type Safety

> Type safety patterns in this project.

---

## Overview

SwiftShare uses **TypeScript 5.8** for type safety. The project has strong typing for data structures from the Rust backend. Types are defined inline in `App.tsx` at the top of the file, near their usage.

---

## Type Organization

### Inline Type Definitions

Types are defined at the top of `App.tsx`:

```typescript
// src/App.tsx - Top of file

// Device information from mDNS discovery
type DeviceInfo = {
  machine_id: string;
  name: string;
  ip: string;
  port: number;
};

// Shared file/directory entry
type SharedEntry = {
  id: string;
  name: string;
  path: string;
  size: number;
  modified: number;
  relative_path: string;
  is_dir: boolean;
};

// Transfer progress
type PullProgress = {
  entry_id: string;
  name: string;
  received_bytes: number;
  total_bytes: number;
  entry_received_bytes: number;
  entry_total_bytes: number;
};

// Directory file info
type DirFileInfo = {
  path: string;
  size: number;
};

// Conflict detection
type ConflictInfo = {
  has_conflict: boolean;
  conflicting_files: string[];
  total_conflict_size: number;
};

// Application settings
type Settings = {
  downloadDir: string;
  maxConcurrent: number;
  maxMbps: number;
  discoveryIntervalMs: number;
  sameSubnetOnly: boolean;
};
```

### Type for useState

Always provide type parameters for complex state:

```typescript
// Good: Explicit type
const [devices, setDevices] = useState<DeviceInfo[]>([]);
const [settings, setSettings] = useState<Settings>(DEFAULT_SETTINGS);

// Good: Simple types can infer
const [count, setCount] = useState(0); // infers number
const [name, setName] = useState(""); // infers string
```

### Type for useRef

```typescript
const speedRef = useRef<{ lastBytes: number; lastTime: number; speed: number }>({
  lastBytes: 0,
  lastTime: 0,
  speed: 0
});

const dirFilesRef = useRef<Map<string, DirFileInfo[]>>(new Map());
```

---

## Tauri Command Types

### Generic Type Parameters

Use `<Type>` to specify return types:

```typescript
// String command
const machineId = await invoke<string>("get_local_machine_id_command");

// Array command
const list = await invoke<SharedEntry[]>("list_shared_command");

// Object command
const conflict = await invoke<ConflictInfo>("check_pull_conflict_command", {
  entryName: node.entry.name,
  entryIsDir: node.entry.is_dir,
  entryId: node.entry.id,
  targetIp: activeDevice.ip,
  targetPort: activeDevice.port,
  destDir: dir,
});
```

### Event Listener Types

Use generic for event payload types:

```typescript
// Device list update event
const unlistenPromise = listen<DeviceInfo[]>("device-list-updated", (event) => {
  setDevices(event.payload ?? []);
});

// Transfer progress event
const unlistenPromise = listen<PullProgress>("pull-progress", (event) => {
  setPullProgress(event.payload ?? null);
});
```

---

## Validation

### Runtime Validation

No validation library is currently used. Simple null checks are sufficient:

```typescript
// Null coalescing for optional values
setDevices(event.payload ?? []);
setRemoteList(list ?? []);

// Type guards for complex checks
if (conflict.has_conflict) {
  setConflictInfo(conflict);
}
```

### Settings Validation

Merge with defaults to ensure all fields exist:

```typescript
const DEFAULT_SETTINGS: Settings = {
  downloadDir: "",
  maxConcurrent: 2,
  maxMbps: 0,
  discoveryIntervalMs: 5000,
  sameSubnetOnly: false,
};

// Merge saved settings with defaults
const merged = { ...DEFAULT_SETTINGS, ...saved };
```

---

## Common Patterns

### Optional Properties

Use `?` for optional properties:

```typescript
type PullProgress = {
  entry_id: string;
  name: string;
  received_bytes: number;
  total_bytes: number;
  entry_received_bytes?: number; // Optional
  entry_total_bytes?: number;   // Optional
};
```

### Type Aliases

Create aliases for reusable types:

```typescript
type StatusType = "success" | "error" | "info";

const [statusType, setStatusType] = useState<StatusType>("info");
```

### Function Types

```typescript
// Callback type
type ConflictCallback = () => void;

const conflictCallbackRef = useRef<(() => void) | null>(null);
```

---

## Forbidden Patterns

1. **`any` type** - Never use `any`, use proper types or `unknown`
2. **`@ts-ignore`** - Avoid suppressing TypeScript errors
3. **Type assertions without checking** - Don't use `as` without validation
4. **Implicit `any`** - Enable `noImplicitAny` in tsconfig

---

## Best Practices

1. **Always type useState** - Especially for arrays and objects
2. **Use Tauri invoke generics** - Specify return types: `invoke<Type>()`
3. **Define types upfront** - At top of file or in dedicated types file
4. **Handle nulls explicitly** - Use `??` or `?.` operators
5. **Enable strict mode** - In tsconfig.json

---

## Configuration

The project uses `tsconfig.json` with standard settings:

```json
{
  "compilerOptions": {
    "target": "ES2020",
    "useDefineForClassFields": true,
    "lib": ["ES2020", "DOM", "DOM.Iterable"],
    "module": "ESNext",
    "skipLibCheck": true,
    "moduleResolution": "bundler",
    "allowImportingTsExtensions": true,
    "resolveJsonModule": true,
    "isolatedModules": true,
    "noEmit": true,
    "jsx": "react-jsx",
    "strict": true,
    "noUnusedLocals": true,
    "noUnusedParameters": true,
    "noFallthroughCasesInSwitch": true
  }
}
```
