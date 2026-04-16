/**
 * Layout state: resizable panel widths, word wrap, and symbol-outline
 * visibility. Round-trip through `.turn` via the session meta payload.
 *
 * `toggleWordWrap` marks the session dirty (imported from sessionState)
 * — this is the only cross-store dependency.
 */

import { markDirty } from './sessionState.svelte'

export const ASSISTANT_WIDTH_MIN = 10
export const ASSISTANT_WIDTH_MAX = 60
export const GOAL_PANEL_MIN = 20
export const GOAL_PANEL_MAX = 80

let assistantWidthPct = $state(25)
let goalPanelPct = $state(30)
let wordWrap = $state(false)
let outlineOpen = $state(false)

export const layoutState = {
  get assistantWidthPct(): number {
    return assistantWidthPct
  },
  get goalPanelPct(): number {
    return goalPanelPct
  },
  get wordWrap(): boolean {
    return wordWrap
  },
  get outlineOpen(): boolean {
    return outlineOpen
  },
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value))
}

export function setAssistantWidthPct(pct: number): void {
  assistantWidthPct = clamp(pct, ASSISTANT_WIDTH_MIN, ASSISTANT_WIDTH_MAX)
}

export function setGoalPanelPct(pct: number): void {
  goalPanelPct = clamp(pct, GOAL_PANEL_MIN, GOAL_PANEL_MAX)
}

export function setWordWrap(v: boolean): void {
  wordWrap = v
}

export function toggleWordWrap(): void {
  wordWrap = !wordWrap
  markDirty()
}

export function setOutlineOpen(v: boolean): void {
  outlineOpen = v
}
