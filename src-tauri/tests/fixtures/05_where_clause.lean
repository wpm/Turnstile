-- Proof with a local recursive helper via a where-clause.
-- Showcases: where, universe-polymorphic type variable {α : Type},
-- List.map, structural recursion in the helper, congrArg with a section variable.

theorem map_id {α : Type} (xs : List α) : xs.map (fun x => x) = xs :=
  aux xs
where
  aux : ∀ ys : List α, ys.map (fun x => x) = ys
    | []      => rfl
    | y :: ys => congrArg (y :: ·) (aux ys)
