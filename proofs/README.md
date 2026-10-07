# Bounded WASI poll and path-rights evidence

Run `python3 scripts/check_poll.py` from the runtime repository. Rust tests,
Clippy, Pico 2 (`thumbv8m.main-none-eabi`) compilation and the affine ownership compile-fail
check each use a separate temporary target. Every target is deleted at the end
of its check, including failure. Sources, decision exports and logs remain.
The proof uses the local Hibana Lean 4.30.0 workspace in `../hibana/proofs/lean`.

Run `python3 scripts/check_pico2.py` for a separate resource gate. It runs a
16-subscription guest on the host, then links the same fixture for Cortex-M33
with the ordinary runtime API, one 64 KiB linear memory and caller-owned VM
storage. The gate counts linked flash sections and static RAM plus a 24 KiB
stack reservation. Its limits are 512 KiB flash and 128 KiB reserved RAM.
These are fixture budgets, not whole-StackChan budgets. The link fixture has
no RP2350 boot metadata or hardware startup; it establishes linking and storage
fit, not physical Pico 2 execution or a worst-case stack bound. No allocator,
special Pico API or reduced runtime implementation is used. Rust products are
deleted after each host or target check; logs and numeric evidence remain.

`PollWriteback.lean` proves 15 general statements and 5 finite cases. Greedy
ordered matching yields a sublist of requested `(userdata, event kind)` keys,
so replies cannot invent a key or consume more matching occurrences than were
requested. A prepared write has validated the complete subscription input,
full reserved output, count output and disjoint output regions. Rejection
preserves memory; commit preserves supplied event/count bytes and all other
memory. Input/output overlap is permitted after all input validation finishes.

The gate exports 520 decisions from the actual Rust VM, including zero-length
requests/replies, duplicate userdata, changed kinds, reversed order and the
maximum u64 userdata. Each accepted case checks actual ABI event bytes and
count writeback; each rejected case checks unchanged output and count. Lean
checks every exported admission decision with `decide`. The exact axiom audit
permits only `propext` and `Quot.sound` for the general model theorems, and no
axioms for finite decisions. There are no custom axioms, `sorry`, or
`native_decide` in the proof.

The model assumes canonical encoded event/count bytes supplied to its plan.
The finite correspondence is not a general Rust refinement proof or a proof of
ABI serialization for every byte value. Pointer-width arithmetic, VM state,
Hibana endpoint progress, fd incarnation/rights, clock behavior, driver
readiness, physical I/O and OS fairness remain separate boundaries. A matching
key only proves request/reply correspondence; it never authorizes an operation.
The runtime retains the guest's exclusive borrow while an import or memory-grow
request is pending. Rust checks that execution cannot resume while that token
is live; Lean does not model Rust's borrow checker.

The 48-byte subscription and 32-byte event layouts and clock flag meanings
were checked against the [WASI Preview 1 specification](https://github.com/WebAssembly/WASI/blob/snapshot-01/phases/snapshot/docs.md#subscription-struct).

`PathRights.lean` proves six statements about fixed ChoreoFS I/O capabilities:
an accepted object has a nonempty capability and the exact applicable I/O mode;
an incompatible mode rejects; a writer requires the write bit; a read request
cannot open a writer; empty materials reject. The gate exports 128 decisions
from actual Rust `ChoreoFs::path_open` calls: four material kinds, every
combination of the three I/O bits, and four ancillary-bit patterns including
the high bit and every non-I/O bit. Denied calls return no descriptor or binding.
The kernel checks every decision; the exact axiom audit permits only `propext`
for all six general theorems and 128 finite decisions.

`PathRights.smt2` checks all 64-bit base-right patterns. Four UNSAT queries rule
out empty grants, added I/O rights, silently dropped conflicting applicable
modes, and read admission on writers. Three SAT witnesses cover the old
writable-default error, the old base/inheriting union, and a genuine write.
The VM regression passes different base/inheriting masks, including u64::MAX,
and checks unchanged descriptor output on denied completion. The actual
compiled std shell rejects a read on its write-only LED object before its
existing choreography rejection check. The sequenced shell rejects a write to
its read-only log and then completes a valid write in the same session.

WASI permits removing rights that do not apply to the object type. The model
therefore excludes `FD_READDIR` on files and `FD_READ` on directories from the
applicable mode; it never substitutes read and write on a file. This admission
model does not prove all WASI rights, inheritance, fd lifetime, dynamic rights
attenuation, Rust execution, Hibana progression, or physical side effects.
The Rust binding and ledger checks remain independently required and tested.
Directory fdstat reports enumeration as base rights and supported child I/O as
inheriting rights; file fdstat reports only its base mode with no inheritance.
The child is admitted only by its own material, binding, ledger and choreography.
This fixed-capability directory convention is tested in Rust and the compiled
std shell; it is not modeled as a general inherited-rights attenuation proof.
