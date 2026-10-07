use super::{
    Call, ExchangeError, FD_BINDING_CAPACITY, FdBindingCapacityError, FdBindingTable,
    HibanaWasiGuestStorage, MAX_ARG_REFS, MAX_ENV_REFS, UNSUPPORTED_WASIP1_CLOCK_ID_TOO_LARGE,
    UNSUPPORTED_WASIP1_INLINE_REPLY_TOO_LARGE, UNSUPPORTED_WASIP1_PATH_REPLY_TOO_LARGE,
    WASIP1_IO_CHUNK_CAPACITY, WasiBoundaryStep, WasiImportPending, WasiImportRequest, clock_id_u8,
    exact_io_reply_len, exact_path_reply_len, inline_io_request_len, prepare_fd_close_bindings,
    prepare_path_open_bindings, split_args, split_environ, wasm_fd_stat,
};
use crate::{
    DEFAULT_GUEST_MEMORY_BYTES, GuestMemory, WasmError,
    protocol::{FdBinding, FdReadRow, FdWriteRow, PathOpened},
};
use core::mem::size_of;
use std::boxed::Box;

#[test]
fn directory_child_rights_and_file_base_rights_are_distinct() {
    use crate::protocol::{self, FdReaddirRow, FdStat, MemRights};
    let dir = wasm_fd_stat(
        FdStat::new(3, MemRights::Read),
        Some(FdBinding::readdir(FdReaddirRow::Base)),
    );
    assert_eq!(dir.filetype(), protocol::WASIP1_FILETYPE_DIRECTORY);
    assert_eq!(dir.rights_base(), 1 << 14);
    assert_eq!(dir.rights_inheriting(), 2 | 64 | (1 << 14));
    for (rights, binding, expected) in [
        (MemRights::Read, FdBinding::read(FdReadRow::Base), 2),
        (MemRights::Write, FdBinding::write(FdWriteRow::Object), 64),
    ] {
        let file = wasm_fd_stat(FdStat::new(4, rights), Some(binding));
        assert_eq!(file.rights_base(), expected);
        assert_eq!(file.rights_inheriting(), 0);
    }
}

#[test]
fn binding_table_and_pending_token_stay_small() {
    assert!(
        size_of::<FdBindingTable>() <= FD_BINDING_CAPACITY * 8,
        "FdBindingTable uses {} bytes",
        size_of::<FdBindingTable>()
    );
    assert!(
        size_of::<Call>() <= 64,
        "Call uses {} bytes",
        size_of::<Call>()
    );
    assert!(
        size_of::<WasiImportRequest>() <= WASIP1_IO_CHUNK_CAPACITY + 16,
        "WasiImportRequest uses {} bytes",
        size_of::<WasiImportRequest>()
    );
    assert!(
        size_of::<WasiImportPending>() <= size_of::<WasiImportRequest>() + 48,
        "WasiImportPending uses {} bytes",
        size_of::<WasiImportPending>()
    );
    assert!(
        size_of::<WasiBoundaryStep>() <= size_of::<WasiImportPending>() + 8,
        "WasiBoundaryStep uses {} bytes",
        size_of::<WasiBoundaryStep>()
    );
}

#[test]
fn fd_binding_capacity_counts_live_entries_not_fd_numbers() {
    let mut bindings = FdBindingTable::empty();
    let read = FdBinding::read(FdReadRow::Base);
    let write = FdBinding::write(FdWriteRow::Base);

    bindings.bind_fd(u8::MAX, read).expect("high fd");
    assert_eq!(bindings.binding(u8::MAX), Some(read));
    bindings.bind_fd(u8::MAX, write).expect("replace high fd");
    assert_eq!(bindings.binding(u8::MAX), Some(write));
    bindings.remove_fd(u8::MAX);
    assert_eq!(bindings.binding(u8::MAX), None);

    for fd in 0..FD_BINDING_CAPACITY as u8 {
        bindings.bind_fd(fd, read).expect("binding slot");
    }
    assert_eq!(
        bindings.bind_fd(200, read),
        Err(FdBindingCapacityError::new(200))
    );
    bindings.remove_fd(7);
    bindings.bind_fd(200, read).expect("reused binding slot");
    assert_eq!(bindings.binding(200), Some(read));
}

#[test]
fn fd_binding_transitions_are_prepared_before_publication() {
    let read = FdBinding::read(FdReadRow::Base);
    let mut bindings = FdBindingTable::empty();
    for fd in 0..FD_BINDING_CAPACITY as u8 {
        bindings.bind_fd(fd, read).expect("binding slot");
    }

    assert_eq!(
        prepare_path_open_bindings(bindings, PathOpened::new_with_binding(200, 0, read)),
        Err(FdBindingCapacityError::new(200))
    );
    assert_eq!(bindings.binding(0), Some(read));

    let prepared = prepare_fd_close_bindings(bindings, 0, 0);
    assert_eq!(bindings.binding(0), Some(read));
    assert_eq!(prepared.binding(0), None);
    assert_eq!(prepare_fd_close_bindings(bindings, 0, 8), bindings);
}

#[test]
fn failed_in_place_initialization_consumes_storage() {
    let mut first_memory = Box::new([0u8; DEFAULT_GUEST_MEMORY_BYTES]);
    let mut second_memory = Box::new([0u8; DEFAULT_GUEST_MEMORY_BYTES]);
    let mut storage = Box::new(HibanaWasiGuestStorage::uninit());
    assert!(matches!(
        storage.init(
            &[],
            GuestMemory::new(&mut first_memory[..]),
            FdBindingTable::empty()
        ),
        Err(ExchangeError::Wasm(_))
    ));

    assert!(matches!(
        storage.init(
            &[],
            GuestMemory::new(&mut second_memory[..]),
            FdBindingTable::empty()
        ),
        Err(ExchangeError::GuestStorageConsumed)
    ));
    drop(storage);
}

#[test]
fn inline_io_len_only_clamps_partial_transfer_imports() {
    assert_eq!(inline_io_request_len(0), 0);
    assert_eq!(
        inline_io_request_len(WASIP1_IO_CHUNK_CAPACITY),
        WASIP1_IO_CHUNK_CAPACITY as u8
    );
    assert_eq!(
        inline_io_request_len(WASIP1_IO_CHUNK_CAPACITY + 1),
        WASIP1_IO_CHUNK_CAPACITY as u8
    );

    assert!(matches!(
        exact_io_reply_len(WASIP1_IO_CHUNK_CAPACITY as u32),
        Ok(len) if len == WASIP1_IO_CHUNK_CAPACITY as u8
    ));
    assert!(matches!(
        exact_io_reply_len(WASIP1_IO_CHUNK_CAPACITY as u32 + 1),
        Err(ExchangeError::Wasm(WasmError::Unsupported(code)))
            if code == UNSUPPORTED_WASIP1_INLINE_REPLY_TOO_LARGE
    ));
}

#[test]
fn exact_path_and_clock_values_fail_fast() {
    assert!(matches!(exact_path_reply_len(40), Ok(40)));
    assert!(matches!(
        exact_path_reply_len(41),
        Err(ExchangeError::Wasm(WasmError::Unsupported(code)))
            if code == UNSUPPORTED_WASIP1_PATH_REPLY_TOO_LARGE
    ));

    assert!(matches!(clock_id_u8(255), Ok(255)));
    assert!(matches!(
        clock_id_u8(256),
        Err(ExchangeError::Wasm(WasmError::Unsupported(code)))
            if code == UNSUPPORTED_WASIP1_CLOCK_ID_TOO_LARGE
    ));
}

#[test]
fn canonical_argument_and_environment_lists_preserve_every_entry() {
    let mut args = [&[][..]; MAX_ARG_REFS];
    let count = split_args(b"\0hibana\0", 2, &mut args).expect("arguments");
    assert_eq!(&args[..count], &[&b""[..], &b"hibana"[..]]);
    assert!(split_args(b"unterminated", 1, &mut args).is_err());

    let mut environ = [(&[][..], &[][..]); MAX_ENV_REFS];
    let count = split_environ(b"MODE=test=1\0EMPTY=\0", 2, &mut environ).expect("environment");
    assert_eq!(
        &environ[..count],
        &[(&b"MODE"[..], &b"test=1"[..]), (&b"EMPTY"[..], &b""[..]),]
    );
    assert!(split_environ(b"MISSING_SEPARATOR\0", 1, &mut environ).is_err());
}
