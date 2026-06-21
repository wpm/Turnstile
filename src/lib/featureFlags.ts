// Build-time feature flags.
//
// Goal State (ADR-0004, epic #88) — the All-Goals view (`GoalState.svelte`),
// the cursor-anchored goal cards (`GoalCard.svelte`), and the Formal Proof
// separator rail — is pulled out of 1.0.0 but kept in the tree behind this
// flag, off by default (#119). With the flag off, none of it is reachable: no
// Goal State view, no cursor cards, no rail, and no toggle path to any of them.
//
// Enable it for development by setting the env var at build/dev time:
//
//     VITE_GOAL_STATE=1 pnpm dev          # or pnpm tauri dev / pnpm build
//
// Vite inlines `import.meta.env.VITE_GOAL_STATE` at build time, so when it is
// unset the guarded branches (and the components they reference) are
// dead-code-eliminated from the production bundle rather than merely hidden.
export const GOAL_STATE_ENABLED: boolean =
  import.meta.env.VITE_GOAL_STATE === "1" ||
  import.meta.env.VITE_GOAL_STATE === "true";
