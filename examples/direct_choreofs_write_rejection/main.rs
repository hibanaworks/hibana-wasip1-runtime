#![allow(long_running_const_eval)]

mod shell_env;
#[path = "../wasi_shell_demo_lib/lib.rs"]
mod wasi_shell_demo_lib;

use std::{env, fs, path::PathBuf, process};

use futures_executor::block_on;
use hibana::{
    g,
    runtime::{
        SessionKitStorage,
        ids::SessionId,
        program::{Projectable, RoleProgram, project},
    },
};
use hibana_wasip1_runtime::{
    DEFAULT_GUEST_MEMORY_BYTES, GuestMemory,
    exchange::{ExchangeError, HibanaStep, HibanaWasiGuest, HibanaWasiGuestStorage, WasiImport},
    protocol,
};

use crate::shell_env::{ShellEnv, initial_bindings};
use crate::wasi_shell_demo_lib::{
    error::{DemoError, DemoResult},
    transport::InProcessTransport,
};

const SHELL_APP_ROLE: u8 = 0;
const SHELL_ENV_ROLE: u8 = 1;
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
        Err(DemoError::Exchange(ExchangeError::Endpoint(error))) => {
            println!("Hibana: ChoreoFS write did not advance on this localside -> {error:?}");
            print_output_state(&shell_env);
            Ok(0)
        }
        result => result,
    }
}

// Global choreography. It contains ordinary read/write/open rows, but no
// refined ChoreoFS write row, so the direct output write cannot advance.
fn direct_choreofs_write_blocked_choreography() -> impl hibana::runtime::program::Projectable {
    let memory_grow = g::seq(
        g::send::<SHELL_APP_ROLE, SHELL_ENV_ROLE, protocol::MemoryGrowReqMsg>(),
        g::send::<SHELL_ENV_ROLE, SHELL_APP_ROLE, protocol::MemoryGrowRetMsg>(),
    );
    let fd_write = g::seq(
        g::send::<SHELL_APP_ROLE, SHELL_ENV_ROLE, protocol::FdWriteReqMsg>(),
        g::send::<SHELL_ENV_ROLE, SHELL_APP_ROLE, protocol::FdWriteRetMsg>(),
    );
    let fd_read = g::seq(
        g::send::<SHELL_APP_ROLE, SHELL_ENV_ROLE, protocol::FdReadReqMsg>(),
        g::send::<SHELL_ENV_ROLE, SHELL_APP_ROLE, protocol::FdReadRetMsg>(),
    );
    let fd_fdstat_get = g::seq(
        g::send::<SHELL_APP_ROLE, SHELL_ENV_ROLE, protocol::FdFdstatGetReqMsg>(),
        g::send::<SHELL_ENV_ROLE, SHELL_APP_ROLE, protocol::FdFdstatGetRetMsg>(),
    );
    let path_open = g::seq(
        g::send::<SHELL_APP_ROLE, SHELL_ENV_ROLE, protocol::PathOpenReqMsg>(),
        g::send::<SHELL_ENV_ROLE, SHELL_APP_ROLE, protocol::PathOpenRetMsg>(),
    );

    let open_selector_flow = g::seq(fd_fdstat_get, path_open);

    g::route(
        memory_grow,
        g::route(fd_write, g::route(fd_read, open_selector_flow)),
    )
    .roll()
}

// Shell app role. The runtime resumes the guest and completes each pending
// import through this role's endpoint.
async fn run_shell_app_engine(
    guest: &mut HibanaWasiGuest<'_>,
    shell_app_endpoint: &mut hibana::Endpoint<'_, SHELL_APP_ROLE>,
) -> DemoResult<i32> {
    let mut run_id = 1u16;
    loop {
        match guest
            .resume_hibana(
                shell_app_endpoint,
                protocol::BudgetRun::new(run_id, 0, 100_000),
            )
            .await?
        {
            HibanaStep::ImportPending(pending) => {
                pending.complete(guest, shell_app_endpoint).await?;
            }
            HibanaStep::MemoryGrowPending(pending) => {
                pending.complete(guest, shell_app_endpoint).await?;
            }
            HibanaStep::BudgetExpired(_) => {
                run_id = run_id.wrapping_add(1);
            }
            HibanaStep::Exit(exit) => {
                return Ok(exit.status() as i32);
            }
        }
    }
}

// Shell environment role. `offer()` appears only at route boundaries; once an
// arm is selected, the rest of that arm is direct endpoint `recv` / `send`.
async fn run_shell_env(
    shell_env_endpoint: &mut hibana::Endpoint<'_, SHELL_ENV_ROLE>,
    shell_env: &mut ShellEnv,
) -> DemoResult<()> {
    loop {
        let branch = shell_env_endpoint.offer().await?;
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
                continue_open_selector_flow(shell_env_endpoint, shell_env).await?;
            }
            import => {
                return Err(DemoError::message(format!(
                    "route boundary reached unhandled import {import:?}"
                )));
            }
        }
    }
}

async fn continue_open_selector_flow(
    shell_env_endpoint: &mut hibana::Endpoint<'_, SHELL_ENV_ROLE>,
    shell_env: &mut ShellEnv,
) -> DemoResult<()> {
    recv_path_open_req_send_ret(shell_env_endpoint, shell_env).await
}

async fn recv_path_open_req_send_ret(
    shell_env_endpoint: &mut hibana::Endpoint<'_, SHELL_ENV_ROLE>,
    shell_env: &mut ShellEnv,
) -> DemoResult<()> {
    let observed = shell_env_endpoint
        .recv::<protocol::PathOpenReqMsg>()
        .await?;
    let response = shell_env.open_path(observed.0)?;
    shell_env_endpoint
        .send::<protocol::PathOpenRetMsg>(&response)
        .await?;
    Ok(())
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

    let transport = InProcessTransport::new();
    let mut slab = vec![0u8; 256 * 1024];
    let mut kit_storage = SessionKitStorage::<InProcessTransport>::uninit();
    let kit = kit_storage.init();
    let rv = kit.rendezvous(&mut slab, transport.clone())?;
    let sid = SessionId::new(SESSION_ID);
    let mut shell_app_endpoint = rv.enter(sid, &shell_app_program)?;
    let mut shell_env_endpoint = rv.enter(sid, &shell_env_program)?;

    let mut guest_memory = [0u8; DEFAULT_GUEST_MEMORY_BYTES];
    let mut guest_storage = HibanaWasiGuestStorage::uninit();
    let guest = guest_storage.init(
        module,
        GuestMemory::new(&mut guest_memory),
        initial_bindings(),
    )?;

    let status = block_on(run_until_guest_exit(
        run_shell_app_engine(guest, &mut shell_app_endpoint),
        run_shell_env(&mut shell_env_endpoint, shell_env),
    ))?;
    shell_env.flush_output();
    Ok(status)
}

async fn run_until_guest_exit<ShellAppFuture, ShellEnvFuture>(
    shell_app: ShellAppFuture,
    shell_env: ShellEnvFuture,
) -> DemoResult<i32>
where
    ShellAppFuture: core::future::Future<Output = DemoResult<i32>>,
    ShellEnvFuture: core::future::Future<Output = DemoResult<()>>,
{
    futures_util::pin_mut!(shell_app);
    futures_util::pin_mut!(shell_env);
    match futures_util::future::select(shell_app, shell_env).await {
        futures_util::future::Either::Left((result, _shell_env)) => result,
        futures_util::future::Either::Right((result, _shell_app)) => {
            result?;
            Err(DemoError::message(
                "shell environment role ended before guest exit",
            ))
        }
    }
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
