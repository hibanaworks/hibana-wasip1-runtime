# hibana-wasip1-runtime

`hibana-wasip1-runtime` is a small Rust 2024, `#![no_std]`, WASI Preview 1
runtime boundary for Wasm guests that advance under Hibana choreography.

It parses a WASI P1 guest, runs it with explicit fuel, stops at every visible
guest boundary, lowers supported WASI imports into typed Hibana messages, and
resumes only after the matching typed completion is received.

```text
WASI P1 guest
  -> GuestMemory caller-owned backing
  -> HibanaWasiGuest::resume_hibana(..., BudgetRun)
  -> HibanaStep
  -> protocol::*ReqMsg
  -> local role offer/recv/send
  -> protocol::*RetMsg
  -> HibanaImportPending::complete(...)
```

This crate intentionally depends on `hibana`; it does not define a parallel
message system.

```toml
[dependencies]
hibana-wasip1-runtime = { path = "../hibana-wasip1-runtime" }
```

## What It Provides

The public boundary is deliberately narrow:

- guest stepper:
  `HibanaWasiGuestStorage`, `HibanaWasiGuest`, `HibanaStep`,
  `HibanaImportPending`, `HibanaMemoryGrowPending`;
- guest memory:
  `GuestMemory`, `GUEST_MEMORY_PAGE_SIZE`, `DEFAULT_GUEST_MEMORY_BYTES`;
- protocol payloads:
  `protocol::{BudgetRun, FdWriteReqMsg, PathOpenReqMsg, ...}`;
- route labels:
  `WasiImport`;
- fd bindings:
  `FdBindingTable`, `protocol::FdBinding`;
- ChoreoFS objects:
  `choreofs::{ChoreoFsObjectSet, ChoreoFs, ChoreoFsOpen, ChoreoFsRead,
  ChoreoFsReadDir, ChoreoFsWrite}`.

The runtime guarantees:

- execution advances only through `resume_hibana(..., BudgetRun)`;
- fuel exhaustion is visible as `HibanaStep::BudgetExpired`;
- supported WASI imports suspend as typed Hibana messages;
- unsupported imports fail closed while building the import plan;
- known imports with wrong signatures fail before guest execution begins;
- `memory.grow` yields a `MemoryGrowReqMsg` and resumes only after
  `MemoryGrowRetMsg`;
- process exit yields `HibanaStep::Exit`;
- guest-memory reads and writeback stay inside checked ABI paths;
- completion is linear because `HibanaImportPending::complete(...)` consumes the
  pending import.

This crate is not a general WASI host, filesystem implementation, socket
runtime, component-model adapter, shell loop, UI loop, policy table, or platform
facade.

## Mental Model

A WASI import is a synchronization point. The runtime may decode the import and
copy guest memory into bounded Rust values, but the outside Hibana choreography
decides whether progress may continue.

The shell app role runs the guest:

```rust,ignore
async fn run_shell_app_engine(
    guest: &mut HibanaWasiGuest<'_>,
    endpoint: &mut hibana::Endpoint<'_, SHELL_APP_ROLE>,
) -> Result<i32, Error> {
    let mut run_id = 1u16;
    loop {
        match guest
            .resume_hibana(endpoint, protocol::BudgetRun::new(run_id, 0, 100_000))
            .await?
        {
            HibanaStep::ImportPending(pending) => {
                pending.complete(guest, endpoint).await?;
            }
            HibanaStep::MemoryGrowPending(pending) => {
                pending.complete(guest, endpoint).await?;
            }
            HibanaStep::BudgetExpired(_) => run_id = run_id.wrapping_add(1),
            HibanaStep::Exit(exit) => return Ok(exit.status() as i32),
        }
    }
}
```

The local role remains ordinary Hibana endpoint code. Route boundaries use
`offer()`. Once an arm is selected, the continuation is direct typed
`recv` / `send`:

```rust,ignore
let branch = shell_env_endpoint.offer().await?;
match WasiImport::from_label(branch.label()).ok_or(Error::UnknownLabel)? {
    WasiImport::FdFdstatGet => {
        let request = branch.recv::<protocol::FdFdstatGetReqMsg>().await?;
        let response = shell_env.stat_fd(request.0);
        shell_env_endpoint
            .send::<protocol::FdFdstatGetRetMsg>(&response)
            .await?;

        let request = shell_env_endpoint
            .recv::<protocol::PathOpenReqMsg>()
            .await?;
        let response = shell_env.open_path(request.0)?;
        shell_env_endpoint
            .send::<protocol::PathOpenRetMsg>(&response)
            .await?;
    }
    import => return Err(Error::UnexpectedImport(import)),
}
```

There is no second support matrix here. If the guest reaches a supported runtime
import that the current choreography does not contain, the endpoint operation
fails as a Hibana localside error.

`memory.grow` is modeled the same way. The runtime stops before committing the
new page count:

```rust,ignore
let memory_grow = g::seq(
    g::send::<SHELL_APP_ROLE, SHELL_ENV_ROLE, protocol::MemoryGrowReqMsg>(),
    g::send::<SHELL_ENV_ROLE, SHELL_APP_ROLE, protocol::MemoryGrowRetMsg>(),
);
```

The local role may grant or reject the request. The runtime rechecks the reply
against `GuestMemory` capacity and the module limit before changing committed
pages.

## ChoreoFS

ChoreoFS is not a host filesystem. It is a typed object vocabulary for local
roles that want ordinary WASI `std::fs` calls to target choreography-owned
objects.

`ChoreoFs` turns an already-admitted WASI request into a concrete operation
token:

```rust,ignore
use hibana_wasip1_runtime::{
    choreofs::{ChoreoFsObject, ChoreoFsObjectSet, FdSpec, ObjectId},
    protocol::{self, FdBinding, FdWriteRow},
};

static OBJECTS: ChoreoFsObjectSet<1> = ChoreoFsObjectSet::new([
    ChoreoFsObject::writable(
        b"outputs/led/green",
        ObjectId(1),
        FdSpec::new(10, 1 << 6, 1),
        FdBinding::write(FdWriteRow::Refined),
    ),
]);

let choreofs = OBJECTS.choreofs();
let write = protocol::FdWrite::new(10, b"1")?;
let operation = choreofs.fd_write(write);

let object = operation.object();
let payload = operation.bytes();
let response = operation.written();
```

`ChoreoFsOpen`, `ChoreoFsRead`, `ChoreoFsReadDir`, and `ChoreoFsWrite` expose
the selected object and typed completion data for one request. They do not own
route selection, endpoint progress, host fallback behavior, or shell policy.

## WASI Shell Demo

The repository includes a host-side demonstration that runs an actual
`wasm32-wasip1` Rust `std` guest.

```text
examples/wasi_std_shell_app.rs
  -> ordinary Rust std::io / std::fs
  -> wasm32-wasip1
  -> hibana-wasip1-runtime
  -> typed Hibana choreography
```

Run the proof:

```sh
bash scripts/check_wasi_shell_demo.sh
```

The script builds the guest once, then runs two host choreographies:

- `direct_choreofs_write_rejection`: the guest may open
  `/outputs/led/green`, but the refined ChoreoFS write message is absent, so
  progress stops with a Hibana localside error.
- `sequenced_choreofs_write`: the guest must read `/objects/log` before it may
  open and write `/outputs/led/green`.

Expected proof shape:

```text
choreography: direct ChoreoFS write blocked
wasi std shell app
wasi> Hibana: ChoreoFS write did not advance on this localside -> EndpointError { ... }
Output: led.green = unchanged
choreography: sequenced ChoreoFS write
wasi std shell app
wasi> log
wasi> session=attached
wasi> applied
wasi> Output: led.green = on
```

The sequenced host can also be run interactively after the guest is built:

```sh
cargo run --locked --example sequenced_choreofs_write -- \
  target/wasi-std-shell-app/wasm32-wasip1/release/wasi-std-shell-app.wasm
```

Try:

```text
help
ls /objects
cat /objects/log
apply /objects/log /outputs/led/green
exit
```

The guest code uses ordinary `std::fs`:

```rust,ignore
for entry in std::fs::read_dir("/objects")? {
    println!("{}", entry?.file_name().to_string_lossy());
}

let log = std::fs::read_to_string("/objects/log")?;

let mut output = std::fs::OpenOptions::new()
    .write(true)
    .open("/outputs/led/green")?;
output.write_all(b"1")?;
```

Those paths are ChoreoFS object selectors. The host process filesystem is not
exposed, no directory tree is mirrored, and the runtime does not substitute a
local policy path when choreography rejects progress.

## Embedded Budget

Raspberry Pi Pico / RP2040 Core1-side execution is a real validation floor for
the runtime boundary. The design must not assume the full chip SRAM is available
to this crate.

The core path therefore prefers:

- caller-owned storage;
- explicit fuel and memory-growth pending boundaries;
- bounded guest-memory copies;
- small typed protocol values;
- direct hot-path copying after ABI checks;
- no hidden allocator requirement in guest execution;
- no normal-path formatting or broad host helper layer.

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

## Non-Goals

`hibana-wasip1-runtime` deliberately excludes:

- host filesystem fallback;
- socket runtime policy;
- component-model adaptation;
- platform frontend policy;
- shell or harness orchestration;
- syscall availability profiles;
- compatibility aliases for removed protocol variants;
- platform-family names in protocol labels, event variants, or feature names.

The intended result is a compact Hibana-bound WASI P1 runtime layer: typed at the
domain boundary, direct in the hot execution path, and explicit about every
guest suspension point.

## License

Licensed under either of Apache License, Version 2.0 or MIT license at your
option.
