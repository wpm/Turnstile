-- myNot agrees with the built-in Bool.not on both constructors.
-- Showcases: def with | equation clauses, cases tactic, <;> combinator.
def myNot : Bool → Bool
  | true  => false
  | false => true

theorem myNot_eq_not (b : Bool) : myNot b = !b := by
  cases b <;> rfl
