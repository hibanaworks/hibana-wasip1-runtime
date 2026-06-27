# hibana-wasip1-runtime

`hibana-wasip1-runtime` is a Rust 2024, `#![no_std]` WASI Preview 1
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
| Engine stepper | running the guest to a WASI boundary | `HibanaWasiGuestStorage`, `HibanaWasiGuest`, `WasiBoundaryStep`, `WasiImportPending`, `WasiMemoryGrowPending`, `WasiImportRequest`, `WasiImportCompletion` |
| Guest memory | caller-owned WASM linear-memory backing | `GuestMemory`, `GUEST_MEMORY_PAGE_SIZE`, `DEFAULT_GUEST_MEMORY_BYTES` |
| Protocol payloads | Hibana message payloads for WASI P1 imports and runtime events | `protocol::*ReqMsg`, `protocol::*RetMsg`, `BudgetRun`, `MemoryGrowReqMsg`, `MemoryGrowRetMsg` |
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

The crate deliberately excludes:

- host filesystem fallback;
- socket runtime policy;
- component-model adaptation;
- UI or shell loop policy;
- syscall availability profiles;
- compatibility aliases for removed protocol variants;
- platform-family names in protocol labels, event variants, or feature names.

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

The guest-running role is small because Hibana already owns endpoint progress:

```rust,ignore
async fn run_guest<const ROLE: u8>(
    guest: &mut HibanaWasiGuest<'_>,
    endpoint: &mut hibana::Endpoint<'_, ROLE>,
) -> Result<i32, Error> {
    let mut run_id = 1u16;
    loop {
        match guest.resume_wasi_boundary(protocol::BudgetRun::new(run_id, 0, 100_000))? {
            WasiBoundaryStep::ImportPending(pending) => {
                match pending.request() {
                    WasiImportRequest::FdRead(request) => {
                        endpoint.send::<protocol::FdReadReqMsg>(&request).await?;
                        let done = endpoint.recv::<protocol::FdReadRetMsg>().await?;
                        pending.complete(guest, WasiImportCompletion::FdRead(done))?;
                    }
                    WasiImportRequest::FdWriteObject(request) => {
                        endpoint.send::<protocol::FdWriteObjectReqMsg>(&request).await?;
                        let done = endpoint.recv::<protocol::FdWriteObjectRetMsg>().await?;
                        pending.complete(guest, WasiImportCompletion::FdWriteObject(done))?;
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
                pending.complete(guest, decision)?;
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
| `examples/direct_choreofs_write_rejection` | proves that a direct write does not advance when the ChoreoFS object write row is absent |
| `examples/sequenced_choreofs_write` | proves that choreography can require reading `/objects/log` before writing `/outputs/led/green` |

Run the demonstration:

```sh
bash scripts/check_wasi_shell_demo.sh
```

The important point is not the shell UI. The proof is that changing the Hibana
choreography changes which WASI guest progress is possible, while the guest
continues to use ordinary Rust `std` APIs.

## Embedded Budget

Raspberry Pi Pico / RP2040 Core1-side execution is a real validation floor for
the runtime boundary. The design must not assume that the full chip SRAM is
available to this crate.

The core path therefore prefers caller-owned storage, explicit fuel, visible
memory-growth boundaries, bounded guest-memory copies, small typed protocol
values, and direct hot-path copying after ABI checks. Host conveniences may
exist in examples, but the engine contract must stay allocation-aware and
reviewable for the reduced Core1-side budget.

The Pico constraint is a resource budget and validation target. It is not a
public protocol naming scheme.

## Build And Test

Run the full local gate:

```sh
bash scripts/check_runtime_gates.sh
```

Focused checks:

```sh
cargo test --locked choreofs
cargo check --locked --example sequenced_choreofs_write
cargo check --locked --example direct_choreofs_write_rejection
bash scripts/check_runtime_residue.sh
bash scripts/check_wasi_shell_demo.sh
```

The gates cover import decoding, unsupported import rejection, guest-memory
bounds, writeback, pending-call mismatch rejection, memory-growth pending, fuel
suspension, restart behavior, ChoreoFS object lookup, example behavior, residue
scans, clippy, and embedded-oriented compilation checks.

## License

Licensed under either of Apache License, Version 2.0 or MIT license at your
option.
