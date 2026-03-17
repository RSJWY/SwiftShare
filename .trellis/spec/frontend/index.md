# Frontend Development Guidelines

> Best practices for frontend development in this project.

---

## Overview

This directory contains guidelines for frontend development in SwiftShare - a Tauri v2 desktop application with React 19 + TypeScript frontend.

---

## Guidelines Index

| Guide | Description | Status |
|-------|-------------|--------|
| [Directory Structure](./directory-structure.md) | Module organization and file layout | **Filled** |
| [Component Guidelines](./component-guidelines.md) | Component patterns, props, composition | **Filled** |
| [Hook Guidelines](./hook-guidelines.md) | Custom hooks, data fetching patterns | **Filled** |
| [State Management](./state-management.md) | Local state, global state, server state | **Filled** |
| [Quality Guidelines](./quality-guidelines.md) | Code standards, forbidden patterns | **Filled** |
| [Type Safety](./type-safety.md) | Type patterns, validation | **Filled** |

---

## Quick Reference

### Tech Stack
- **Framework**: Tauri v2 (desktop)
- **Frontend**: React 19 + TypeScript 5.8
- **Styling**: Tailwind CSS v4 + Framer Motion
- **Build**: Vite 7

### Key Patterns
- Single-file component approach (`App.tsx`)
- Local state with useState, useEffect, useRef
- Tauri commands via `invoke()` from `@tauri-apps/api/core`
- Tauri events via `listen()` from `@tauri-apps/api/event`

---

## Language

All documentation is written in **English**.
