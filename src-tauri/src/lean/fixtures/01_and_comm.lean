-- Term-mode proof: conjunction is commutative.
-- Showcases: anonymous constructor ⟨·, ·⟩, field projections (.1 / .2),
-- implicit-argument braces {}, and Prop-level reasoning without tactics.
theorem my_and_comm {p q : Prop} (h : p ∧ q) : q ∧ p :=
  ⟨h.2, h.1⟩
