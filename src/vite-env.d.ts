/// <reference types="vite/client" />
//
// Brings in Vite's ambient module declarations — notably `*?raw`, which
// `src/__tests__/conflicts.test.ts` uses to read App.tsx's own source as a
// string. Without this reference `tsc --noEmit` cannot type that import.
