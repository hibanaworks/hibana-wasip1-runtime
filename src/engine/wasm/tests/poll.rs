use super::{
    TestVm, TestWasmArg, VALTYPE_I32, VmEvent, core_wasip1_single_import_module, test_budget,
};
use crate::wasip1::Wasip1ImportName;

fn module(input: u32, output: u32, count: u32, nevents: u32) -> std::vec::Vec<u8> {
    core_wasip1_single_import_module(
        Wasip1ImportName::PollOneoff,
        &[VALTYPE_I32; 4],
        &[VALTYPE_I32],
        &[
            TestWasmArg::I32(input),
            TestWasmArg::I32(output),
            TestWasmArg::I32(count),
            TestWasmArg::I32(nevents),
        ],
        true,
    )
}

fn subscription(userdata: u64, kind: u8) -> [u8; 48] {
    let mut record = [0; 48];
    record[..8].copy_from_slice(&userdata.to_le_bytes());
    record[8] = kind;
    record[16..20].copy_from_slice(&1u32.to_le_bytes());
    if kind == 0 {
        record[24..32].copy_from_slice(&20u64.to_le_bytes());
        record[32..40].copy_from_slice(&3u64.to_le_bytes());
        record[40..42].copy_from_slice(&1u16.to_le_bytes());
    }
    record
}

fn event(userdata: u64, kind: u8) -> [u8; 32] {
    let mut record = [0; 32];
    record[..8].copy_from_slice(&userdata.to_le_bytes());
    record[10] = kind;
    record
}

#[test]
fn mixed_fd_and_absolute_clock_events_preserve_all_records() {
    for (output, nevents) in [(0, 512), (512, 0)] {
        let module = module(64, output, 2, nevents);
        let mut guest = TestVm::new(&module).unwrap();
        let VmEvent::PollOneoff(call) = guest.resume(test_budget()).unwrap() else {
            panic!("poll expected");
        };
        let input = [subscription(11, 1), subscription(22, 0)];
        guest.write_memory(64, input.as_flattened()).unwrap();
        assert_eq!(
            guest.poll_oneoff_request(call).unwrap().subscriptions(),
            input
        );
        let ready = [event(11, 1), event(22, 0)];
        guest.finish_poll_oneoff(call, &ready, 0).unwrap();
        let mut actual = [0; 64];
        guest.read_memory(output, &mut actual).unwrap();
        assert_eq!(actual, ready.as_flattened());
        assert_eq!(guest.core.read_memory_u32(nevents).unwrap(), 2);
    }
}

#[test]
fn malformed_last_subscription_preserves_all_outputs() {
    let module = module(64, 512, 2, 0);
    let mut guest = TestVm::new(&module).unwrap();
    let VmEvent::PollOneoff(call) = guest.resume(test_budget()).unwrap() else {
        panic!("poll expected");
    };
    let input = [subscription(11, 1), subscription(22, 3)];
    guest.write_memory(64, input.as_flattened()).unwrap();
    guest.write_memory(512, &[0xa5; 64]).unwrap();
    guest.write_memory(0, &[0xa5; 4]).unwrap();
    assert!(guest.poll_oneoff_request(call).is_err());
    assert!(guest.finish_poll_oneoff(call, &[event(11, 1)], 0).is_err());
    let mut actual = [0; 64];
    guest.read_memory(512, &mut actual).unwrap();
    assert_eq!(actual, [0xa5; 64]);
    assert_eq!(guest.core.read_memory_u32(0).unwrap(), 0xa5a5a5a5);
}

#[test]
fn invalid_tail_output_or_count_never_writes_a_prefix() {
    for (input, output, nevents) in [
        (64, 65504, 0),
        (64, 512, 65534),
        (65504, 512, 0),
        (64, 512, 544),
    ] {
        let module = module(input, output, 2, nevents);
        let mut guest = TestVm::new(&module).unwrap();
        let VmEvent::PollOneoff(call) = guest.resume(test_budget()).unwrap() else {
            panic!("poll expected");
        };
        if input == 64 {
            guest
                .write_memory(
                    input,
                    [subscription(11, 1), subscription(22, 0)].as_flattened(),
                )
                .unwrap();
        }
        guest.write_memory(output, &[0xa5; 32]).unwrap();
        assert!(guest.poll_oneoff_request(call).is_err());
        assert!(guest.finish_poll_oneoff(call, &[event(11, 1)], 0).is_err());
        let mut actual = [0; 32];
        guest.read_memory(output, &mut actual).unwrap();
        assert_eq!(actual, [0xa5; 32]);
    }
}

#[test]
fn completion_rejects_extra_reordered_and_mismatched_events_atomically() {
    for ready in [
        std::vec![event(11, 1), event(11, 1)],
        std::vec![event(22, 0), event(11, 1)],
        std::vec![event(11, 2)],
        std::vec![event(33, 1)],
        std::vec![],
    ] {
        let module = module(64, 512, 2, 0);
        let mut guest = TestVm::new(&module).unwrap();
        let VmEvent::PollOneoff(call) = guest.resume(test_budget()).unwrap() else {
            panic!("poll expected");
        };
        guest
            .write_memory(
                64,
                [subscription(11, 1), subscription(22, 0)].as_flattened(),
            )
            .unwrap();
        guest.write_memory(512, &[0xa5; 64]).unwrap();
        guest.write_memory(0, &[0xa5; 4]).unwrap();
        assert!(guest.finish_poll_oneoff(call, &ready, 0).is_err());
        let mut actual = [0; 64];
        guest.read_memory(512, &mut actual).unwrap();
        assert_eq!(actual, [0xa5; 64]);
        assert_eq!(guest.core.read_memory_u32(0).unwrap(), 0xa5a5a5a5);
    }
}

#[test]
fn duplicate_userdata_consumes_distinct_subscription_occurrences() {
    let module = module(64, 512, 2, 0);
    let mut guest = TestVm::new(&module).unwrap();
    let VmEvent::PollOneoff(call) = guest.resume(test_budget()).unwrap() else {
        panic!("poll expected");
    };
    guest
        .write_memory(
            64,
            [subscription(11, 1), subscription(11, 1)].as_flattened(),
        )
        .unwrap();
    guest
        .finish_poll_oneoff(call, &[event(11, 1), event(11, 1)], 0)
        .unwrap();
    assert_eq!(guest.core.read_memory_u32(0).unwrap(), 2);
}

#[test]
fn input_output_alias_uses_the_complete_snapshot_before_writing() {
    let module = module(64, 64, 2, 256);
    let mut guest = TestVm::new(&module).unwrap();
    let VmEvent::PollOneoff(call) = guest.resume(test_budget()).unwrap() else {
        panic!("poll expected");
    };
    guest
        .write_memory(
            64,
            [subscription(11, 1), subscription(22, 0)].as_flattened(),
        )
        .unwrap();
    let ready = [event(11, 1), event(22, 0)];
    guest.finish_poll_oneoff(call, &ready, 0).unwrap();
    let mut actual = [0; 64];
    guest.read_memory(64, &mut actual).unwrap();
    assert_eq!(actual, ready.as_flattened());
}

fn key_lists() -> std::vec::Vec<std::vec::Vec<(u64, u8)>> {
    let keys = [(0, 0), (u64::MAX, 1), (7, 2)];
    let mut lists = std::vec![std::vec![]];
    let mut level = std::vec![std::vec![]];
    for _ in 0..3 {
        let mut next = std::vec![];
        for prefix in &level {
            for key in keys {
                let mut list = prefix.clone();
                list.push(key);
                next.push(list);
            }
        }
        lists.extend(next.iter().cloned());
        level = next;
    }
    lists
}

#[test]
#[ignore = "exports actual VM decisions for the Lean correspondence gate"]
fn export_actual_poll_decisions() {
    use std::fmt::Write;
    let output = std::env::var("HIBANA_WASIP1_POLL_LEAN_EXPORT").expect("export destination");
    let mut lean = std::string::String::from("import PollWriteback\nnamespace WasiPoll\n");
    let lists = key_lists();
    let mut index = 0;
    for requests in lists.iter().filter(|list| list.len() <= 2) {
        for events in &lists {
            let module = module(64, 512, requests.len() as u32, 0);
            let mut guest = TestVm::new(&module).unwrap();
            let VmEvent::PollOneoff(call) = guest.resume(test_budget()).unwrap() else {
                panic!("poll expected");
            };
            let input: std::vec::Vec<_> = requests.iter().map(|&(id, kind)| subscription(id, kind)).collect();
            let ready: std::vec::Vec<_> = events.iter().map(|&(id, kind)| event(id, kind)).collect();
            guest.write_memory(64, input.as_flattened()).unwrap();
            guest.write_memory(512, &[0xa5; 96]).unwrap();
            guest.write_memory(0, &[0xa5; 4]).unwrap();
            let accepted = guest.finish_poll_oneoff(call, &ready, 0).is_ok();
            let mut actual = [0; 96];
            guest.read_memory(512, &mut actual).unwrap();
            if accepted {
                assert_eq!(&actual[..ready.len() * 32], ready.as_flattened());
                assert!(actual[ready.len() * 32..].iter().all(|byte| *byte == 0xa5));
                assert_eq!(guest.core.read_memory_u32(0).unwrap(), ready.len() as u32);
            } else {
                assert_eq!(actual, [0xa5; 96]);
                assert_eq!(guest.core.read_memory_u32(0).unwrap(), 0xa5a5a5a5);
            }
            let render = |keys: &[(u64, u8)]| -> std::string::String {
                let items: std::vec::Vec<_> = keys.iter().map(|(id, kind)| std::format!("({id}, {kind})")).collect();
                std::format!("[{}]", items.join(", "))
            };
            writeln!(lean, "theorem actual_vm_decision_{index} : admitted {} {} = {accepted} := by decide", render(requests), render(events)).unwrap();
            writeln!(lean, "#print axioms actual_vm_decision_{index}").unwrap();
            index += 1;
        }
    }
    assert_eq!(index, 520);
    lean.push_str("end WasiPoll\n");
    std::fs::write(output, lean).expect("write Lean export");
    std::println!("Exported {index} actual VM admission decisions; accepted writes and rejected memory preservation checked");
    std::println!("Native bytes: request={} completion={} pending={} boundary={}",
        core::mem::size_of::<crate::WasiImportRequest>(), core::mem::size_of::<crate::WasiImportCompletion>(),
        core::mem::size_of::<crate::WasiImportPending>(), core::mem::size_of::<crate::WasiBoundaryStep>());
}

#[test]
fn poll_accepts_the_full_fd_domain_and_rejects_one_more_without_writing() {
    let input: std::vec::Vec<_> = (0..16).map(|id| subscription(id, (id % 3) as u8)).collect();
    let ready: std::vec::Vec<_> = (0..16).map(|id| event(id, (id % 3) as u8)).collect();
    for count in [0, 16, 17] {
        let module = module(1024, 512, count, 0);
        let mut guest = TestVm::new(&module).unwrap();
        let VmEvent::PollOneoff(call) = guest.resume(test_budget()).unwrap() else {
            panic!("poll expected");
        };
        guest.write_memory(1024, input.as_flattened()).unwrap();
        guest.write_memory(512, &[0xa5; 512]).unwrap();
        guest.write_memory(0, &[0xa5; 4]).unwrap();
        let accepted = guest.finish_poll_oneoff(call, &ready, 0).is_ok();
        assert_eq!(accepted, count == 16);
        let mut actual = [0; 512];
        guest.read_memory(512, &mut actual).unwrap();
        if accepted {
            assert_eq!(actual, ready.as_flattened());
            assert_eq!(guest.core.read_memory_u32(0).unwrap(), 16);
        } else {
            assert_eq!(actual, [0xa5; 512]);
            assert_eq!(guest.core.read_memory_u32(0).unwrap(), 0xa5a5a5a5);
        }
    }
}
