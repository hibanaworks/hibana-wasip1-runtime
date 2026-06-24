#!/bin/sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$repo_root"

if ! rustup target list --installed | rg -q '^wasm32-wasip1$'; then
    echo "wasm32-wasip1 target is not installed; run: rustup target add wasm32-wasip1" >&2
    exit 1
fi

guest_source="$repo_root/examples/wasi_std_shell_app.rs"
guest_target="$repo_root/target/wasi-std-shell-app"
guest_manifest="$guest_target/Cargo.toml"
guest_wasm="$guest_target/wasm32-wasip1/release/wasi-std-shell-app.wasm"

mkdir -p "$guest_target/src"
cp "$guest_source" "$guest_target/src/main.rs"
cat > "$guest_manifest" <<'CARGO'
[package]
name = "wasi-std-shell-app"
version = "0.1.0"
edition = "2024"

[workspace]

[dependencies]

[[bin]]
name = "wasi-std-shell-app"
path = "src/main.rs"

[profile.release]
panic = "abort"
opt-level = "z"
lto = true
codegen-units = 1
strip = "debuginfo"
CARGO

cargo generate-lockfile \
    --manifest-path "$guest_manifest" \
    --offline

RUSTFLAGS="${RUSTFLAGS:-} -C link-arg=--initial-memory=65536 -C link-arg=--max-memory=65536 -C link-arg=-zstack-size=8192" \
    cargo build \
    --manifest-path "$guest_manifest" \
    --target wasm32-wasip1 \
    --release \
    --target-dir "$guest_target" \
    --locked

blocked_output=$(
    printf '%s\n' \
        'echo 1 > /outputs/led/green' \
        'exit' |
        cargo run --quiet --locked --example direct_choreofs_write_rejection -- "$guest_wasm" 2>&1
)
printf '%s\n' "$blocked_output"
for expected in \
    '^choreography: direct ChoreoFS write blocked$' \
    '^wasi std shell app$' \
    '^(wasi> )*Hibana: ChoreoFS write did not advance on this localside -> ' \
    '^Output: led\.green = unchanged$'
do
    if ! printf '%s\n' "$blocked_output" | rg -q "$expected"; then
        echo "missing direct-write block proof line: $expected" >&2
        exit 1
    fi
done
if printf '%s\n' "$blocked_output" | rg -q '^direct-choreofs-write-rejection failed:'; then
    echo "direct-write block example should handle the expected Hibana progress rejection" >&2
    exit 1
fi

sequenced_output=$(
    printf '%s\n' \
        'help' \
        'ls /objects' \
        'cat /objects/log' \
        'apply /objects/log /outputs/led/green' \
        'exit' |
        cargo run --quiet --locked --example sequenced_choreofs_write -- "$guest_wasm" 2>&1
)
printf '%s\n' "$sequenced_output"
for expected in \
    '^choreography: sequenced ChoreoFS write$' \
    '^wasi std shell app$' \
    '^(wasi> )*commands:$' \
    '^  ls /objects$' \
    '^(wasi> )*log$' \
    '^(wasi> )*session=attached$' \
    '^(wasi> )*applied$' \
    '^(wasi> )*Output: led\.green = on$'
do
    if ! printf '%s\n' "$sequenced_output" | rg -q "$expected"; then
        echo "missing sequenced ChoreoFS write proof line: $expected" >&2
        exit 1
    fi
done
if printf '%s\n' "$sequenced_output" | rg -q '^sequenced-choreofs-write failed:'; then
    echo "sequenced ChoreoFS write example should complete" >&2
    exit 1
fi
