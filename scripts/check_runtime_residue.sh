#!/bin/sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$repo_root"

fail=0

check_absent() {
    label=$1
    pattern=$2
    shift 2
    if rg -n --glob '!**/target/**' "$pattern" "$@"; then
        printf '%s\n' "residue: $label" >&2
        fail=1
    fi
}

runtime_source() {
    find src -name '*.rs' -print | while IFS= read -r file; do
        awk '
            BEGIN { cfg_test = 0; in_tests = 0 }
            cfg_test {
                if ($0 ~ /^[[:space:]]*mod tests[[:space:]]*\{/) {
                    in_tests = 1
                }
                cfg_test = 0
                next
            }
            /^[[:space:]]*#\[cfg\(test\)\]/ {
                cfg_test = 1
                next
            }
            !in_tests { print FILENAME ":" FNR ":" $0 }
        ' "$file"
    done
}

check_absent \
    "old source vocabulary" \
    'hibana-pico|Game Boy|GameBoy|gameboy|F#|FSharp|Fame|Fable|Blazor|Pokemon|Pokémon|CHIP-8|resource envelope' \
    Cargo.toml README.md src guest examples

check_absent \
    "syscall feature profiles" \
    '\[features\]|cfg\(feature|feature =|deadline_tick|new_pages\(\)|new_pages: Option' \
    Cargo.toml src guest examples

check_absent \
    "dead public runtime protocol rows" \
    'ProcExitReq|ProcExitStatus|LABEL_WASI_PROC_EXIT|TAG_REQ_WASI_PROC_EXIT|BudgetRunMsg|BudgetExpiredMsg|BudgetSuspend|BudgetRestart|LABEL_ENGINE_RUN|LABEL_ENGINE_BUDGET_EXPIRED|LABEL_ENGINE_SUSPEND|LABEL_ENGINE_RESTART|FdError|LABEL_WASI_FD_ERROR|MemReadGrantControl|MemWriteGrantControl|LABEL_MEM_GRANT_READ_CONTROL|LABEL_MEM_GRANT_WRITE_CONTROL' \
    Cargo.toml src guest examples

check_absent \
    "public internal wire tags or engine envelope enums" \
    'pub[[:space:]]+const[[:space:]]+TAG_|pub[[:space:]]+enum[[:space:]]+EngineReq|pub[[:space:]]+enum[[:space:]]+EngineRet' \
    Cargo.toml src guest examples

check_absent \
    "localside hiding helpers" \
    'complete_offered_row|drive_all|drive_|offer::|standard_shell|read_only_fs|unsupported_by_choreography|handler set|handler sets|branch adapter|answer_|MemoryFence|HibanaMemoryFence|MemFence|LABEL_MEM_FENCE|memory[- ]fence|memory-growth fencing|fence_epoch' \
    README.md src guest examples

check_absent \
    "string payload runtime errors" \
    'Invalid\(&'\''static|Unsupported\(&'\''static|WasmError::Invalid\([[:space:]]*"|WasmError::Unsupported\([[:space:]]*"' \
    src

check_absent \
    "raw lease sentinel protocol surface" \
    'MEM_LEASE_NONE|lease_id:[[:space:]]*u8|pub const .*LEASE.*=[[:space:]]*0' \
    src

check_absent \
    "untyped host completion" \
    'finish_host_call|fn finish_host_import|\.finish_host_import' \
    src

check_absent \
    "broad fallback/default residue" \
    'Default|pub const EMPTY|unwrap_or|unwrap_or_default|TODO|FIXME|deprecated|legacy|compatibility|compat alias|fallback path' \
    src

if runtime_source | rg -n 'extern crate std|std::|Vec<|Box<|String|format!|println!|eprintln!|panic!|todo!|unimplemented!'; then
    printf '%s\n' "residue: host or panic surface in non-test runtime source" >&2
    fail=1
fi

if rg -n '^[[:space:]]*Done,' src/engine/wasm/mod.rs; then
    printf '%s\n' "residue: public facade exposes a second termination event" >&2
    fail=1
fi

exit "$fail"
