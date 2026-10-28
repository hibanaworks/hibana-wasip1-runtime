//! Execute the resource fixture and measure the same runtime linked for Pico 2.
#![forbid(long_running_const_eval)]
mod check_common;
mod pico2_elf;
use check_common::{Result, fresh, run};
use pico2_elf::{FLASH_LIMIT, RAM_LIMIT, STACK_RESERVED};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

const TARGET: &str = "thumbv8m.main-none-eabi";

fn leb(mut value: usize) -> Vec<u8> {
    let mut result = Vec::new();
    while value >= 128 {
        result.push((value as u8 & 127) | 128);
        value >>= 7;
    }
    result.push(value as u8);
    result
}
fn i32(mut value: i32) -> Vec<u8> {
    let mut result = vec![0x41];
    loop {
        let byte = value as u8 & 127;
        value >>= 7;
        if (value == 0 && byte & 64 == 0) || (value == -1 && byte & 64 != 0) {
            result.push(byte);
            return result;
        }
        result.push(byte | 128);
    }
}
fn name(value: &[u8]) -> Vec<u8> {
    [leb(value.len()), value.to_vec()].concat()
}
fn section(kind: u8, value: &[u8]) -> Vec<u8> {
    [vec![kind], leb(value.len()), value.to_vec()].concat()
}

fn guest() -> Vec<u8> {
    let mut records = vec![0u8; 16 * 48];
    for index in 0..16 {
        let offset = index * 48;
        records[offset..offset + 8].copy_from_slice(&(index as u64 + 1).to_le_bytes());
        let kind = index % 3;
        records[offset + 8] = kind as u8;
        records[offset + 16..offset + 20]
            .copy_from_slice(&(if kind == 0 { 1u32 } else { index as u32 }).to_le_bytes());
    }
    let body = [
        vec![0],
        i32(64),
        i32(1024),
        i32(16),
        i32(2048),
        vec![0x10, 0, 0x04, 0x40, 0, 0x0b],
        i32(2048),
        vec![0x28, 2, 0],
        i32(16),
        vec![0x47, 0x04, 0x40, 0, 0x0b, 0x0b],
    ]
    .concat();
    [
        b"\0asm\x01\0\0\0".to_vec(),
        section(
            1,
            &[2, 0x60, 4, 0x7f, 0x7f, 0x7f, 0x7f, 1, 0x7f, 0x60, 0, 0],
        ),
        section(
            2,
            &[
                vec![1],
                name(b"wasi_snapshot_preview1"),
                name(b"poll_oneoff"),
                vec![0, 0],
            ]
            .concat(),
        ),
        section(3, &[1, 1]),
        section(5, &[1, 1, 1, 1]),
        section(7, &[vec![1], name(b"_start"), vec![0, 1]].concat()),
        section(10, &[vec![1], leb(body.len()), body].concat()),
        section(
            11,
            &[vec![1, 0], i32(64), vec![0x0b], leb(records.len()), records].concat(),
        ),
    ]
    .concat()
}

fn quoted(value: &str) -> String {
    let mut output = String::from("\"");
    for character in value.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            c if c < ' ' => output.push_str(&format!("\\u{:04x}", c as u32)),
            c => output.push(c),
        }
    }
    output.push('"');
    output
}

fn cargo(project: &Path, arguments: &[&str], flags: &str) -> Command {
    let mut command = Command::new("cargo");
    command
        .arg("+stable")
        .args(arguments)
        .current_dir(project)
        .env("CARGO_BUILD_JOBS", "1")
        .env("CARGO_TARGET_DIR", project.join("target"))
        .env("WASI_RESOURCE_GUEST", project.join("guest.wasm"))
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env("RUSTFLAGS", flags);
    command
}

fn check(root: &Path, project: &Path, evidence: &Path) -> Result<()> {
    fs::create_dir(project.join("src"))?;
    fs::copy(
        root.join("scripts/fixtures/pico2_resources.rs"),
        project.join("src/main.rs"),
    )?;
    fs::write(project.join("guest.wasm"), guest())?;
    fs::write(
        project.join("Cargo.toml"),
        format!(
            r#"[package]
name = "runtime-resource-probe"
version = "0.0.0"
edition = "2024"
[workspace]
[dependencies]
hibana-wasip1-runtime = {{ path = {} }}
[profile.release]
opt-level = "z"
codegen-units = 1
panic = "abort"
[lints.rust]
long_running_const_eval = "forbid"
"#,
            quoted(root.to_str().ok_or("non-UTF-8 runtime path")?)
        ),
    )?;
    let linker = project.join("resource.ld");
    fs::write(
        &linker,
        format!(
            r#"ENTRY(runtime_resource_entry)
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
"#
        ),
    )?;
    let flags = "-C embed-bitcode=no";
    run(
        &mut cargo(project, &["generate-lockfile", "--offline"], flags),
        &evidence.join("lock.log"),
    )?;
    fs::copy(
        project.join("Cargo.lock"),
        evidence.join("resource-Cargo.lock"),
    )?;
    run(
        &mut cargo(project, &["tree", "--offline", "--locked"], flags),
        &evidence.join("dependencies.log"),
    )?;
    run(
        &mut cargo(
            project,
            &["run", "--offline", "--locked", "--release"],
            flags,
        ),
        &evidence.join("host.log"),
    )?;
    println!("Host: all 16 poll interests completed");
    run(
        &mut cargo(
            project,
            &["clippy", "--offline", "--locked", "--", "-D", "warnings"],
            flags,
        ),
        &evidence.join("clippy.log"),
    )?;
    let flags = format!(
        "{flags} -C link-arg=-T{} -C link-arg=--gc-sections",
        linker.display()
    );
    run(
        &mut cargo(
            project,
            &[
                "build",
                "--offline",
                "--locked",
                "--release",
                "--target",
                TARGET,
            ],
            &flags,
        ),
        &evidence.join("pico2.log"),
    )?;

    let sysroot = run(
        Command::new("rustc").args(["+stable", "--print", "sysroot"]),
        &evidence.join("sysroot.log"),
    )?;
    let version = run(
        Command::new("rustc").args(["+stable", "-vV"]),
        &evidence.join("compiler.log"),
    )?;
    let host = version
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .ok_or("missing compiler host")?;
    let llvm = Path::new(sysroot.trim())
        .join("lib/rustlib")
        .join(host)
        .join("bin");
    let binary = project
        .join("target")
        .join(TARGET)
        .join("release/runtime-resource-probe");
    run(
        Command::new(llvm.join("llvm-size")).arg("-A").arg(&binary),
        &evidence.join("sections.log"),
    )?;
    let data = fs::read(&binary)?;
    let analysis = pico2_elf::inspect(&data)?;
    pico2_elf::reject_mutations(&data, &analysis)?;
    let assembly = run(
        Command::new(llvm.join("llvm-objdump"))
            .arg("-d")
            .arg(&binary),
        &evidence.join("assembly.log"),
    )?;
    if assembly.trim().is_empty() {
        return Err("empty target disassembly".into());
    }
    let sections = analysis
        .sections
        .iter()
        .map(|(key, value)| format!("{}: {value}", quoted(key)))
        .collect::<Vec<_>>()
        .join(", ");
    let types = [
        "guest_memory",
        "guest_storage",
        "fd_bindings",
        "request",
        "completion",
        "pending",
        "boundary",
    ];
    let type_sizes = types
        .into_iter()
        .zip(analysis.type_sizes)
        .map(|(key, value)| format!("{}: {value}", quoted(key)))
        .collect::<Vec<_>>()
        .join(", ");
    let stack = analysis.sections[".stack"] as u64;
    fs::write(
        evidence.join("resources.json"),
        format!(
            r#"{{
  "target": "{TARGET}",
  "linked_flash_bytes": {},
  "static_ram_bytes": {},
  "stack_reserved_bytes": {stack},
  "reserved_ram_bytes": {},
  "flash_limit_bytes": {FLASH_LIMIT},
  "ram_limit_bytes": {RAM_LIMIT},
  "ram_accounting": "loaded address extent including alignment",
  "sections": {{{sections}}},
  "type_sizes": {{{type_sizes}}},
  "elf_negative_controls_rejected": 3,
  "physical_execution": "not verified",
  "worst_case_stack": "not verified"
}}
"#,
            analysis.flash,
            analysis.ram - stack,
            analysis.ram
        ),
    )?;
    println!(
        "Pico 2 link: flash={} bytes, RAM including stack={} bytes; three ELF mutations rejected",
        analysis.flash, analysis.ram
    );
    Ok(())
}

fn main() -> Result<()> {
    let root = env::current_dir()?.canonicalize()?;
    let mut arguments = env::args_os().skip(1);
    let evidence = match arguments.next() {
        Some(path) => {
            let path = PathBuf::from(path);
            fs::create_dir(&path)?;
            path.canonicalize()?
        }
        None => fresh("wasi-pico2-evidence")?,
    };
    if arguments.next().is_some() {
        return Err("usage: check-pico2 [NEW_EVIDENCE_DIRECTORY]".into());
    }
    println!("Evidence: {}", evidence.display());
    let project = fresh("wasi-pico2-target")?;
    let result = check(&root, &project, &evidence);
    let cleanup = fs::remove_dir_all(&project);
    match (result, cleanup) {
        (Ok(()), Ok(())) => {
            println!("Temporary Rust products removed");
            Ok(())
        }
        (Err(error), Ok(())) => Err(error),
        (Ok(()), Err(error)) => Err(error.into()),
        (Err(error), Err(cleanup)) => {
            Err(format!("{error}; target cleanup also failed: {cleanup}").into())
        }
    }
}
