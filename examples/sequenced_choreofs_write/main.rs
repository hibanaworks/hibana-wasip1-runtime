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
    exchange::{
        HibanaWasiGuest, HibanaWasiGuestStorage, WasiBoundaryStep, WasiImport,
        WasiImportCompletion, WasiImportPending, WasiImportRequest,
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
const SESSION_ID: u32 = 0x5750_0001;

fn main() {
    match run() {
        Ok(status) => process::exit(status),
        Err(error) => {
            eprintln!("sequenced-choreofs-write failed: {error}");
            process::exit(1);
        }
    }
}

fn run() -> DemoResult<i32> {
    let module_path = parse_host_args()?;
    let module = fs::read(&module_path)?;

    println!("choreography: sequenced ChoreoFS write");
    let program = sequenced_choreofs_write_choreography();
    let mut shell_env = ShellEnv::new();
    let status = run_guest_session(&module, &program, &mut shell_env)?;
    if shell_env.led_green() {
        println!("Output: led.green = on");
    }
    Ok(status)
}

// Global choreography defines progress by composing ordinary Hibana
// request/return rows. ShellEnv only materializes local object effects for
// messages admitted by this program.
fn sequenced_choreofs_write_choreography() -> impl hibana::runtime::program::Projectable {
    let memory_grow = || {
        g::seq(
            g::send::<SHELL_APP_ROLE, SHELL_ENV_ROLE, protocol::MemoryGrowReqMsg>(),
            g::send::<SHELL_ENV_ROLE, SHELL_APP_ROLE, protocol::MemoryGrowRetMsg>(),
        )
    };
    let fd_write = || {
        g::seq(
            g::send::<SHELL_APP_ROLE, SHELL_ENV_ROLE, protocol::FdWriteReqMsg>(),
            g::send::<SHELL_ENV_ROLE, SHELL_APP_ROLE, protocol::FdWriteRetMsg>(),
        )
    };
    let fd_write_object = || {
        g::seq(
            g::send::<SHELL_APP_ROLE, SHELL_ENV_ROLE, protocol::FdWriteObjectReqMsg>(),
            g::send::<SHELL_ENV_ROLE, SHELL_APP_ROLE, protocol::FdWriteObjectRetMsg>(),
        )
    };
    let fd_read = || {
        g::seq(
            g::send::<SHELL_APP_ROLE, SHELL_ENV_ROLE, protocol::FdReadReqMsg>(),
            g::send::<SHELL_ENV_ROLE, SHELL_APP_ROLE, protocol::FdReadRetMsg>(),
        )
    };
    let fd_readdir = || {
        g::seq(
            g::send::<SHELL_APP_ROLE, SHELL_ENV_ROLE, protocol::FdReaddirReqMsg>(),
            g::send::<SHELL_ENV_ROLE, SHELL_APP_ROLE, protocol::FdReaddirRetMsg>(),
        )
    };
    let fd_fdstat_get = || {
        g::seq(
            g::send::<SHELL_APP_ROLE, SHELL_ENV_ROLE, protocol::FdFdstatGetReqMsg>(),
            g::send::<SHELL_ENV_ROLE, SHELL_APP_ROLE, protocol::FdFdstatGetRetMsg>(),
        )
    };
    let fd_prestat_get = || {
        g::seq(
            g::send::<SHELL_APP_ROLE, SHELL_ENV_ROLE, protocol::FdPrestatGetReqMsg>(),
            g::send::<SHELL_ENV_ROLE, SHELL_APP_ROLE, protocol::FdPrestatGetRetMsg>(),
        )
    };
    let fd_prestat_dir_name = || {
        g::seq(
            g::send::<SHELL_APP_ROLE, SHELL_ENV_ROLE, protocol::FdPrestatDirNameReqMsg>(),
            g::send::<SHELL_ENV_ROLE, SHELL_APP_ROLE, protocol::FdPrestatDirNameRetMsg>(),
        )
    };
    let path_filestat_get = || {
        g::seq(
            g::send::<SHELL_APP_ROLE, SHELL_ENV_ROLE, protocol::PathFilestatGetReqMsg>(),
            g::send::<SHELL_ENV_ROLE, SHELL_APP_ROLE, protocol::PathFilestatGetRetMsg>(),
        )
    };
    let path_open = || {
        g::seq(
            g::send::<SHELL_APP_ROLE, SHELL_ENV_ROLE, protocol::PathOpenReqMsg>(),
            g::send::<SHELL_ENV_ROLE, SHELL_APP_ROLE, protocol::PathOpenRetMsg>(),
        )
    };
    let fd_filestat_get = || {
        g::seq(
            g::send::<SHELL_APP_ROLE, SHELL_ENV_ROLE, protocol::FdFilestatGetReqMsg>(),
            g::send::<SHELL_ENV_ROLE, SHELL_APP_ROLE, protocol::FdFilestatGetRetMsg>(),
        )
    };
    let fd_close = || {
        g::seq(
            g::send::<SHELL_APP_ROLE, SHELL_ENV_ROLE, protocol::FdCloseReqMsg>(),
            g::send::<SHELL_ENV_ROLE, SHELL_APP_ROLE, protocol::FdCloseRetMsg>(),
        )
    };

    let open_selector_flow = g::seq(fd_fdstat_get(), path_open());
    let object_log_read_flow = g::seq(
        path_filestat_get(),
        g::seq(
            fd_fdstat_get(),
            g::seq(
                path_open(),
                g::seq(
                    fd_filestat_get(),
                    g::seq(fd_read(), g::seq(fd_read(), fd_close())),
                ),
            ),
        ),
    );
    let output_write_flow = g::seq(
        fd_fdstat_get(),
        g::seq(
            path_open(),
            g::seq(
                fd_write_object(),
                g::seq(fd_write_object(), g::seq(fd_write(), fd_close())),
            ),
        ),
    );
    let sequenced_write_flow = g::seq(object_log_read_flow, output_write_flow);

    g::route(
        memory_grow(),
        g::route(
            fd_write(),
            g::route(
                fd_read(),
                g::route(
                    fd_readdir(),
                    g::route(
                        fd_close(),
                        g::route(
                            fd_filestat_get(),
                            g::route(
                                fd_prestat_get(),
                                g::route(
                                    fd_prestat_dir_name(),
                                    g::route(open_selector_flow, sequenced_write_flow),
                                ),
                            ),
                        ),
                    ),
                ),
            ),
        ),
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
        match guest.resume_wasi_boundary(protocol::BudgetRun::new(run_id, 0, 100_000))? {
            WasiBoundaryStep::ImportPending(pending) => {
                complete_pending_import(guest, shell_app_endpoint, pending).await?;
            }
            WasiBoundaryStep::MemoryGrowPending(pending) => {
                let request = pending.request();
                shell_app_endpoint
                    .send::<protocol::MemoryGrowReqMsg>(&request)
                    .await?;
                let decision = shell_app_endpoint
                    .recv::<protocol::MemoryGrowRetMsg>()
                    .await?;
                pending.complete(guest, decision)?;
            }
            WasiBoundaryStep::BudgetExpired(_) => {
                run_id = run_id.wrapping_add(1);
            }
            WasiBoundaryStep::Exit(exit) => {
                return Ok(exit.status() as i32);
            }
        }
    }
}

async fn complete_pending_import(
    guest: &mut HibanaWasiGuest<'_>,
    shell_app_endpoint: &mut hibana::Endpoint<'_, SHELL_APP_ROLE>,
    pending: WasiImportPending,
) -> DemoResult<()> {
    match pending.request() {
        WasiImportRequest::FdWrite(request) => {
            shell_app_endpoint
                .send::<protocol::FdWriteReqMsg>(&request)
                .await?;
            let completion = shell_app_endpoint.recv::<protocol::FdWriteRetMsg>().await?;
            pending.complete(guest, WasiImportCompletion::FdWrite(completion))?;
        }
        WasiImportRequest::FdWriteObject(request) => {
            shell_app_endpoint
                .send::<protocol::FdWriteObjectReqMsg>(&request)
                .await?;
            let completion = shell_app_endpoint
                .recv::<protocol::FdWriteObjectRetMsg>()
                .await?;
            pending.complete(guest, WasiImportCompletion::FdWriteObject(completion))?;
        }
        WasiImportRequest::FdRead(request) => {
            shell_app_endpoint
                .send::<protocol::FdReadReqMsg>(&request)
                .await?;
            let completion = shell_app_endpoint.recv::<protocol::FdReadRetMsg>().await?;
            pending.complete(guest, WasiImportCompletion::FdRead(completion))?;
        }
        WasiImportRequest::FdReaddir(request) => {
            shell_app_endpoint
                .send::<protocol::FdReaddirReqMsg>(&request)
                .await?;
            let completion = shell_app_endpoint
                .recv::<protocol::FdReaddirRetMsg>()
                .await?;
            pending.complete(guest, WasiImportCompletion::FdReaddir(completion))?;
        }
        WasiImportRequest::PathOpen(request) => {
            shell_app_endpoint
                .send::<protocol::PathOpenReqMsg>(&request)
                .await?;
            let completion = shell_app_endpoint
                .recv::<protocol::PathOpenRetMsg>()
                .await?;
            pending.complete(guest, WasiImportCompletion::PathOpen(completion))?;
        }
        WasiImportRequest::FdPrestatGet(request) => {
            shell_app_endpoint
                .send::<protocol::FdPrestatGetReqMsg>(&request)
                .await?;
            let completion = shell_app_endpoint
                .recv::<protocol::FdPrestatGetRetMsg>()
                .await?;
            pending.complete(guest, WasiImportCompletion::FdPrestatGet(completion))?;
        }
        WasiImportRequest::FdPrestatDirName(request) => {
            shell_app_endpoint
                .send::<protocol::FdPrestatDirNameReqMsg>(&request)
                .await?;
            let completion = shell_app_endpoint
                .recv::<protocol::FdPrestatDirNameRetMsg>()
                .await?;
            pending.complete(guest, WasiImportCompletion::FdPrestatDirName(completion))?;
        }
        WasiImportRequest::FdFilestatGet(request) => {
            shell_app_endpoint
                .send::<protocol::FdFilestatGetReqMsg>(&request)
                .await?;
            let completion = shell_app_endpoint
                .recv::<protocol::FdFilestatGetRetMsg>()
                .await?;
            pending.complete(guest, WasiImportCompletion::FdFilestatGet(completion))?;
        }
        WasiImportRequest::ArgsSizesGet(request) => {
            shell_app_endpoint
                .send::<protocol::ArgsSizesGetReqMsg>(&request)
                .await?;
            let completion = shell_app_endpoint
                .recv::<protocol::ArgsSizesGetRetMsg>()
                .await?;
            pending.complete(guest, WasiImportCompletion::ArgsSizesGet(completion))?;
        }
        WasiImportRequest::ArgsGet(request) => {
            shell_app_endpoint
                .send::<protocol::ArgsGetReqMsg>(&request)
                .await?;
            let completion = shell_app_endpoint.recv::<protocol::ArgsGetRetMsg>().await?;
            pending.complete(guest, WasiImportCompletion::ArgsGet(completion))?;
        }
        WasiImportRequest::EnvironSizesGet(request) => {
            shell_app_endpoint
                .send::<protocol::EnvironSizesGetReqMsg>(&request)
                .await?;
            let completion = shell_app_endpoint
                .recv::<protocol::EnvironSizesGetRetMsg>()
                .await?;
            pending.complete(guest, WasiImportCompletion::EnvironSizesGet(completion))?;
        }
        WasiImportRequest::EnvironGet(request) => {
            shell_app_endpoint
                .send::<protocol::EnvironGetReqMsg>(&request)
                .await?;
            let completion = shell_app_endpoint
                .recv::<protocol::EnvironGetRetMsg>()
                .await?;
            pending.complete(guest, WasiImportCompletion::EnvironGet(completion))?;
        }
        WasiImportRequest::FdFdstatGet(request) => {
            shell_app_endpoint
                .send::<protocol::FdFdstatGetReqMsg>(&request)
                .await?;
            let completion = shell_app_endpoint
                .recv::<protocol::FdFdstatGetRetMsg>()
                .await?;
            pending.complete(guest, WasiImportCompletion::FdFdstatGet(completion))?;
        }
        WasiImportRequest::PathFilestatGet(request) => {
            shell_app_endpoint
                .send::<protocol::PathFilestatGetReqMsg>(&request)
                .await?;
            let completion = shell_app_endpoint
                .recv::<protocol::PathFilestatGetRetMsg>()
                .await?;
            pending.complete(guest, WasiImportCompletion::PathFilestatGet(completion))?;
        }
        WasiImportRequest::FdClose(request) => {
            shell_app_endpoint
                .send::<protocol::FdCloseReqMsg>(&request)
                .await?;
            let completion = shell_app_endpoint.recv::<protocol::FdCloseRetMsg>().await?;
            pending.complete(guest, WasiImportCompletion::FdClose(completion))?;
        }
        WasiImportRequest::ClockResGet(request) => {
            shell_app_endpoint
                .send::<protocol::ClockResGetReqMsg>(&request)
                .await?;
            let completion = shell_app_endpoint
                .recv::<protocol::ClockResGetRetMsg>()
                .await?;
            pending.complete(guest, WasiImportCompletion::ClockResGet(completion))?;
        }
        WasiImportRequest::ClockTimeGet(request) => {
            shell_app_endpoint
                .send::<protocol::ClockTimeGetReqMsg>(&request)
                .await?;
            let completion = shell_app_endpoint
                .recv::<protocol::ClockTimeGetRetMsg>()
                .await?;
            pending.complete(guest, WasiImportCompletion::ClockTimeGet(completion))?;
        }
        WasiImportRequest::PollOneoff(request) => {
            shell_app_endpoint
                .send::<protocol::PollOneoffReqMsg>(&request)
                .await?;
            let completion = shell_app_endpoint
                .recv::<protocol::PollOneoffRetMsg>()
                .await?;
            pending.complete(guest, WasiImportCompletion::PollOneoff(completion))?;
        }
        WasiImportRequest::RandomGet(request) => {
            shell_app_endpoint
                .send::<protocol::RandomGetReqMsg>(&request)
                .await?;
            let completion = shell_app_endpoint
                .recv::<protocol::RandomGetRetMsg>()
                .await?;
            pending.complete(guest, WasiImportCompletion::RandomGet(completion))?;
        }
    }
    Ok(())
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
            WasiImport::FdReaddir => {
                let observed = branch.recv::<protocol::FdReaddirReqMsg>().await?;
                let response = shell_env.read_dir_fd(observed.0)?;
                shell_env_endpoint
                    .send::<protocol::FdReaddirRetMsg>(&response)
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
            WasiImport::FdFilestatGet => {
                let observed = branch.recv::<protocol::FdFilestatGetReqMsg>().await?;
                let response = shell_env.file_stat_fd(observed.0);
                shell_env_endpoint
                    .send::<protocol::FdFilestatGetRetMsg>(&response)
                    .await?;
            }
            WasiImport::FdClose => {
                let observed = branch.recv::<protocol::FdCloseReqMsg>().await?;
                let response = shell_env.close_fd(observed.0);
                shell_env_endpoint
                    .send::<protocol::FdCloseRetMsg>(&response)
                    .await?;
            }
            WasiImport::FdPrestatGet => {
                let observed = branch.recv::<protocol::FdPrestatGetReqMsg>().await?;
                let response = shell_env.prestat_fd(observed.0);
                shell_env_endpoint
                    .send::<protocol::FdPrestatGetRetMsg>(&response)
                    .await?;
            }
            WasiImport::FdPrestatDirName => {
                let observed = branch.recv::<protocol::FdPrestatDirNameReqMsg>().await?;
                let response = shell_env.prestat_dir_name(observed.0)?;
                shell_env_endpoint
                    .send::<protocol::FdPrestatDirNameRetMsg>(&response)
                    .await?;
            }
            WasiImport::PathFilestatGet => {
                let observed = branch.recv::<protocol::PathFilestatGetReqMsg>().await?;
                let response = shell_env.file_stat_path(observed.0);
                shell_env_endpoint
                    .send::<protocol::PathFilestatGetRetMsg>(&response)
                    .await?;
                continue_sequenced_write_flow(shell_env_endpoint, shell_env).await?;
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

async fn continue_sequenced_write_flow(
    shell_env_endpoint: &mut hibana::Endpoint<'_, SHELL_ENV_ROLE>,
    shell_env: &mut ShellEnv,
) -> DemoResult<()> {
    recv_fd_fdstat_get_req_send_ret(shell_env_endpoint, shell_env).await?;
    recv_path_open_req_send_ret(shell_env_endpoint, shell_env).await?;
    recv_fd_filestat_get_req_send_ret(shell_env_endpoint, shell_env).await?;
    recv_fd_read_req_send_ret(shell_env_endpoint, shell_env).await?;
    recv_fd_read_req_send_ret(shell_env_endpoint, shell_env).await?;
    recv_fd_close_req_send_ret(shell_env_endpoint, shell_env).await?;

    recv_fd_fdstat_get_req_send_ret(shell_env_endpoint, shell_env).await?;
    recv_path_open_req_send_ret(shell_env_endpoint, shell_env).await?;
    recv_fd_write_object_req_send_ret(shell_env_endpoint, shell_env).await?;
    recv_fd_write_object_req_send_ret(shell_env_endpoint, shell_env).await?;
    recv_fd_write_req_send_ret(shell_env_endpoint, shell_env).await?;
    recv_fd_close_req_send_ret(shell_env_endpoint, shell_env).await
}

async fn recv_fd_write_object_req_send_ret(
    shell_env_endpoint: &mut hibana::Endpoint<'_, SHELL_ENV_ROLE>,
    shell_env: &mut ShellEnv,
) -> DemoResult<()> {
    let observed = shell_env_endpoint
        .recv::<protocol::FdWriteObjectReqMsg>()
        .await?;
    let response = shell_env.write_fd(observed.0);
    shell_env_endpoint
        .send::<protocol::FdWriteObjectRetMsg>(&response)
        .await?;
    Ok(())
}

async fn recv_fd_write_req_send_ret(
    shell_env_endpoint: &mut hibana::Endpoint<'_, SHELL_ENV_ROLE>,
    shell_env: &mut ShellEnv,
) -> DemoResult<()> {
    let observed = shell_env_endpoint.recv::<protocol::FdWriteReqMsg>().await?;
    let response = shell_env.write_fd(observed.0);
    shell_env_endpoint
        .send::<protocol::FdWriteRetMsg>(&response)
        .await?;
    Ok(())
}

async fn recv_fd_read_req_send_ret(
    shell_env_endpoint: &mut hibana::Endpoint<'_, SHELL_ENV_ROLE>,
    shell_env: &mut ShellEnv,
) -> DemoResult<()> {
    let observed = shell_env_endpoint.recv::<protocol::FdReadReqMsg>().await?;
    let response = shell_env.read_fd(observed.0)?;
    shell_env_endpoint
        .send::<protocol::FdReadRetMsg>(&response)
        .await?;
    Ok(())
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

async fn recv_fd_filestat_get_req_send_ret(
    shell_env_endpoint: &mut hibana::Endpoint<'_, SHELL_ENV_ROLE>,
    shell_env: &mut ShellEnv,
) -> DemoResult<()> {
    let observed = shell_env_endpoint
        .recv::<protocol::FdFilestatGetReqMsg>()
        .await?;
    let response = shell_env.file_stat_fd(observed.0);
    shell_env_endpoint
        .send::<protocol::FdFilestatGetRetMsg>(&response)
        .await?;
    Ok(())
}

async fn recv_fd_fdstat_get_req_send_ret(
    shell_env_endpoint: &mut hibana::Endpoint<'_, SHELL_ENV_ROLE>,
    shell_env: &mut ShellEnv,
) -> DemoResult<()> {
    let observed = shell_env_endpoint
        .recv::<protocol::FdFdstatGetReqMsg>()
        .await?;
    let response = shell_env.stat_fd(observed.0);
    shell_env_endpoint
        .send::<protocol::FdFdstatGetRetMsg>(&response)
        .await?;
    Ok(())
}

async fn recv_fd_close_req_send_ret(
    shell_env_endpoint: &mut hibana::Endpoint<'_, SHELL_ENV_ROLE>,
    shell_env: &mut ShellEnv,
) -> DemoResult<()> {
    let observed = shell_env_endpoint.recv::<protocol::FdCloseReqMsg>().await?;
    let response = shell_env.close_fd(observed.0);
    shell_env_endpoint
        .send::<protocol::FdCloseRetMsg>(&response)
        .await?;
    Ok(())
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

fn offered_import(label: u8) -> DemoResult<WasiImport> {
    WasiImport::from_label(label)
        .ok_or_else(|| DemoError::message(format!("route boundary reached non-WASI label {label}")))
}

fn parse_host_args() -> DemoResult<PathBuf> {
    let mut args = env::args_os();
    let _program = args.next();
    let module_path = args.next().map(PathBuf::from).ok_or_else(|| {
        DemoError::message("usage: cargo run --example sequenced_choreofs_write -- <guest.wasm>")
    })?;
    if args.next().is_some() {
        return Err(DemoError::message(
            "usage: cargo run --example sequenced_choreofs_write -- <guest.wasm>",
        ));
    }
    Ok(module_path)
}
