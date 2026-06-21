// The right-column views. Defaults to "assistant" (the Proof Assistant); the
// header toggle flips it to "prose" (the Prose Proof). The Goal State view is
// pulled from 1.0.0 and lives behind GOAL_STATE_ENABLED (#119), so it is
// intentionally not part of this shipped type.
export type ProofView = "assistant" | "prose";
