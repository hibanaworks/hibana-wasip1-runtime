#!/usr/bin/env python3
"""Check the unified runtime on Pico 2; retain logs, remove every Rust product."""
import json
import os
from pathlib import Path
import re
import struct
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
TARGET = "thumbv8m.main-none-eabi"
RAM_LIMIT = 128 * 1024
FLASH_LIMIT = 512 * 1024
STACK_RESERVED = 24 * 1024


def leb(value: int) -> bytes:
    result = bytearray()
    while value >= 128:
        result.append((value & 127) | 128)
        value >>= 7
    result.append(value)
    return bytes(result)


def i32(value: int) -> bytes:
    result = bytearray()
    while True:
        byte = value & 127
        value >>= 7
        if (value == 0 and not byte & 64) or (value == -1 and byte & 64):
            result.append(byte)
            return b"\x41" + result
        result.append(byte | 128)


def guest() -> bytes:
    """A guest that polls all 16 interests and traps on errno or wrong count."""
    def section(kind: int, data: bytes) -> bytes:
        return bytes([kind]) + leb(len(data)) + data

    def name(value: bytes) -> bytes:
        return leb(len(value)) + value

    records = bytearray(16 * 48)
    for index in range(16):
        offset = index * 48
        struct.pack_into("<Q", records, offset, index + 1)
        kind = index % 3
        records[offset + 8] = kind
        struct.pack_into("<I", records, offset + 16, 1 if kind == 0 else index)
    body = (b"\x00" + i32(64) + i32(1024) + i32(16) + i32(2048)
            + b"\x10\x00\x04\x40\x00\x0b"
            + i32(2048) + b"\x28\x02\x00" + i32(16)
            + b"\x47\x04\x40\x00\x0b\x0b")
    return (b"\x00asm\x01\x00\x00\x00"
            + section(1, b"\x02\x60\x04\x7f\x7f\x7f\x7f\x01\x7f\x60\x00\x00")
            + section(2, b"\x01" + name(b"wasi_snapshot_preview1") + name(b"poll_oneoff") + b"\x00\x00")
            + section(3, b"\x01\x01")
            + section(5, b"\x01\x01\x01\x01")
            + section(7, b"\x01" + name(b"_start") + b"\x00\x01")
            + section(10, b"\x01" + leb(len(body)) + body)
            + section(11, b"\x01\x00" + i32(64) + b"\x0b" + leb(len(records)) + records))


def command(arguments: list[str], cwd: Path, env: dict, log: Path) -> str:
    result = subprocess.run(arguments, cwd=cwd, env=env, text=True,
                            stdout=subprocess.PIPE, stderr=subprocess.STDOUT, check=False)
    with log.open("a") as output:
        output.write(result.stdout)
    if result.returncode:
        print(result.stdout)
        raise SystemExit(result.returncode)
    return result.stdout


def check(evidence: Path, name: str) -> None:
    with tempfile.TemporaryDirectory(prefix=f"wasi-pico2-{name}-") as temporary:
        project = Path(temporary)
        (project / "src").mkdir()
        (project / "src/main.rs").write_text((ROOT / "scripts/fixtures/pico2_resources.rs").read_text())
        (project / "guest.wasm").write_bytes(guest())
        (project / "Cargo.toml").write_text(f'''[package]
name = "runtime-resource-probe"
version = "0.0.0"
edition = "2024"
[dependencies]
hibana-wasip1-runtime = {{ path = {json.dumps(str(ROOT))} }}
[patch.crates-io]
hibana = {{ path = {json.dumps(str(ROOT.parent / "hibana"))} }}
[profile.release]
opt-level = "z"
codegen-units = 1
panic = "abort"
''')
        linker = project / "resource.ld"
        linker.write_text(f'''ENTRY(runtime_resource_entry)
MEMORY {{ FLASH (rx) : ORIGIN = 0x10000000, LENGTH = 4M
         RAM (rwx) : ORIGIN = 0x20000000, LENGTH = 520K }}
SECTIONS {{
 .text : {{ *(.text*) *(.rodata*) }} > FLASH
 .ARM.extab : {{ *(.ARM.extab*) }} > FLASH
 .ARM.exidx : {{ *(.ARM.exidx*) }} > FLASH
 .runtime_budget : {{ KEEP(*(.runtime_budget)) }} > FLASH
 .data : {{ *(.data*) }} > RAM AT > FLASH
 .bss (NOLOAD) : {{ *(.bss*) *(COMMON) }} > RAM
 .stack (NOLOAD) : {{ . = ALIGN(8); . += {STACK_RESERVED}; }} > RAM
}}
ASSERT(SIZEOF(.data) + SIZEOF(.bss) + SIZEOF(.stack) <= {RAM_LIMIT}, "runtime RAM budget")
''')
        env = dict(os.environ, CARGO_TARGET_DIR=str(project / "target"),
                   WASI_RESOURCE_GUEST=str(project / "guest.wasm"))
        # The fixture uses its own linker, regardless of the caller's workspace.
        env.pop("CARGO_ENCODED_RUSTFLAGS", None)
        env["RUSTFLAGS"] = "-C embed-bitcode=no"
        log = evidence / f"{name}.log"
        if name == "host":
            command(["cargo", "+1.95.0", "run", "--offline", "--release"], project, env, log)
        elif name == "clippy":
            command(["cargo", "+1.95.0", "clippy", "--offline", "--", "-D", "warnings"], project, env, log)
        else:
            env["RUSTFLAGS"] += f" -C link-arg=-T{linker} -C link-arg=--gc-sections"
            command(["cargo", "+1.95.0", "build", "--offline", "--release", "--target", TARGET], project, env, log)
            sysroot = Path(command(["rustc", "+1.95.0", "--print", "sysroot"], project, env, log).strip())
            host = re.search(r"^host: (.+)$", command(["rustc", "+1.95.0", "-vV"], project, env, log), re.MULTILINE).group(1)
            llvm = sysroot / "lib/rustlib" / host / "bin"
            binary = project / "target" / TARGET / "release/runtime-resource-probe"
            sections = command([str(llvm / "llvm-size"), "-A", str(binary)], project, env, log)
            sizes = {match[1]: int(match[2]) for match in re.finditer(r"^(\.\S+)\s+(\d+)\s+\d+$", sections, re.MULTILINE)}
            # Count the complete flash load extent, including alignment and
            # every file-backed LOAD segment, rather than a section allowlist.
            elf = binary.read_bytes()
            if elf[:6] != b"\x7fELF\x01\x01" or struct.unpack_from("<H", elf, 18)[0] != 40:
                raise SystemExit("resource image is not a little-endian ARM ELF32")
            program_offset = struct.unpack_from("<I", elf, 28)[0]
            entry_size, entry_count = struct.unpack_from("<HH", elf, 42)
            flash_end = 0x10000000
            for index in range(entry_count):
                kind, _, virtual, physical, file_bytes, memory_bytes, _, _ = struct.unpack_from(
                    "<8I", elf, program_offset + index * entry_size)
                if kind != 1:
                    continue
                if memory_bytes and 0x20000000 <= virtual < 0x20000000 + 520 * 1024:
                    if virtual + memory_bytes > 0x20000000 + 520 * 1024:
                        raise SystemExit("load segment exceeds Pico 2 SRAM")
                if file_bytes:
                    if physical < 0x10000000 or physical + file_bytes > 0x10400000:
                        raise SystemExit("file-backed load lies outside Pico 2 flash")
                    flash_end = max(flash_end, physical + file_bytes)
            flash = flash_end - 0x10000000
            ram = sum(sizes.get(section, 0) for section in (".data", ".bss", ".stack"))
            if not sizes.get(".text") or sizes.get(".bss", 0) < 64 * 1024 or sizes.get(".stack", 0) < STACK_RESERVED:
                raise SystemExit("resource fixture lost code, VM memory or stack reservation")
            if flash > FLASH_LIMIT or ram > RAM_LIMIT:
                raise SystemExit(f"Pico 2 runtime fixture exceeded budget: flash={flash}, RAM={ram}")
            budget_dump = command([str(llvm / "llvm-objdump"), "-s", "--section=.runtime_budget", str(binary)], project, env, log)
            budget_hex = "".join(match[1].replace(" ", "") for match in re.finditer(
                r"^\s*[0-9a-f]+\s+((?:[0-9a-f]{8} ?){1,4})", budget_dump, re.MULTILINE))
            budget_bytes = bytes.fromhex(budget_hex)
            if len(budget_bytes) != 28:
                raise SystemExit("target type size evidence is incomplete")
            type_sizes = dict(zip(("guest_memory", "guest_storage", "fd_bindings", "request",
                                   "completion", "pending", "boundary"), struct.unpack("<7I", budget_bytes)))
            assembly = command([str(llvm / "llvm-objdump"), "-d", str(binary)], project, env, evidence / "assembly.log")
            if not assembly:
                raise SystemExit("empty target disassembly")
            report = {"target": TARGET, "linked_flash_bytes": flash, "static_ram_bytes": ram - sizes[".stack"],
                      "stack_reserved_bytes": sizes[".stack"], "reserved_ram_bytes": ram,
                      "flash_limit_bytes": FLASH_LIMIT, "ram_limit_bytes": RAM_LIMIT,
                      "sections": sizes, "type_sizes": type_sizes,
                      "physical_execution": "not verified", "worst_case_stack": "not verified"}
            (evidence / "resources.json").write_text(json.dumps(report, indent=2) + "\n")
            print(f"Pico 2 link: flash={flash:,} B, RAM including stack reservation={ram:,} B", flush=True)
    print(f"{name}: passed, all Rust products removed ({evidence / (name + '.log')})", flush=True)


def main() -> None:
    evidence = Path(tempfile.mkdtemp(prefix="wasi-pico2-evidence-"))
    print(f"Evidence: {evidence}", flush=True)
    check(evidence, "host")
    check(evidence, "clippy")
    check(evidence, "pico2")


if __name__ == "__main__":
    main()
