-- Term-mode proof: conjunction is commutative.
-- Showcases: anonymous constructor ⟨·, ·⟩, field projections (.1 / .2),
-- implicit-argument braces {}, and Prop-level reasoning without tactics.
theorem my_and_comm {p q : Prop} (h : p ∧ q) : q ∧ p :=
  ⟨h.2, h.1⟩

-- Nested anonymous constructor for associativity.
theorem my_and_assoc {p q r : Prop} (h : p ∧ q ∧ r) : (p ∧ q) ∧ r :=
  ⟨⟨h.1, h.2.1⟩, h.2.2⟩
