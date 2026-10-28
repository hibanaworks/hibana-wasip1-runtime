mod shell_env;
#[path = "../wasi_shell_demo_lib/lib.rs"]
mod wasi_shell_demo_lib;

use std::{env, fs, path::PathBuf, process};

use hibana::{
    Endpoint, g,
    runtime::{
        SessionKitStorage,
        ids::SessionId,
        program::{Projectable, RoleProgram, project},
    },
};
use hibana_wasip1_runtime::{
    DEFAULT_GUEST_MEMORY_BYTES, GuestMemory,
    exchange::{
        HibanaWasiGuest, HibanaWasiGuestStorage, WasiBoundaryStep, WasiImport,
        WasiImportCompletion, WasiImportRequest,
    },
    protocol,
};

use crate::shell_env::{ShellEnv, initial_bindings};
use crate::wasi_shell_demo_lib::{
    error::{DemoError, DemoResult},
    transport::InProcessTransport,
};

const SHELL_APP_ROLE: u8 = 0;
const SHELL_ENV_ROLE: u8 = 1;
const SHELL_IO_ROLE: u8 = 2;
const RETURN_ENV_LABEL: u8 = 201;
type Environment<'r> = g::Msg<200, Endpoint<'r, SHELL_IO_ROLE>>;
type ReturnEnvironment<'r> = g::Msg<RETURN_ENV_LABEL, Endpoint<'r, SHELL_IO_ROLE>>;
type GuestExited = g::Msg<202, i32>;
type GuestFailed = g::Msg<203, i32>;
const SESSION_ID: u32 = 0x5750_0001;

fn main() {
    match run() {
        Ok(status) => process::exit(status),
        Err(error) => {
            eprintln!("direct-choreofs-write-rejection failed: {error}");
            process::exit(1);
        }
    }
}

fn run() -> DemoResult<i32> {
    let module_path = parse_host_args()?;
    let module = fs::read(&module_path)?;

    println!("choreography: direct ChoreoFS write blocked");
    let program = direct_choreofs_write_blocked_choreography();
    let mut shell_env = ShellEnv::new();
    match run_guest_session(&module, &program, &mut shell_env) {
        Err(DemoError::Endpoint(error)) => {
            println!("Hibana: ChoreoFS write did not advance on this localside -> {error:?}");
            print_output_state(&shell_env);
            Ok(0)
        }
        result => result,
    }
}

// Global choreography. It contains ordinary read/write/open rows, but no
// ChoreoFS object write row, so the direct output write cannot advance.
fn direct_choreofs_write_blocked_choreography() -> impl hibana::runtime::program::Projectable {
    let memory_grow = g::seq(
        g::send::<SHELL_IO_ROLE, SHELL_ENV_ROLE, protocol::MemoryGrowReqMsg>(),
        g::send::<SHELL_ENV_ROLE, SHELL_IO_ROLE, protocol::MemoryGrowRetMsg>(),
    );
    let fd_write = g::seq(
        g::send::<SHELL_IO_ROLE, SHELL_ENV_ROLE, protocol::FdWriteReqMsg>(),
        g::send::<SHELL_ENV_ROLE, SHELL_IO_ROLE, protocol::FdWriteRetMsg>(),
    );
    let fd_read = g::seq(
        g::send::<SHELL_IO_ROLE, SHELL_ENV_ROLE, protocol::FdReadReqMsg>(),
        g::send::<SHELL_ENV_ROLE, SHELL_IO_ROLE, protocol::FdReadRetMsg>(),
    );
    let fd_fdstat_get = g::seq(
        g::send::<SHELL_IO_ROLE, SHELL_ENV_ROLE, protocol::FdFdstatGetReqMsg>(),
        g::send::<SHELL_ENV_ROLE, SHELL_IO_ROLE, protocol::FdFdstatGetRetMsg>(),
    );
    let path_open = g::seq(
        g::send::<SHELL_IO_ROLE, SHELL_ENV_ROLE, protocol::PathOpenReqMsg>(),
        g::send::<SHELL_ENV_ROLE, SHELL_IO_ROLE, protocol::PathOpenRetMsg>(),
    );
    let prestat = g::seq(
        g::send::<SHELL_IO_ROLE, SHELL_ENV_ROLE, protocol::FdPrestatGetReqMsg>(),
        g::send::<SHELL_ENV_ROLE, SHELL_IO_ROLE, protocol::FdPrestatGetRetMsg>(),
    );
    let prestat_name = g::seq(
        g::send::<SHELL_IO_ROLE, SHELL_ENV_ROLE, protocol::FdPrestatDirNameReqMsg>(),
        g::send::<SHELL_ENV_ROLE, SHELL_IO_ROLE, protocol::FdPrestatDirNameRetMsg>(),
    );

    let open_selector_flow = g::seq(fd_fdstat_get, path_open);

    let imports = g::route(
        memory_grow,
        g::route(
            fd_write,
            g::route(
                fd_read,
                g::route(prestat, g::route(prestat_name, open_selector_flow)),
            ),
        ),
    )
    .roll();
    let finish = || {
        g::route(
            g::seq(
                g::send::<SHELL_IO_ROLE, SHELL_ENV_ROLE, GuestExited>(),
                g::send::<SHELL_APP_ROLE, SHELL_ENV_ROLE, ReturnEnvironment<'_>>(),
            ),
            g::seq(
                g::send::<SHELL_IO_ROLE, SHELL_ENV_ROLE, GuestFailed>(),
                g::send::<SHELL_APP_ROLE, SHELL_ENV_ROLE, ReturnEnvironment<'_>>(),
            ),
        )
    };
    g::seq(
        g::send::<SHELL_ENV_ROLE, SHELL_APP_ROLE, Environment<'_>>(),
        g::route(g::seq(imports, finish()), finish()),
    )
}

// Shell app role. The runtime resumes the guest and completes each pending
// import through this role's endpoint.
async fn run_shell_app_engine(
    guest: &mut HibanaWasiGuest<'_>,
    control: &mut Endpoint<'_, SHELL_APP_ROLE>,
) -> DemoResult<i32> {
    let mut io = control.recv::<Environment<'_>>().await?;
    let mut run_id = 1u16;
    loop {
        match guest.resume_wasi_boundary(protocol::BudgetRun::new(run_id, 0, 100_000))? {
            WasiBoundaryStep::ImportPending(pending) => match pending.request()? {
                WasiImportRequest::FdWrite(request) => {
                    io.send::<protocol::FdWriteReqMsg>(&request).await?;
                    let completion = io.recv::<protocol::FdWriteRetMsg>().await?;
                    pending.complete(WasiImportCompletion::FdWrite(completion))?;
                }
                WasiImportRequest::FdWriteObject(request) => {
                    println!(
                        "ChoreoFS object write attempt: fd={} bytes={:?}",
                        request.0.fd(),
                        request.0.as_bytes()
                    );
                    io.send::<protocol::FdWriteObjectReqMsg>(&request).await?;
                    let completion = io.recv::<protocol::FdWriteObjectRetMsg>().await?;
                    pending.complete(WasiImportCompletion::FdWriteObject(completion))?;
                }
                WasiImportRequest::FdRead(request) => {
                    io.send::<protocol::FdReadReqMsg>(&request).await?;
                    let completion = io.recv::<protocol::FdReadRetMsg>().await?;
                    pending.complete(WasiImportCompletion::FdRead(completion))?;
                }
                WasiImportRequest::FdReaddir(request) => {
                    io.send::<protocol::FdReaddirReqMsg>(&request).await?;
                    let completion = io.recv::<protocol::FdReaddirRetMsg>().await?;
                    pending.complete(WasiImportCompletion::FdReaddir(completion))?;
                }
                WasiImportRequest::PathOpen(request) => {
                    io.send::<protocol::PathOpenReqMsg>(&request).await?;
                    let completion = io.recv::<protocol::PathOpenRetMsg>().await?;
                    pending.complete(WasiImportCompletion::PathOpen(completion))?;
                }
                WasiImportRequest::FdPrestatGet(request) => {
                    io.send::<protocol::FdPrestatGetReqMsg>(&request).await?;
                    let completion = io.recv::<protocol::FdPrestatGetRetMsg>().await?;
                    pending.complete(WasiImportCompletion::FdPrestatGet(completion))?;
                }
                WasiImportRequest::FdPrestatDirName(request) => {
                    io.send::<protocol::FdPrestatDirNameReqMsg>(&request)
                        .await?;
                    let completion = io.recv::<protocol::FdPrestatDirNameRetMsg>().await?;
                    pending.complete(WasiImportCompletion::FdPrestatDirName(completion))?;
                }
                WasiImportRequest::FdFilestatGet(request) => {
                    io.send::<protocol::FdFilestatGetReqMsg>(&request).await?;
                    let completion = io.recv::<protocol::FdFilestatGetRetMsg>().await?;
                    pending.complete(WasiImportCompletion::FdFilestatGet(completion))?;
                }
                WasiImportRequest::ArgsSizesGet(request) => {
                    io.send::<protocol::ArgsSizesGetReqMsg>(&request).await?;
                    let completion = io.recv::<protocol::ArgsSizesGetRetMsg>().await?;
                    pending.complete(WasiImportCompletion::ArgsSizesGet(completion))?;
                }
                WasiImportRequest::ArgsGet(request) => {
                    io.send::<protocol::ArgsGetReqMsg>(&request).await?;
                    let completion = io.recv::<protocol::ArgsGetRetMsg>().await?;
                    pending.complete(WasiImportCompletion::ArgsGet(completion))?;
                }
                WasiImportRequest::EnvironSizesGet(request) => {
                    io.send::<protocol::EnvironSizesGetReqMsg>(&request).await?;
                    let completion = io.recv::<protocol::EnvironSizesGetRetMsg>().await?;
                    pending.complete(WasiImportCompletion::EnvironSizesGet(completion))?;
                }
                WasiImportRequest::EnvironGet(request) => {
                    io.send::<protocol::EnvironGetReqMsg>(&request).await?;
                    let completion = io.recv::<protocol::EnvironGetRetMsg>().await?;
                    pending.complete(WasiImportCompletion::EnvironGet(completion))?;
                }
                WasiImportRequest::FdFdstatGet(request) => {
                    io.send::<protocol::FdFdstatGetReqMsg>(&request).await?;
                    let completion = io.recv::<protocol::FdFdstatGetRetMsg>().await?;
                    pending.complete(WasiImportCompletion::FdFdstatGet(completion))?;
                }
                WasiImportRequest::PathFilestatGet(request) => {
                    io.send::<protocol::PathFilestatGetReqMsg>(&request).await?;
                    let completion = io.recv::<protocol::PathFilestatGetRetMsg>().await?;
                    pending.complete(WasiImportCompletion::PathFilestatGet(completion))?;
                }
                WasiImportRequest::FdClose(request) => {
                    io.send::<protocol::FdCloseReqMsg>(&request).await?;
                    let completion = io.recv::<protocol::FdCloseRetMsg>().await?;
                    pending.complete(WasiImportCompletion::FdClose(completion))?;
                }
                WasiImportRequest::ClockResGet(request) => {
                    io.send::<protocol::ClockResGetReqMsg>(&request).await?;
                    let completion = io.recv::<protocol::ClockResGetRetMsg>().await?;
                    pending.complete(WasiImportCompletion::ClockResGet(completion))?;
                }
                WasiImportRequest::ClockTimeGet(request) => {
                    io.send::<protocol::ClockTimeGetReqMsg>(&request).await?;
                    let completion = io.recv::<protocol::ClockTimeGetRetMsg>().await?;
                    pending.complete(WasiImportCompletion::ClockTimeGet(completion))?;
                }
                WasiImportRequest::PollOneoff(request) => {
                    io.send::<protocol::PollOneoffReqMsg>(&request).await?;
                    let completion = io.recv::<protocol::PollOneoffRetMsg>().await?;
                    pending.complete(WasiImportCompletion::PollOneoff(completion))?;
                }
                WasiImportRequest::RandomGet(request) => {
                    io.send::<protocol::RandomGetReqMsg>(&request).await?;
                    let completion = io.recv::<protocol::RandomGetRetMsg>().await?;
                    pending.complete(WasiImportCompletion::RandomGet(completion))?;
                }
            },
            WasiBoundaryStep::MemoryGrowPending(pending) => {
                let request = pending.request();
                io.send::<protocol::MemoryGrowReqMsg>(&request).await?;
                let decision = io.recv::<protocol::MemoryGrowRetMsg>().await?;
                pending.complete(decision)?;
            }
            WasiBoundaryStep::BudgetExpired(_) => {
                run_id = run_id.wrapping_add(1);
                tokio::task::yield_now().await;
            }
            WasiBoundaryStep::Exit(exit) => {
                let status = exit.status() as i32;
                if status == 0 {
                    io.send::<GuestExited>(&status).await?;
                } else {
                    io.send::<GuestFailed>(&status).await?;
                }
                control.send::<ReturnEnvironment<'_>>(io).await?;
                return Ok(status);
            }
        }
    }
}

// Shell environment role. `offer()` appears only at route boundaries; once an
// arm is selected, the rest of that arm is direct endpoint `recv` / `send`.
async fn run_shell_env<'r>(
    shell_env_endpoint: &mut Endpoint<'r, SHELL_ENV_ROLE>,
    io: Endpoint<'r, SHELL_IO_ROLE>,
    shell_env: &mut ShellEnv,
) -> DemoResult<()> {
    shell_env_endpoint.send::<Environment<'_>>(io).await?;
    loop {
        let branch = shell_env_endpoint.offer().await?;
        match branch.label() {
            202 => {
                if branch.recv::<GuestExited>().await? != 0 {
                    return Err(DemoError::message("successful exit has nonzero status"));
                }
                let _io = shell_env_endpoint.recv::<ReturnEnvironment<'_>>().await?;
                return Ok(());
            }
            203 => {
                let _status = branch.recv::<GuestFailed>().await?;
                let _io = shell_env_endpoint.recv::<ReturnEnvironment<'_>>().await?;
                return Ok(());
            }
            _ => {}
        }
        if branch.label() == protocol::LABEL_ENGINE_MEMORY_GROW {
            let observed = branch.recv::<protocol::MemoryGrowReqMsg>().await?;
            let decision = if observed.0.would_fit() {
                protocol::MemoryGrowDecision::grant()
            } else {
                protocol::MemoryGrowDecision::reject()
            };
            shell_env_endpoint
                .send::<protocol::MemoryGrowRetMsg>(&protocol::MemoryGrowRet(decision))
                .await?;
            continue;
        }
        match offered_import(branch.label())? {
            WasiImport::FdWrite => {
                let observed = branch.recv::<protocol::FdWriteReqMsg>().await?;
                let response = shell_env.write_fd(observed.0);
                shell_env_endpoint
                    .send::<protocol::FdWriteRetMsg>(&response)
                    .await?;
            }
            WasiImport::FdRead => {
                let observed = branch.recv::<protocol::FdReadReqMsg>().await?;
                let response = shell_env.read_fd(observed.0)?;
                shell_env_endpoint
                    .send::<protocol::FdReadRetMsg>(&response)
                    .await?;
            }
            WasiImport::FdFdstatGet => {
                let observed = branch.recv::<protocol::FdFdstatGetReqMsg>().await?;
                let response = shell_env.stat_fd(observed.0);
                shell_env_endpoint
                    .send::<protocol::FdFdstatGetRetMsg>(&response)
                    .await?;
                let observed = shell_env_endpoint
                    .recv::<protocol::PathOpenReqMsg>()
                    .await?;
                let response = shell_env.open_path(observed.0)?;
                shell_env_endpoint
                    .send::<protocol::PathOpenRetMsg>(&response)
                    .await?;
            }
            WasiImport::FdPrestatGet => {
                let observed = branch.recv::<protocol::FdPrestatGetReqMsg>().await?;
                shell_env_endpoint
                    .send::<protocol::FdPrestatGetRetMsg>(&shell_env.prestat_fd(observed.0))
                    .await?;
            }
            WasiImport::FdPrestatDirName => {
                let observed = branch.recv::<protocol::FdPrestatDirNameReqMsg>().await?;
                shell_env_endpoint
                    .send::<protocol::FdPrestatDirNameRetMsg>(
                        &shell_env.prestat_dir_name(observed.0)?,
                    )
                    .await?;
            }
            import => {
                return Err(DemoError::message(format!(
                    "route boundary reached unhandled import {import:?}"
                )));
            }
        }
    }
}

fn offered_import(label: u8) -> DemoResult<WasiImport> {
    WasiImport::from_label(label)
        .ok_or_else(|| DemoError::message(format!("route boundary reached non-WASI label {label}")))
}

fn run_guest_session<P>(module: &[u8], program: &P, shell_env: &mut ShellEnv) -> DemoResult<i32>
where
    P: Projectable,
{
    let shell_app_program: RoleProgram<SHELL_APP_ROLE> = project(program);
    let shell_env_program: RoleProgram<SHELL_ENV_ROLE> = project(program);

    let sid = SessionId::new(SESSION_ID);
    let io_program = project::<SHELL_IO_ROLE>(program);
    let transport = InProcessTransport::new(sid, SHELL_IO_ROLE, shell_env.binding.clone());
    let mut slab = vec![0u8; 256 * 1024];
    let mut kit_storage = SessionKitStorage::<InProcessTransport>::uninit();
    let kit = kit_storage.init();
    let rv = kit.rendezvous(&mut slab, transport.clone())?;
    let mut shell_app_endpoint = rv.enter(sid, &shell_app_program)?;
    let mut shell_env_endpoint = rv.enter(sid, &shell_env_program)?;
    let io = rv.enter(sid, &io_program)?;

    let mut guest_memory = [0u8; DEFAULT_GUEST_MEMORY_BYTES];
    let mut guest_storage = HibanaWasiGuestStorage::uninit();
    let guest = guest_storage.init(
        module,
        GuestMemory::new(&mut guest_memory),
        initial_bindings(),
    )?;

    let runtime = tokio::runtime::Builder::new_current_thread().build()?;
    let (status, ()) = runtime.block_on(async {
        tokio::try_join!(
            run_shell_app_engine(guest, &mut shell_app_endpoint),
            run_shell_env(&mut shell_env_endpoint, io, shell_env),
        )
    })?;
    if !transport.is_drained() {
        return Err(DemoError::message("unread frames after environment return"));
    }
    shell_env.flush_output();
    Ok(status)
}

fn print_output_state(shell_env: &ShellEnv) {
    if shell_env.led_green() {
        println!("Output: led.green = on");
    } else {
        println!("Output: led.green = unchanged");
    }
}

fn parse_host_args() -> DemoResult<PathBuf> {
    let mut args = env::args_os();
    let _program = args.next();
    let module_path = args.next().map(PathBuf::from).ok_or_else(|| {
        DemoError::message(
            "usage: cargo run --example direct_choreofs_write_rejection -- <guest.wasm>",
        )
    })?;
    if args.next().is_some() {
        return Err(DemoError::message(
            "usage: cargo run --example direct_choreofs_write_rejection -- <guest.wasm>",
        ));
    }
    Ok(module_path)
}
