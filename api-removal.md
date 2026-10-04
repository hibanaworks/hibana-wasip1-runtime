# Runtime surface and ownership reduction

The runtime uses one `no_std`, no-allocation bounded implementation for host and
Pico. No public row, label, type, scheduler API, device registry, or compatibility
entry point is added. The existing poll payloads and message aliases become
borrowed validated ABI records; their lifetimes are normally inferred.

| Public API | Canonical surface |
| --- | --- |
| `PollOneoff::timeout_nanos` | Replaced by `subscriptions()` on the same payload type. It retains all meaningful ABI fields. |
| `PollReady::ready` | Replaced by `events()` on the same payload type. It carries actual matching event records. |
| `WasiImportPending::import` | Deleted; the typed request already identifies its row. |
| `WasiMemoryGrowPending::{previous_pages, requested_pages, max_pages}` | Deleted; `request().0` is the single protocol snapshot. |
| `WasiImportPending::complete(guest, response)` | `complete(response)`; the token exclusively borrows the original guest. |
| `WasiMemoryGrowPending::complete(guest, decision)` | `complete(decision)` with the same ownership discipline. |

Four redundant public methods are removed. The payload constructors and getters
are replaced, with no increase in their number. The two borrow lifetimes on a
pending token distinguish its short execution borrow from the module/memory
lifetime; they are required by mutable-reference invariance, and are inferred
outside signatures that Rust requires to spell them.

The private duplicated call enum and the stored request copies are deleted.
The pending token retains the original VM call and exclusive guest/binding
borrow. Request lowering and completion derive the fd row from that stable
binding table. They preserve the existing typed row mismatch and fd mismatch
checks; no new dispatcher or authority is introduced.

The poll wire format is one count byte followed by canonical ABI records.
Incompatible scalar encodings are erased; poll schema identities change to
`0x57500023` and `0x57500024`. Message labels remain the same. There is no old
wire decoder, alias, format selection flag, or compatibility path.

All input records and complete writeback ranges are validated before creating
an exclusive prepared output pair. Commit has no fallible step between event
and count writes. Existing path-open/fd-close binding publication ordering is
preserved. [Lean scope and correspondence](proofs/README.md) separates the
general prepared-write model from finite Rust evidence and driver guarantees.
