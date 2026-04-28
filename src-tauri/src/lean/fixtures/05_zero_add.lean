-- Proof by structural induction on ℕ.
-- Showcases: induction ... with, named case branches | zero and | succ,
-- rfl for definitional equality, congrArg to lift equalities through a function.
theorem my_zero_add (n : Nat) : 0 + n = n := by
  induction n with
  | zero      => rfl
  | succ n ih => exact congrArg Nat.succ ih
