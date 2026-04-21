export interface GoalBlock {
  caseLabel: string | null;
  hypotheses: string[];
  conclusion: string;
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
