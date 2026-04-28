-- Double negation is identity; cases splits Bool, <;> rfl closes both at once.
-- Showcases: def with | equation clauses, Bool, cases tactic, <;> combinator.
def myNot : Bool → Bool
  | true  => false
  | false => true

theorem myNot_myNot (b : Bool) : myNot (myNot b) = b := by
  cases b <;> rfl
