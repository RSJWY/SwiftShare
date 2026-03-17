# Quality Guidelines

> Code quality standards for frontend development.

---

## Overview

SwiftShare follows standard React and TypeScript best practices. The project uses:
- **TypeScript** with strict mode for type safety
- **Tailwind CSS** for styling
- **Framer Motion** for animations
- **Tauri v2** for desktop integration

There are currently **no automated tests** (unit or e2e). Quality is maintained through code review and manual testing.

---

## Forbidden Patterns

### 1. Using `any` Type

```typescript
// BAD
const data: any = getData();

// GOOD
const data: DeviceInfo = getData();
```

### 2. Using `var` Keyword

```typescript
// BAD
var count = 0;

// GOOD
const count = 0; // or let if mutable
```

### 3. Not Cleaning Up Effects

```typescript
// BAD
useEffect(() => {
  const listener = listen(...);
  // Missing cleanup!
});

// GOOD
useEffect(() => {
  const unlistenPromise = listen(...);
  return () => {
    unlistenPromise.then(unlisten => unlisten());
  };
}, []);
```

### 4. Inline Styles

```typescript
// BAD
<div style={{ color: 'red', fontSize: '14px' }}>

// GOOD
<div className="text-red-500 text-sm">
```

### 5. Not Handling Errors

```typescript
// BAD
await invoke("some_command", { data });

// GOOD
try {
  await invoke("some_command", { data });
} catch (error) {
  console.error("Operation failed:", error);
  setStatusMessage(`操作失败: ${String(error)}`);
}
```

### 6. Creating Variables Without Types

```typescript
// BAD - implicit any
const [data, setData] = useState(getInitialData());

// GOOD
const [data, setData] = useState<SharedEntry[]>([]);
```

---

## Required Patterns

### 1. Always Type useState

```typescript
// Required for complex types
const [devices, setDevices] = useState<DeviceInfo[]>([]);
const [settings, setSettings] = useState<Settings>(DEFAULT_SETTINGS);
```

### 2. Handle Null Values

```typescript
// Use nullish coalescing
setDevices(event.payload ?? []);

// Use optional chaining
const name = device?.name ?? "Unknown";
```

### 3. Use Semantic HTML

```typescript
// Button for actions
<button onClick={handleClick}>Save</button>

// Not div
<div onClick={handleClick}>Save</div>
```

### 4. Clean Up Event Listeners

```typescript
// Always return cleanup
useEffect(() => {
  const unlistenPromise = listen<T>("event", (e) => { ... });
  return () => {
    unlistenPromise.then(unlisten => unlisten());
  };
}, [dependencies]);
```

### 5. Type Tauri Invocations

```typescript
// Always specify return type
const result = await invoke<ReturnType>("command_name", { params });
```

---

## Linting

The project uses TypeScript's built-in strict mode. Run checks:

```bash
# TypeScript type checking
npx tsc --noEmit

# Or via Vite
pnpm build
```

### Recommended VS Code Settings

```json
{
  "typescript.tsdk": "node_modules/typescript/lib",
  "editor.codeActionsOnSave": {
    "source.fixAll": "explicit"
  }
}
```

---

## Testing Requirements

### Current State

**No automated tests** exist in this project. Testing is done manually:
1. Manual feature testing during development
2. Build verification with `pnpm tauri build`

### Recommended for Future

If tests are added, consider:
- **Unit tests**: Utility functions, helpers
- **E2E tests**: Critical user flows (file transfer, device discovery)

---

## Code Review Checklist

### General
- [ ] Code compiles without errors
- [ ] No TypeScript errors (run `tsc`)
- [ ] No console errors in browser
- [ ] Code follows existing patterns

### React/TypeScript
- [ ] Types are defined for all state
- [ ] useEffect has proper cleanup
- [ ] Error handling is present for async calls
- [ ] No `any` types used

### UI/UX
- [ ] Buttons are semantic `<button>` elements
- [ ] Loading states are handled
- [ ] Error messages are user-friendly
- [ ] Responsive behavior works correctly

### Tauri Integration
- [ ] Event listeners are cleaned up
- [ ] Commands have error handling
- [ ] Plugin APIs are used correctly

---

## Build Verification

Before considering code complete:

```bash
# Type check
pnpm build

# This runs: tsc && vite build
```

If build succeeds, the code is ready for review.
