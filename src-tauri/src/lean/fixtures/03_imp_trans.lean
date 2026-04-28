-- Tactic-mode proof: implication is transitive.
-- Showcases: by block, intro, have with explicit type annotation, exact, apply.
theorem imp_trans {p q r : Prop} (hpq : p → q) (hqr : q → r) : p → r := by
  intro hp
  have hq : q := hpq hp
  exact hqr hq
