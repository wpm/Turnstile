export interface GoalBlock {
  caseLabel: string | null;
  hypotheses: string[];
  conclusion: string;
}

/**
 * True when the goal text means the proof is finished (no remaining goals).
 * Lean's `$/lean/plainGoal` renders this as the bare string "no goals" — with
 * no `⊢` turnstile — so it must not be parsed as a goal whose conclusion is
 * the words "no goals" (that would render a meaningless `⊢ no goals`). The
 * check is case-insensitive to be robust to casing differences.
 */
export function isProofComplete(raw: string): boolean {
  return raw.trim().toLowerCase() === "no goals";
}

/**
 * The hypotheses in `current` that are not already in `previous`, preserving
 * order. Used by the compact (card) rendering to show only what a row *added*
 * to the context — the hypothesis delta — since goals tend to be short and the
 * unchanged stack is noise at the cursor. With no `previous`, every hypothesis
 * is "new", so the delta is the whole list.
 */
export function hypothesisDelta(
  current: string[],
  previous: string[] = [],
): string[] {
  const prior = new Set(previous);
  return current.filter((h) => !prior.has(h));
}

export function parseGoalText(raw: string): GoalBlock[] {
  if (!raw.trim()) return [];

  return raw.split(/\n\s*\n/).map((block) => {
    const lines = block.split("\n").filter((l, i) => i > 0 || l.trim() !== "");
    let caseLabel: string | null = null;

    if (lines.length > 1 && lines[0].startsWith("case ")) {
      caseLabel = lines[0].trim();
      lines.shift();
    }

    const goalIdx = lines.findIndex((l) => l.trimStart().startsWith("⊢"));

    if (goalIdx === -1) {
      return { caseLabel, hypotheses: [], conclusion: block.trim() };
    }

    const hypotheses = lines.slice(0, goalIdx).filter(Boolean);
    const conclusion = lines.slice(goalIdx).join("\n").replace(/^⊢\s*/, "");
    return { caseLabel, hypotheses, conclusion };
  });
}
