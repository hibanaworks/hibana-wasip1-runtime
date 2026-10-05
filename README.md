# hibana-wasip1-runtime

`hibana-wasip1-runtime` is a Rust 2024, `#![no_std]`, bounded WASI Preview 1
runtime boundary for Wasm guests that advance under Hibana choreography.

The crate is intentionally narrow. It parses and runs a WASI P1 guest, copies
data across the guest-memory ABI, lowers supported imports into typed Hibana
messages, and resumes the guest only after the matching typed completion is
received.

```text
WASI P1 guest
  -> GuestMemory
  -> HibanaWasiGuest::resume_wasi_boundary(..., BudgetRun)
  -> WasiBoundaryStep
  -> WasiImportRequest / WasiImportCompletion
  -> Hibana Endpoint send::<protocol::*ReqMsg>() / recv::<protocol::*RetMsg>()
```

It depends on `hibana`; it does not define a second message system, a host
policy layer, or a filesystem fallback.

Version `0.1` targets Hibana `0.9.6` and its schema-identified wire payload
contract.

## Install

```bash
cargo add hibana-wasip1-runtime
```

Or write the dependency explicitly:

```toml
[dependencies]
hibana-wasip1-runtime = "0.1"
```

## What This Crate Is

This crate is for embedding a WASI P1 guest inside a Hibana protocol. The guest
executes until it reaches a visible boundary, and that boundary becomes a typed
protocol event.

The core path is:

1. caller provides a Wasm module, `GuestMemory`, fd bindings, and Hibana
   endpoint;
2. `resume_wasi_boundary(..., BudgetRun)` runs the guest with explicit fuel;
3. the runtime stops at budget exhaustion, a supported WASI import,
   `memory.grow`, or process exit;
4. supported imports become `WasiImportRequest` values that the caller sends
   through typed Hibana endpoint operations;
5. the outside local role answers with the matching `protocol::*RetMsg`;
6. the runtime performs checked writeback and resumes only through the consumed
   pending event.

Authority lives in choreography. The runtime can decode an import and preserve
the ABI contract, but it does not decide which operation is allowed in a given
session.

## Public Surface

There are four public surfaces:

| Surface | Used for | Main names |
| --- | --- | --- |
| Engine stepper | running the guest to a WASI boundary | `HibanaWasiGuestStorage`, `HibanaWasiGuest`, `BudgetRun`, `WasiBoundaryStep`, `WasiImportPending`, `WasiMemoryGrowPending`, `WasiImportRequest`, `WasiImportCompletion` |
| Guest memory | caller-owned WASM linear-memory backing | `GuestMemory`, `GUEST_MEMORY_PAGE_SIZE`, `DEFAULT_GUEST_MEMORY_BYTES` |
| Protocol payloads | Hibana message payloads for WASI P1 imports and memory growth | `protocol::*ReqMsg`, `protocol::*RetMsg`, `MemoryGrowReqMsg`, `MemoryGrowRetMsg` |
| ChoreoFS facts | object and fd facts a local role can use while answering admitted WASI calls | `ChoreoFsObjectSet`, `ChoreoFs`, `ChoreoFsOpen`, `ChoreoFsRead`, `ChoreoFsReadDir`, `ChoreoFsWrite`, `FdBindingTable` |

Application code should read the global choreography and the local-side endpoint
operations. It should not need to reverse-engineer a hidden syscall table or a
host callback registry.

## Runtime Contract

The runtime advances through one explicit operation:

```rust,ignore
let step = guest
    .resume_wasi_boundary(protocol::BudgetRun::new(run_id, generation, fuel))?;
```

Each resume returns exactly one visible state:

- `WasiBoundaryStep::ImportPending`: a supported WASI import was lowered to a
  typed request and must be sent through the endpoint, answered, and completed
  with the matching return value;
- `WasiBoundaryStep::MemoryGrowPending`: `memory.grow` was requested and must
  be sent through the endpoint, then granted or rejected by `MemoryGrowRetMsg`;
- `WasiBoundaryStep::BudgetExpired`: fuel ended before another visible
  boundary;
- `WasiBoundaryStep::Exit`: the guest called `proc_exit` or returned from start.

Unsupported imports fail closed while the import plan is built. Known imports
with wrong signatures fail before guest execution begins. Completion is linear:
`WasiImportPending::complete(...)` and `WasiMemoryGrowPending::complete(...)`
consume the pending value, so a response cannot be reused for a later import.
Each token retains an exclusive borrow of its original guest and fd bindings.
Completion takes only the typed response; it cannot be directed to another guest,
and execution cannot resume while the token remains live. `request()` on an
import returns a validated borrowed view; the token stores the VM call itself,
without a second call enum or a retained copy of the protocol request.
`HibanaWasiGuestStorage` is one-shot even when initialization fails, so partially
initialized in-place storage is never retried.
Every message payload has a stable Hibana `SCHEMA_ID`; malformed or
non-canonical bytes are rejected before decode. Multi-region ABI writeback is
prepared in full before publication, so an invalid later range cannot leave an
earlier guest-memory write committed.

Successful `args_sizes_get` / `environ_sizes_get` completions establish the
exact compact list layout accepted by the following `args_get` /
`environ_get`. The data completion must match that count and byte length
exactly; otherwise the pending call remains uncommitted.

The crate deliberately excludes:

- host filesystem fallback;
- socket runtime policy;
- component-model adaptation;
- UI or shell loop policy;
- syscall availability profiles;
- compatibility aliases for removed protocol variants;
- platform-family names in protocol labels, event variants, or feature names.

## Supported Profile

This is not a general-purpose WebAssembly engine or a complete implementation
of all WASI P1 imports. It is one bounded profile whose unsupported module
shapes and imports are rejected while the module is loaded.

The supported WASI P1 imports are:

```text
args_get              args_sizes_get       clock_res_get
clock_time_get        environ_get          environ_sizes_get
fd_close              fd_fdstat_get        fd_filestat_get
fd_prestat_dir_name   fd_prestat_get       fd_read
fd_readdir            fd_write             path_filestat_get
path_open             poll_oneoff          proc_exit
random_get
```

`poll_oneoff` accepts 1–16 WASI subscriptions: clocks and fd read/write
interests. Borrowed 48-byte subscription records retain userdata, clock ID,
nanosecond timeout/precision and relative/absolute flags. Borrowed 32-byte event
records carry the selected userdata/kind, errno, nbytes and hangup flag. Only
meaningful ABI fields enter the canonical wire encoding; padding is zero.
Replies are nonempty ordered subsequences of requested occurrences, including
duplicate userdata. A reply consumes each matching occurrence once.
The complete input and reserved output ranges, output count and disjoint event/
count regions are validated before any guest write. Input/output aliasing is
allowed after validation. Readiness is correspondence data, not I/O authority;
fd rights, incarnation and physical readiness remain the answering owner's job.
The format replaces the scalar poll format and uses distinct schema identities.
There is one bounded `no_std` path for Pico and host users.
I/O completion payloads carry at most 96 bytes, path payloads at most 40 bytes,
and the fd table holds 16 live bindings while accepting any `u8` fd number.

The current Wasm profile is bounded as follows:

| Area | Ceiling |
| --- | ---: |
| types / imports / functions / globals | 24 / 16 / 224 / 16 |
| parameters / results per function | 12 / 1 |
| value stack / locals / call frames | 64 / 32 / 16 |
| control frames / decoded control targets | 128 / 192 |
| `br_table` labels / table functions | 64 / 64 |
| data segments / element segments | 8 / 8 |

These are rejection ceilings, not truncation points. Raising one requires
remeasuring the embedded object budget and adding its exact boundary test.
`f32.sqrt` and `f64.sqrt` are rejected instead of being approximated; adding
them requires a correctly rounded `no_std` implementation and a measured Pico
flash budget.

## Hibana Integration

A WASI row is ordinary `hibana::g` choreography:

```rust,ignore
let fd_read = g::seq(
    g::send::<APP, ENV, protocol::FdReadReqMsg>(),
    g::send::<ENV, APP, protocol::FdReadRetMsg>(),
);

let memory_grow = g::seq(
    g::send::<APP, ENV, protocol::MemoryGrowReqMsg>(),
    g::send::<ENV, APP, protocol::MemoryGrowRetMsg>(),
);

let program = g::route(memory_grow, fd_read).roll();
```

The guest-running role is small because Hibana already owns endpoint progress.
The following is an excerpt; the repository examples contain the exhaustive,
compiled match over all admitted imports:

```rust,ignore
async fn run_guest<const ROLE: u8>(
    guest: &mut HibanaWasiGuest<'_>,
    endpoint: &mut hibana::Endpoint<'_, ROLE>,
) -> Result<i32, Error> {
    let mut run_id = 1u16;
    loop {
        match guest.resume_wasi_boundary(protocol::BudgetRun::new(run_id, 0, 100_000))? {
            WasiBoundaryStep::ImportPending(pending) => {
                match pending.request()? {
                    WasiImportRequest::FdRead(request) => {
                        endpoint.send::<protocol::FdReadReqMsg>(&request).await?;
                        let done = endpoint.recv::<protocol::FdReadRetMsg>().await?;
                        pending.complete(WasiImportCompletion::FdRead(done))?;
                    }
                    WasiImportRequest::FdWriteObject(request) => {
                        endpoint.send::<protocol::FdWriteObjectReqMsg>(&request).await?;
                        let done = endpoint.recv::<protocol::FdWriteObjectRetMsg>().await?;
                        pending.complete(WasiImportCompletion::FdWriteObject(done))?;
                    }
                    // Other admitted imports follow the same direct Hibana row shape:
                    // send the matching protocol::*ReqMsg, receive protocol::*RetMsg,
                    // then complete the pending import with WasiImportCompletion.
                }
            }
            WasiBoundaryStep::MemoryGrowPending(pending) => {
                let request = pending.request();
                endpoint.send::<protocol::MemoryGrowReqMsg>(&request).await?;
                let decision = endpoint.recv::<protocol::MemoryGrowRetMsg>().await?;
                pending.complete(decision)?;
            }
            WasiBoundaryStep::BudgetExpired(_) => run_id = run_id.wrapping_add(1),
            WasiBoundaryStep::Exit(exit) => return Ok(exit.status() as i32),
        }
    }
}
```

The answering role is ordinary Hibana local-side code. At route boundaries it
uses `offer()`. Inside the selected arm, it uses typed `recv()` and `send()` for
the request and return messages that the global choreography admitted.

```rust,ignore
let branch = endpoint.offer().await?;

if branch.label() == protocol::LABEL_WASI_FD_READ {
    let protocol::FdReadReq(request) = branch.recv::<protocol::FdReadReqMsg>().await?;
    let read = choreofs.fd_read(request);
    let (response, next_offset) = read.read_from(offset)?;
    offset = next_offset;
    endpoint.send::<protocol::FdReadRetMsg>(&response).await?;
}
```

If a guest reaches a supported import that the current choreography does not
contain, the endpoint operation fails as a Hibana local-side error. There is no
separate runtime-side support matrix that reauthorizes it.

## Memory Growth

`memory.grow` is a protocol boundary. The runtime stops before changing the
committed page count and sends `protocol::MemoryGrowReqMsg`.

The outside role replies with `protocol::MemoryGrowRetMsg`:

- grant: the runtime rechecks `GuestMemory` capacity and the module limit, then
  commits pages and returns the previous page count to the guest;
- reject: the runtime leaves committed pages unchanged and returns `u32::MAX`
  to the guest.

`GuestMemory` is caller-owned backing storage. This makes embedded budgets and
host harness budgets explicit instead of hiding allocation inside the engine.

## ChoreoFS

ChoreoFS is a typed object vocabulary for local roles that want a WASI guest's
ordinary `std::fs` calls to address choreography-owned objects.

It is not a host filesystem. It does not own route selection, endpoint progress,
or fallback behavior. A local role uses ChoreoFS only after choreography has
admitted the corresponding WASI row.

Typical flow:

```text
std::fs in the guest
  -> WASI P1 import
  -> protocol::PathOpenReqMsg / FdReadReqMsg / FdWriteReqMsg
  -> Hibana choreography admits or rejects progress
  -> local role optionally uses ChoreoFS object and fd facts
  -> matching protocol::*RetMsg
```

`ChoreoFsOpen`, `ChoreoFsRead`, `ChoreoFsReadDir`, and `ChoreoFsWrite` are
operation tokens for one already-admitted request. They expose selected object
facts and produce typed completion payloads; they do not replace Hibana route
authority.

## Examples

The repository includes one guest program and two host choreographies:

| Path | Purpose |
| --- | --- |
| `examples/wasi_std_shell_app.rs` | a real `wasm32-wasip1` Rust `std` guest using `std::io` and `std::fs` |
| `examples/direct_choreofs_write_rejection` | demonstrates that a direct write does not advance when the ChoreoFS object write row is absent |
| `examples/sequenced_choreofs_write` | demonstrates that choreography can require reading `/objects/log` before writing `/outputs/led/green` |

Run the demonstration:

```sh
bash scripts/check_wasi_shell_demo.sh
```

The important point is not the shell UI. The executable evidence shows that
changing the Hibana choreography changes which WASI guest progress is possible,
while the guest continues to use ordinary Rust `std` APIs.

## Embedded Budget

The same public API is compiled for Pico 2 (`thumbv8m.main-none-eabi`). On ARM, a compile-time
assertion limits the VM object to 32 KiB. `DEFAULT_GUEST_MEMORY_BYTES` is one
64 KiB Wasm page, so the VM object plus default guest backing is bounded by
96 KiB before caller-owned Hibana session, transport, and application storage.

Guest memory remains caller-owned. `memory.grow` cannot commit beyond that
backing or the module limit. The hot path performs no allocation and uses
explicit fuel, bounded copies, compact typed payloads, and checked ABI ranges.
Host-only example code is not part of this resource claim.

`python3 scripts/check_pico2.py` executes a 16-interest poll guest on the host
and links the same runtime fixture for Cortex-M33. The fixture includes one
64 KiB linear memory, caller-owned VM storage and a 24 KiB stack reservation.
It checks a 128 KiB reserved-RAM limit and a 512 KiB flash-load limit, with no
allocator or separate board API. Linking does not establish physical Pico 2
execution, worst-case stack usage, or whole-application resource fit.

## Build And Test

Repository builds use the sibling `../hibana` checkout through the root Cargo
patch. Check out Hibana's `development/rolled-route-ownership` branch there;
the [CI workflow](.github/workflows/quality.yml) pins the verified core revision.
The proof gate uses its Lean 4.30.0 workspace. Package verification also rebuilds
the normalized crate against the registry dependency.

Run the full local gate:

```sh
bash scripts/check_runtime_gates.sh
```

Focused checks:

```sh
cargo test --locked choreofs
cargo check --locked --example sequenced_choreofs_write
cargo check --locked --example direct_choreofs_write_rejection
bash scripts/check_miri.sh
bash scripts/check_wasi_shell_demo.sh
```

The gates cover import decoding, unsupported import rejection, guest-memory
bounds, atomic writeback, pending-call mismatch rejection, canonical
argument/environment payloads, memory-growth pending, fuel suspension, restart
behavior, ChoreoFS object lookup, example behavior, clippy,
Miri, Pico 2 compilation and resource linking, documentation, and package verification. CI runs
the same gate with Rust `1.95.0` and Miri `nightly-2026-05-28`.

## License

Licensed under either of Apache License, Version 2.0 or MIT license at your
option.

## Poll proof and verification

`python3 scripts/check_poll.py` runs all-target tests, Clippy, Pico 2
compilation, the affine ownership compile-fail test, and Lean kernel checks.
Each Rust verification uses a temporary target that is deleted on success or
failure. The gate verifies 20 model theorems and 520 admission decisions exported
from actual Rust VM execution. [Proof scope](proofs/README.md) records the
unmodeled boundaries and the exact axiom audit. [Surface reduction](api-removal.md)
records the removed APIs and ownership changes.
