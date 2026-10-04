//! Link/storage fixture, without RP2350 boot metadata or board startup.
#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]
#![deny(unsafe_op_in_unsafe_fn)]

use core::mem::size_of;
use hibana_wasip1_runtime::{
    DEFAULT_GUEST_MEMORY_BYTES, FdBindingTable, GuestMemory, HibanaWasiGuestStorage,
    WasiBoundaryStep, WasiImportCompletion, WasiImportPending, WasiImportRequest,
    protocol::{BudgetRun, PollReady, PollReadyRet},
};

static mut MEMORY: [u8; DEFAULT_GUEST_MEMORY_BYTES] = [0; DEFAULT_GUEST_MEMORY_BYTES];
static mut STORAGE: HibanaWasiGuestStorage<'static> = HibanaWasiGuestStorage::uninit();
const MODULE: &[u8] = include_bytes!(env!("WASI_RESOURCE_GUEST"));

#[cfg_attr(target_os = "none", unsafe(link_section = ".runtime_budget"))]
#[used]
static SIZES: [u32; 7] = [
    DEFAULT_GUEST_MEMORY_BYTES as u32,
    size_of::<HibanaWasiGuestStorage<'static>>() as u32,
    size_of::<FdBindingTable>() as u32,
    size_of::<WasiImportRequest<'static>>() as u32,
    size_of::<WasiImportCompletion<'static>>() as u32,
    size_of::<WasiImportPending<'static, 'static>>() as u32,
    size_of::<WasiBoundaryStep<'static, 'static>>() as u32,
];

const _: () = {
    assert!(DEFAULT_GUEST_MEMORY_BYTES + size_of::<HibanaWasiGuestStorage<'static>>() <= 96 * 1024);
    assert!(size_of::<WasiImportRequest<'static>>() <= 112);
    assert!(size_of::<WasiImportCompletion<'static>>() <= 112);
    assert!(size_of::<WasiImportPending<'static, 'static>>() <= 80);
    assert!(size_of::<WasiBoundaryStep<'static, 'static>>() <= 88);
};

fn run() {
    // SAFETY: each executable calls run exactly once, on one thread. There are
    // no other accesses to either static, and both borrows end with this run.
    let (memory, storage) = unsafe {
        (
            &mut *core::ptr::addr_of_mut!(MEMORY),
            &mut *core::ptr::addr_of_mut!(STORAGE),
        )
    };
    let guest = storage
        .init(MODULE, GuestMemory::new(memory), FdBindingTable::empty())
        .expect("initialize bounded guest");
    let mut polls = 0;
    loop {
        match guest
            .resume_wasi_boundary(BudgetRun::new(1, 1, 128))
            .expect("bounded guest execution")
        {
            WasiBoundaryStep::ImportPending(pending) => {
                let WasiImportRequest::PollOneoff(request) = pending.request().expect("poll view")
                else {
                    panic!("resource guest requested an unexpected import");
                };
                let subscriptions = request.0.subscriptions();
                assert_eq!(subscriptions.len(), 16);
                let mut events = [[0u8; 32]; 16];
                for (event, subscription) in events.iter_mut().zip(subscriptions) {
                    event[..8].copy_from_slice(&subscription[..8]);
                    event[10] = subscription[8];
                }
                // The fixture simulates readiness. It measures the ordinary
                // runtime path; it is not a driver or a source of authority.
                pending
                    .complete(WasiImportCompletion::PollOneoff(PollReadyRet(
                        PollReady::new(&events).expect("valid ready events"),
                    )))
                    .expect("atomic poll completion");
                polls += 1;
            }
            WasiBoundaryStep::BudgetExpired(_) => {}
            WasiBoundaryStep::MemoryGrowPending(_) => panic!("resource guest cannot grow memory"),
            WasiBoundaryStep::Exit(exit) => {
                assert_eq!(exit.status(), 0);
                assert_eq!(polls, 1);
                break;
            }
        }
    }
}

#[cfg(not(target_os = "none"))]
fn main() {
    run();
    println!("16-subscription guest passed; host sizes: {SIZES:?}");
}

#[cfg(target_os = "none")]
#[unsafe(no_mangle)]
fn runtime_resource_entry() -> ! {
    run();
    loop {
        core::hint::spin_loop();
    }
}

#[cfg(target_os = "none")]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
