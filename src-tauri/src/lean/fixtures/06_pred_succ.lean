-- Proof by structural induction: predecessor of successor is identity.
-- Showcases: induction with trivial cases, wildcard pattern _ for unused ih.
theorem my_pred_succ (n : Nat) : Nat.pred (Nat.succ n) = n := by
  induction n with
  | zero      => rfl
  | succ n _  => rfl
