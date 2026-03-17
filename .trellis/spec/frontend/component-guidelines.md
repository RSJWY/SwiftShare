# Component Guidelines

> How components are built in this project.

---

## Overview

SwiftShare uses React 19 with a **single-file component approach**. All UI is currently in `App.tsx`. Components are defined inline or as local functions within the same file. This approach is suitable for the current small codebase.

---

## Current Component Structure

### Single File Pattern

All components and UI live in `src/App.tsx`:

```typescript
// src/App.tsx
import { useState, useEffect, useRef } from "react";

type DeviceInfo = {
  machine_id: string;
  name: string;
  ip: string;
  port: number;
};

// Local utility functions
function formatFileSize(bytes: number): string {
  // ...
}

function App() {
  // All state
  const [devices, setDevices] = useState<DeviceInfo[]>([]);
  
  // All UI
  return (
    <main>
      {/* JSX */}
    </main>
  );
}
```

---

## Props Conventions

### Inline Type Definitions

Types are defined inline at the top of the file or near their usage:

```typescript
// Define types at file top
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
```

### Props for Extracted Components

If components are extracted, define props interfaces:

```typescript
interface DeviceListProps {
  devices: DeviceInfo[];
  onSelect: (device: DeviceInfo) => void;
  activeDevice: DeviceInfo | null;
}

function DeviceList({ devices, onSelect, activeDevice }: DeviceListProps) {
  return (
    <div>
      {devices.map(device => (
        <button key={device.machine_id} onClick={() => onSelect(device)}>
          {device.name}
        </button>
      ))}
    </div>
  );
}
```

---

## Styling Patterns

### Tailwind CSS

The project uses **Tailwind CSS v4** with **Tailwind Vite plugin**:

```typescript
// Tailwind classes for styling
<div className="flex items-center justify-between">
<button className="subtle-button text-xs">
<span className="text-sm font-semibold text-white">
```

### Custom CSS (Glassmorphism)

Custom styles in `App.css` for glassmorphism effects:

```css
/* src/App.css */
.glass-panel {
  @apply rounded-2xl border border-white/10 bg-white/5 backdrop-blur-xl;
}

.subtle-button {
  @apply rounded-lg border border-white/10 bg-white/5 px-3 py-1.5 
         text-xs text-white/70 transition hover:bg-white/10;
}
```

### Framer Motion

Animations use **Framer Motion**:

```typescript
import { motion } from "framer-motion";

<motion.div
  initial={{ opacity: 0 }}
  animate={{ opacity: 1 }}
  className="..."
>
```

---

## Event Handling Patterns

### Tauri Event Listeners

Use `listen()` from `@tauri-apps/api/event`:

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

### Drag and Drop

Use `@crabnebula/tauri-plugin-drag`:

```typescript
import { startDrag } from "@crabnebula/tauri-plugin-drag";

await startDrag({
  item: [path],
  icon: path,
});
```

---

## Accessibility

### Basic A11y Patterns

1. **Semantic HTML**: Use `<main>`, `<header>`, `<section>`, `<button>`
2. **Button elements**: Use `<button>` for interactive elements
3. **Text alternatives**: Status messages for screen readers
4. **Focus states**: Tailwind's focus-visible patterns

```typescript
// Good: Semantic button with clear purpose
<button
  className="subtle-button"
  onClick={handleClick}
  aria-label="选择下载目录"
>
  选择
</button>

// Good: Status announcement
<div className="sr-only" aria-live="polite">
  {statusMessage}
</div>
```

### Current Limitations

- No ARIA labels on all interactive elements
- No keyboard navigation for file lists
- Status messages not announced to screen readers

---

## Common Mistakes

1. **Using div for clickable items** - Use `<button>` instead
2. **Missing cleanup for event listeners** - Always return cleanup function in useEffect
3. **Not handling loading/error states** - Add proper state management
4. **Inline styles** - Use Tailwind classes instead
5. **Not using TypeScript types** - Define proper types for all data structures

---

## Best Practices

1. **Keep components small** - Extract when App.tsx exceeds ~500 lines
2. **Define types upfront** - Types at top of file or in dedicated types file
3. **Use semantic HTML** - Buttons for actions, links for navigation
4. **Clean up listeners** - Always unsubscribe from Tauri events
5. **Handle errors** - Try/catch with user-friendly error messages
