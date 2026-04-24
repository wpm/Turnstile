-- Function defined by pattern-matching equations; theorem proved with cases + <;>.
-- Showcases: def with | equation clauses, Bool, cases tactic,
-- the <;> tactic combinator (apply one tactic to all remaining goals), rfl.

def myNot : Bool → Bool
  | true  => false
  | false => true

-- Double negation is identity; cases splits Bool, <;> rfl closes both at once.
theorem myNot_myNot (b : Bool) : myNot (myNot b) = b := by
  cases b <;> rfl

-- myNot agrees with the built-in Bool.not on both constructors.
theorem myNot_eq_not (b : Bool) : myNot b = !b := by
  cases b <;> rfl
