import Std

/- The bounded poll admission and prepare/commit model. Byte encoders, Rust
   execution, Endpoint progress, fd freshness, time and driver truth remain
   explicit implementation boundaries; this file does not assume them proved. -/
namespace WasiPoll

abbrev Key := Nat × Nat

-- Greedy ordered occurrence matching: each matched input occurrence is removed.
def orderedMatch : List Key → List Key → Bool
  | _, [] => true
  | [], _ :: _ => false
  | request :: requests, event :: events =>
      if event = request then orderedMatch requests events
      else orderedMatch requests (event :: events)

-- Admission also requires nonempty bounded arrays; readiness alone cannot
-- replace the existing import's ABI and descriptor boundary.
def admitted (requests events : List Key) : Bool :=
  decide (0 < requests.length ∧ requests.length ≤ 16 ∧ 0 < events.length ∧ events.length ≤ 16) &&
    orderedMatch requests events

theorem matching_is_sublist (requests events : List Key)
    (accepted : orderedMatch requests events = true) : events.Sublist requests := by
  induction requests generalizing events with
  | nil =>
      cases events with
      | nil => exact .slnil
      | cons e es => simp [orderedMatch] at accepted
  | cons r rs ih =>
      cases events with
      | nil => exact List.nil_sublist _
      | cons e es =>
          by_cases eq : e = r
          · subst e
            exact (ih es (by simpa [orderedMatch] using accepted)).cons_cons r
          · exact (ih (e :: es) (by simpa [orderedMatch, eq] using accepted)).cons r

theorem matching_bounds_events (requests events : List Key)
    (accepted : orderedMatch requests events = true) : events.length ≤ requests.length :=
  (matching_is_sublist requests events accepted).length_le

theorem matching_never_fabricates (requests events : List Key)
    (accepted : orderedMatch requests events = true) (key : Key) (present : key ∈ events) :
    key ∈ requests :=
  (matching_is_sublist requests events accepted).subset present

theorem matching_preserves_occurrence_count (requests events : List Key)
    (accepted : orderedMatch requests events = true) (key : Key) :
    events.count key ≤ requests.count key :=
  (matching_is_sublist requests events accepted).count_le key

abbrev Byte := Fin 256
abbrev Memory := Nat → Byte

abbrev inside (start size index : Nat) : Prop := start ≤ index ∧ index < start + size

def writeRange (memory : Memory) (start : Nat) (bytes : List Byte) : Memory :=
  fun index => if h : inside start bytes.length index then
    bytes[index - start]'(by unfold inside at h; omega)
  else memory index

theorem write_outside (memory : Memory) (start : Nat) (bytes : List Byte)
    (index : Nat) (outside : ¬ inside start bytes.length index) :
    writeRange memory start bytes index = memory index := by
  simp [writeRange, outside]

theorem write_inside (memory : Memory) (start : Nat) (bytes : List Byte)
    (index : Nat) (h : inside start bytes.length index) :
    writeRange memory start bytes index = bytes[index - start]'(by unfold inside at h; omega) := by
  unfold writeRange
  rw [dif_pos h]

structure Plan where
  input : Nat
  output : Nat
  countAt : Nat
  requests : List Key
  events : List Key
  eventBytes : List Byte
  countBytes : List Byte

abbrev valid (capacity : Nat) (p : Plan) : Prop :=
  0 < p.requests.length ∧ p.requests.length ≤ 16 ∧
  0 < p.events.length ∧ orderedMatch p.requests p.events = true ∧
  p.input + p.requests.length * 48 ≤ capacity ∧
  p.output + p.requests.length * 32 ≤ capacity ∧
  p.countAt + 4 ≤ capacity ∧
  (p.output + p.requests.length * 32 ≤ p.countAt ∨ p.countAt + 4 ≤ p.output) ∧
  p.eventBytes.length = p.events.length * 32 ∧ p.countBytes.length = 4

structure Prepared (capacity : Nat) where
  plan : Plan
  checked : valid capacity plan

def prepare (capacity : Nat) (p : Plan) : Option (Prepared capacity) :=
  if h : valid capacity p then some ⟨p, h⟩ else none

-- The prepared type has no failure step between the two disjoint writes.
def commit {capacity : Nat} (p : Prepared capacity) (memory : Memory) : Memory :=
  writeRange (writeRange memory p.plan.output p.plan.eventBytes) p.plan.countAt p.plan.countBytes

def publish (capacity : Nat) (p : Plan) (memory : Memory) : Memory :=
  match prepare capacity p with
  | none => memory
  | some prepared => commit prepared memory

theorem rejection_preserves_memory (capacity : Nat) (p : Plan) (memory : Memory)
    (rejected : ¬ valid capacity p) : publish capacity p memory = memory := by
  simp [publish, prepare, rejected]

theorem invalid_last_input_rejects (capacity : Nat) (p : Plan)
    (invalid : capacity < p.input + p.requests.length * 48) : prepare capacity p = none := by
  have h : ¬ valid capacity p := by
    intro ⟨_, _, _, _, bound, _⟩
    omega
  simp [prepare, h]

theorem invalid_full_output_rejects (capacity : Nat) (p : Plan)
    (invalid : capacity < p.output + p.requests.length * 32) : prepare capacity p = none := by
  have h : ¬ valid capacity p := by
    intro ⟨_, _, _, _, _, bound, _⟩
    omega
  simp [prepare, h]

theorem overlapping_outputs_reject (capacity : Nat) (p : Plan)
    (a : p.output < p.countAt + 4) (b : p.countAt < p.output + p.requests.length * 32) :
    prepare capacity p = none := by
  have h : ¬ valid capacity p := by
    intro ⟨_, _, _, _, _, _, _, disjoint, _⟩
    rcases disjoint with d | d <;> omega
  simp [prepare, h]

theorem prepared_event_bytes_fit {capacity : Nat} (p : Prepared capacity) :
    p.plan.output + p.plan.eventBytes.length ≤ capacity := by
  rcases p.checked with ⟨_, _, _, matched, _, outBound, _, _, bytesLength, _⟩
  have countBound := matching_bounds_events p.plan.requests p.plan.events matched
  omega

theorem prepared_output_regions_disjoint {capacity : Nat} (p : Prepared capacity)
    (index : Nat) (eventPosition : inside p.plan.output p.plan.eventBytes.length index) :
    ¬ inside p.plan.countAt p.plan.countBytes.length index := by
  rcases p.checked with ⟨_, _, _, matched, _, _, _, disjoint, eventLength, countLength⟩
  have countBound := matching_bounds_events p.plan.requests p.plan.events matched
  unfold inside at *
  rcases disjoint with d | d <;> omega

theorem commit_preserves_event_bytes {capacity : Nat} (p : Prepared capacity)
    (memory : Memory) (index : Nat)
    (h : inside p.plan.output p.plan.eventBytes.length index) :
    commit p memory index = p.plan.eventBytes[index - p.plan.output]'(by unfold inside at h; omega) := by
  rw [commit, write_outside _ _ _ _ (prepared_output_regions_disjoint p index h)]
  exact write_inside _ _ _ _ h

theorem commit_writes_count_bytes {capacity : Nat} (p : Prepared capacity)
    (memory : Memory) (index : Nat)
    (h : inside p.plan.countAt p.plan.countBytes.length index) :
    commit p memory index = p.plan.countBytes[index - p.plan.countAt]'(by unfold inside at h; omega) :=
  write_inside _ _ _ _ h

theorem commit_preserves_outside {capacity : Nat} (p : Prepared capacity)
    (memory : Memory) (index : Nat)
    (eventOutside : ¬ inside p.plan.output p.plan.eventBytes.length index)
    (countOutside : ¬ inside p.plan.countAt p.plan.countBytes.length index) :
    commit p memory index = memory index := by
  rw [commit, write_outside _ _ _ _ countOutside, write_outside _ _ _ _ eventOutside]

theorem duplicate_occurrences_allowed : orderedMatch [(11, 1), (11, 1)] [(11, 1), (11, 1)] = true := by decide
theorem extra_occurrence_rejected : orderedMatch [(11, 1), (22, 0)] [(11, 1), (11, 1)] = false := by decide
theorem reordered_occurrences_rejected : orderedMatch [(11, 1), (22, 0)] [(22, 0), (11, 1)] = false := by decide
theorem changed_kind_rejected : orderedMatch [(11, 1), (22, 0)] [(11, 2)] = false := by decide
theorem missing_userdata_rejected : orderedMatch [(11, 1), (22, 0)] [(33, 1)] = false := by decide

#print axioms matching_is_sublist
#print axioms matching_bounds_events
#print axioms matching_never_fabricates
#print axioms matching_preserves_occurrence_count
#print axioms write_outside
#print axioms write_inside
#print axioms rejection_preserves_memory
#print axioms invalid_last_input_rejects
#print axioms invalid_full_output_rejects
#print axioms overlapping_outputs_reject
#print axioms prepared_event_bytes_fit
#print axioms prepared_output_regions_disjoint
#print axioms commit_preserves_event_bytes
#print axioms commit_writes_count_bytes
#print axioms commit_preserves_outside
#print axioms duplicate_occurrences_allowed
#print axioms extra_occurrence_rejected
#print axioms reordered_occurrences_rejected
#print axioms changed_kind_rejected
#print axioms missing_userdata_rejected
end WasiPoll
