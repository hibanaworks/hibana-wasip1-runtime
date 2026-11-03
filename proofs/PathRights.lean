import Std

/- ChoreoFS's fixed I/O capability admission. Other WASI rights, descriptor
   lifetime, guest execution and Hibana progress are separate boundaries. -/
namespace WasiPathRights

def required (kind : Nat) : Nat :=
  if kind = 1 then 2 else if kind = 2 then 64 else if kind = 3 then 16384 else if kind = 4 then 66 else 0

def applicable (kind : Nat) : Nat := if kind = 3 then 16448 else 66

def selected (kind rights : Nat) : Nat :=
  if kind = 4 then rights &&& applicable kind else required kind

def admitted (kind rights : Nat) : Bool :=
  if kind = 4 then decide (rights &&& 66 ≠ 0)
  else decide (required kind ≠ 0 ∧ rights &&& applicable kind = required kind)

theorem accepted_has_capability (kind rights : Nat) (h : admitted kind rights = true) :
    required kind ≠ 0 := by
  by_cases hk : kind = 4
  · simp [required, hk]
  · have both : required kind ≠ 0 ∧ rights &&& applicable kind = required kind :=
      of_decide_eq_true (by simpa [admitted, hk] using h)
    exact both.1

theorem accepted_has_exact_io_mode (kind rights : Nat) (hk : kind ≠ 4) (h : admitted kind rights = true) :
    rights &&& applicable kind = required kind := by
  have both : required kind ≠ 0 ∧ rights &&& applicable kind = required kind :=
    of_decide_eq_true (by simpa [admitted, hk] using h)
  exact both.2

theorem mismatched_io_mode_rejected (kind rights : Nat) (hk : kind ≠ 4)
    (h : rights &&& applicable kind ≠ required kind) : admitted kind rights = false := by
  simp [admitted, hk, h]

theorem writable_requires_write (rights : Nat) (h : admitted 2 rights = true) :
    rights &&& 66 = 64 := by
  simpa [required, applicable] using accepted_has_exact_io_mode 2 rights (by decide) h

theorem read_request_cannot_open_writer (rights : Nat) (h : rights &&& 66 = 2) :
    admitted 2 rights = false := by
  simp [admitted, required, applicable, h]

theorem empty_material_rejected (rights : Nat) : admitted 0 rights = false := by
  simp [admitted, required]

theorem read_write_requires_requested_io (rights : Nat) (h : admitted 4 rights = true) :
    rights &&& 66 ≠ 0 := by simpa [admitted] using h

theorem read_write_accepts_read (rights : Nat) (h : rights &&& 66 = 2) :
    admitted 4 rights = true := by simp [admitted, h]

theorem read_write_accepts_write (rights : Nat) (h : rights &&& 66 = 64) :
    admitted 4 rights = true := by simp [admitted, h]

theorem read_write_accepts_both (rights : Nat) (h : rights &&& 66 = 66) :
    admitted 4 rights = true := by simp [admitted, h]

theorem accepted_has_exact_selected_mode (kind rights : Nat) (h : admitted kind rights = true) :
    selected kind rights = rights &&& applicable kind := by
  by_cases hk : kind = 4
  · simp [selected, hk]
  · simpa [selected, hk] using (accepted_has_exact_io_mode kind rights hk h).symm

theorem accepted_has_nonempty_selected_mode (kind rights : Nat) (h : admitted kind rights = true) :
    selected kind rights ≠ 0 := by
  by_cases hk : kind = 4
  · subst kind
    simpa [selected, applicable] using read_write_requires_requested_io rights h
  · simpa [selected, hk] using accepted_has_capability kind rights h

theorem accepted_cannot_add_io_rights (kind rights : Nat) (h : admitted kind rights = true) :
    selected kind rights &&& rights = selected kind rights := by
  calc
    selected kind rights &&& rights = (rights &&& applicable kind) &&& rights := by
      rw [accepted_has_exact_selected_mode kind rights h]
    _ = rights &&& (applicable kind &&& rights) := Nat.and_assoc _ _ _
    _ = rights &&& (rights &&& applicable kind) := by rw [Nat.and_comm (applicable kind) rights]
    _ = (rights &&& rights) &&& applicable kind := (Nat.and_assoc _ _ _).symm
    _ = rights &&& applicable kind := by rw [Nat.and_self]
    _ = selected kind rights := (accepted_has_exact_selected_mode kind rights h).symm

theorem matching_write_is_admitted (rights : Nat) (h : rights &&& 66 = 64) :
    admitted 2 rights = true := by simp [admitted, required, applicable, h]

#print axioms accepted_has_capability
#print axioms accepted_has_exact_io_mode
#print axioms mismatched_io_mode_rejected
#print axioms writable_requires_write
#print axioms read_request_cannot_open_writer
#print axioms empty_material_rejected
#print axioms read_write_requires_requested_io
#print axioms read_write_accepts_read
#print axioms read_write_accepts_write
#print axioms read_write_accepts_both
#print axioms accepted_has_exact_selected_mode
#print axioms accepted_has_nonempty_selected_mode
#print axioms accepted_cannot_add_io_rights
#print axioms matching_write_is_admitted
end WasiPathRights
