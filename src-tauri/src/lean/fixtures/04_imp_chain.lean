-- Tactic-mode proof: hypothetical syllogism chained with apply.
-- Showcases: multi-line theorem signature, apply, exact.
theorem imp_chain {p q r s : Prop}
    (h1 : p → q) (h2 : q → r) (h3 : r → s) : p → s := by
  intro hp
  apply h3
  apply h2
  exact h1 hp
