-- Tactic-mode proof: implication is transitive.
-- Showcases: by block, intro, have with explicit type annotation, exact, apply.
theorem imp_trans {p q r : Prop} (hpq : p → q) (hqr : q → r) : p → r := by
  intro hp
  have hq : q := hpq hp
  exact hqr hq

-- Hypothetical syllogism chained with apply.
theorem imp_chain {p q r s : Prop}
    (h1 : p → q) (h2 : q → r) (h3 : r → s) : p → s := by
  intro hp
  apply h3
  apply h2
  exact h1 hp
