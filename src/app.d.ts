/// <reference types="vite/client" />

// See https://svelte.dev/docs/kit/types#app.d.ts
declare global {
  namespace App {
    // interface Error {}
    // interface Locals {}
    // interface PageData {}
    // interface PageState {}
    // interface Platform {}
  }

  // Build-time feature flags exposed through Vite's env. Declaring the key here
  // (rather than reading it off the `[key: string]: any` index signature) keeps
  // it typed as `string | undefined` instead of `any`, so strict type-checked
  // lint stays happy. See `src/lib/featureFlags.ts`.
  interface ImportMetaEnv {
    /**
     * Goal State feature flag (#119). When unset/`"0"` the Goal State view, the
     * cursor-anchored goal cards, and the Formal Proof separator rail are all
     * absent (the 1.0.0 default). `"1"` or `"true"` restores them for dev.
     */
    readonly VITE_GOAL_STATE?: string;
  }

  interface ImportMeta {
    readonly env: ImportMetaEnv;
  }
}

export {};
