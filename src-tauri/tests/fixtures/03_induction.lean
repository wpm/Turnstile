-- Proof by structural induction on ℕ.
-- Showcases: induction ... with, named case branches | zero and | succ,
-- rfl for definitional equality, congrArg to lift equalities through a function.

-- 0 + n = n  (not definitional on the left; requires induction).
theorem my_zero_add (n : Nat) : 0 + n = n := by
  induction n with
  | zero      => rfl
  | succ n ih => exact congrArg Nat.succ ih

-- The predecessor of a successor is the original number.
theorem my_pred_succ (n : Nat) : Nat.pred (Nat.succ n) = n := by
  induction n with
  | zero      => rfl
  | succ n _  => rfl
