# Directory Structure

> How frontend code is organized in this project.

---

## Overview

SwiftShare is a Tauri v2 desktop application with a minimal frontend codebase. The frontend follows a flat structure with most UI code in a single file. This approach works well for small to medium-sized desktop applications.

---

## Directory Layout

```
SwiftShare/
├── src/                          # Frontend source
│   ├── App.tsx                   # Main application component (ALL UI)
│   ├── main.tsx                  # React entry point
│   ├── updater.ts                # Auto-update functionality
│   ├── App.css                   # Global styles + Tailwind
│   └── vite-env.d.ts             # Vite type declarations
├── src-tauri/                    # Rust backend
│   ├── src/
│   │   ├── lib.rs                # Tauri commands registration
│   │   ├── main.rs               # Binary entry point
│   │   ├── discovery.rs          # mDNS device discovery
│   │   └── transport.rs          # TCP file transfer
│   ├── capabilities/             # Tauri v2 permissions
│   ├── tauri.conf.json           # Tauri configuration
│   └── Cargo.toml                # Rust dependencies
├── public/                       # Static assets
├── package.json                   # Frontend dependencies
├── vite.config.ts               # Vite configuration
└── tsconfig.json                # TypeScript configuration
```

---

## Module Organization

### Current Pattern

The project uses a **single-file component approach**:
- All UI code is in `src/App.tsx`
- No separate component, hook, or utility directories
- This is suitable for small applications (< 2000 lines)

### When to Consider Splitting

If the application grows, consider splitting into:

```
src/
├── components/           # Reusable UI components
│   ├── DeviceList.tsx
│   ├── FileList.tsx
│   ├── TransferProgress.tsx
│   └── SettingsModal.tsx
├── hooks/               # Custom React hooks
│   ├── useDevices.ts
│   ├── useTransfer.ts
│   └── useSettings.ts
├── types/               # Shared type definitions
│   └── index.ts
├── utils/               # Utility functions
│   └── format.ts
└── App.tsx              # Main app (composition layer)
```

---

## Naming Conventions

| Item | Convention | Example |
|------|------------|---------|
| Component files | PascalCase | `DeviceList.tsx` |
| Hook files | camelCase, prefix `use` | `useDevices.ts` |
| Type files | PascalCase or `types.ts` | `DeviceInfo.ts` |
| Utility files | camelCase | `format.ts` |
| CSS files | Match component name | `App.css` |

---

## File Purposes

| File | Purpose |
|------|---------|
| `src/main.tsx` | React app entry, renders App to DOM |
| `src/App.tsx` | Main component, all UI logic, state management |
| `src/updater.ts` | Auto-update check logic using Tauri updater plugin |
| `src/App.css` | Global styles + Tailwind directives + custom glassmorphism |

---

## Examples

### Good: Current Single-File Approach

```typescript
// src/App.tsx - Current pattern works for small apps
function App() {
  const [devices, setDevices] = useState<DeviceInfo[]>([]);
  const [sharedList, setSharedList] = useState<SharedEntry[]>([]);
  // ... all state and UI in one file
}
```

### Good: When to Split

If adding new features, consider:
- New settings pages → `src/components/SettingsModal.tsx`
- Complex device management → `src/hooks/useDevices.ts`
- File operations → `src/utils/file.ts`

---

## Anti-Patterns

1. **Don't create deep directory nesting** - Keep it flat for small apps
2. **Don't over-engineer** - No need for complex folder structures if not needed
3. **Don't mix concerns** - Keep UI, logic, and types reasonably organized

---

## Future Considerations

As the app grows, consider:
1. Extracting reusable components (DeviceList, FileList, etc.)
2. Creating custom hooks for complex state logic
3. Moving type definitions to a dedicated types file
4. Adding utility functions for formatting, validation, etc.
