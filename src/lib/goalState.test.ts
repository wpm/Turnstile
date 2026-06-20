import { describe, it, expect } from "vitest";
import { hypothesisDelta, isProofComplete, parseGoalText } from "./goalState";

describe("parseGoalText", () => {
  it("returns empty array for empty string", () => {
    expect(parseGoalText("")).toEqual([]);
  });

  it("returns empty array for whitespace-only input", () => {
    expect(parseGoalText("   \n  ")).toEqual([]);
  });

  it("parses a single goal with no case label and no hypotheses", () => {
    const [goal] = parseGoalText("⊢ True");
    expect(goal.caseLabel).toBeNull();
    expect(goal.hypotheses).toEqual([]);
    expect(goal.conclusion).toBe("True");
  });

  it("strips leading ⊢ and optional whitespace from conclusion", () => {
    const [goal] = parseGoalText("⊢  P ∧ Q");
    expect(goal.conclusion).toBe("P ∧ Q");
  });

  it("parses hypotheses above the ⊢ line", () => {
    const input = "h : P\nh2 : Q\n⊢ P ∧ Q";
    const [goal] = parseGoalText(input);
    expect(goal.hypotheses).toEqual(["h : P", "h2 : Q"]);
    expect(goal.conclusion).toBe("P ∧ Q");
  });

  it("parses a case label", () => {
    const input = "case inl\nh : P\n⊢ P ∨ Q";
    const [goal] = parseGoalText(input);
    expect(goal.caseLabel).toBe("case inl");
    expect(goal.hypotheses).toEqual(["h : P"]);
    expect(goal.conclusion).toBe("P ∨ Q");
  });

  it("does not treat 'case' as a label when it is the only line", () => {
    const input = "case inl";
    const [goal] = parseGoalText(input);
    expect(goal.caseLabel).toBeNull();
  });

  it("parses two goals separated by a blank line", () => {
    const input = "h : P\n⊢ P\n\nh2 : Q\n⊢ Q";
    const goals = parseGoalText(input);
    expect(goals).toHaveLength(2);
    expect(goals[0].hypotheses).toEqual(["h : P"]);
    expect(goals[0].conclusion).toBe("P");
    expect(goals[1].hypotheses).toEqual(["h2 : Q"]);
    expect(goals[1].conclusion).toBe("Q");
  });

  it("parses two goals each with a case label", () => {
    const input = "case inl\nh : P\n⊢ P ∨ Q\n\ncase inr\nh2 : Q\n⊢ P ∨ Q";
    const goals = parseGoalText(input);
    expect(goals).toHaveLength(2);
    expect(goals[0].caseLabel).toBe("case inl");
    expect(goals[1].caseLabel).toBe("case inr");
  });

  it("handles a multi-line conclusion (indented continuation)", () => {
    const input = "⊢ (fun x =>\n  x + 1) 0 = 1";
    const [goal] = parseGoalText(input);
    expect(goal.conclusion).toBe("(fun x =>\n  x + 1) 0 = 1");
  });

  it("returns conclusion as trimmed block text when no ⊢ is found", () => {
    const input = "no turnstile here";
    const [goal] = parseGoalText(input);
    expect(goal.conclusion).toBe("no turnstile here");
    expect(goal.hypotheses).toEqual([]);
  });

  it("blank line between hypothesis and ⊢ splits into two goal blocks", () => {
    // \n\n is the block separator, so "h : P\n\n⊢ P" is parsed as two blocks,
    // not one block with a blank hypothesis line.
    const input = "h : P\n\n⊢ P";
    expect(parseGoalText(input)).toHaveLength(2);
  });

  it("detects ⊢ even with leading indentation (trimStart)", () => {
    const input = "h : P\n  ⊢ P";
    const [goal] = parseGoalText(input);
    expect(goal.hypotheses).toEqual(["h : P"]);
    expect(goal.conclusion).toBe("  ⊢ P");
  });
});

describe("hypothesisDelta", () => {
  it("returns the whole list when there is no previous context", () => {
    expect(hypothesisDelta(["h : P", "h2 : Q"])).toEqual(["h : P", "h2 : Q"]);
  });

  it("keeps only hypotheses absent from the previous context, in order", () => {
    expect(hypothesisDelta(["h : P", "h2 : Q", "h3 : R"], ["h : P"])).toEqual([
      "h2 : Q",
      "h3 : R",
    ]);
  });

  it("is empty when the row added no hypotheses", () => {
    expect(hypothesisDelta(["h : P"], ["h : P", "h2 : Q"])).toEqual([]);
  });

  it("matches hypotheses by exact string", () => {
    // A renamed/retyped hypothesis is a different string, so it counts as new.
    expect(hypothesisDelta(["h : P ∧ Q"], ["h : P"])).toEqual(["h : P ∧ Q"]);
  });
});

describe("isProofComplete", () => {
  it("is true for Lean's bare 'no goals'", () => {
    expect(isProofComplete("no goals")).toBe(true);
  });

  it("is true regardless of casing or surrounding whitespace", () => {
    expect(isProofComplete("No goals")).toBe(true);
    expect(isProofComplete("  no goals\n")).toBe(true);
  });

  it("is false for an empty or whitespace-only goal state", () => {
    expect(isProofComplete("")).toBe(false);
    expect(isProofComplete("   ")).toBe(false);
  });

  it("is false for a real goal, even one mentioning 'no goals'", () => {
    expect(isProofComplete("⊢ True")).toBe(false);
    expect(isProofComplete("h : p\n⊢ no goals here")).toBe(false);
  });
});
