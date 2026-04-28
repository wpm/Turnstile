-- Term-mode proof: conjunction is associative.
-- Showcases: nested anonymous constructor ⟨⟨·, ·⟩, ·⟩ and field projections.
theorem my_and_assoc {p q r : Prop} (h : p ∧ q ∧ r) : (p ∧ q) ∧ r :=
  ⟨⟨h.1, h.2.1⟩, h.2.2⟩
