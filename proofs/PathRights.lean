import Std

/- ChoreoFS's fixed I/O capability admission. Other WASI rights, descriptor
   lifetime, guest execution and Hibana progress are separate boundaries. -/
namespace WasiPathRights

def required (kind : Nat) : Nat :=
  if kind = 1 then 2 else if kind = 2 then 64 else if kind = 3 then 16384 else 0

def applicable (kind : Nat) : Nat := if kind = 3 then 16448 else 66

def admitted (kind rights : Nat) : Bool :=
  decide (required kind ≠ 0 ∧ rights &&& applicable kind = required kind)

theorem accepted_has_capability (kind rights : Nat) (h : admitted kind rights = true) :
    required kind ≠ 0 := (of_decide_eq_true h).1

theorem accepted_has_exact_io_mode (kind rights : Nat) (h : admitted kind rights = true) :
    rights &&& applicable kind = required kind := (of_decide_eq_true h).2

theorem mismatched_io_mode_rejected (kind rights : Nat)
    (h : rights &&& applicable kind ≠ required kind) : admitted kind rights = false := by
  simp [admitted, h]

theorem writable_requires_write (rights : Nat) (h : admitted 2 rights = true) :
    rights &&& 66 = 64 := by
  simpa [required, applicable] using accepted_has_exact_io_mode 2 rights h

theorem read_request_cannot_open_writer (rights : Nat) (h : rights &&& 66 = 2) :
    admitted 2 rights = false := by
  simp [admitted, required, applicable, h]

theorem empty_material_rejected (rights : Nat) : admitted 0 rights = false := by
  simp [admitted, required]

#print axioms accepted_has_capability
#print axioms accepted_has_exact_io_mode
#print axioms mismatched_io_mode_rejected
#print axioms writable_requires_write
#print axioms read_request_cannot_open_writer
#print axioms empty_material_rejected
end WasiPathRights
