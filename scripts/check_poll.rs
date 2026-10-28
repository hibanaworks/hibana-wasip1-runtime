//! Build the public runtime and check its actual VM decisions with Lean and Z3.
mod check_common;
use check_common::{Result, fresh, run};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

fn cargo(root: &Path, target: &Path, evidence: &Path, args: &[&str], name: &str) -> Result<()> {
    run(
        Command::new("cargo")
            .arg("+stable")
            .args(args)
            .current_dir(root)
            .env("CARGO_BUILD_JOBS", "1")
            .env("RUST_TEST_THREADS", "1")
            .env("CARGO_TARGET_DIR", target)
            .env(
                "HIBANA_WASIP1_POLL_LEAN_EXPORT",
                evidence.join("decisions.lean"),
            )
            .env(
                "HIBANA_WASIP1_PATH_LEAN_EXPORT",
                evidence.join("path-decisions.lean"),
            ),
        &evidence.join(format!("{name}.log")),
    )?;
    println!("{name}: passed");
    Ok(())
}

fn correspondence(
    root: &Path,
    evidence: &Path,
    model: &str,
    export: &str,
    namespace: &str,
    expected: &[String],
) -> Result<()> {
    let proof = fs::read_to_string(root.join("proofs").join(format!("{model}.lean")))?;
    let decisions = fs::read_to_string(evidence.join(export))?;
    let import = format!("import {model}\n");
    let decisions = decisions
        .strip_prefix(&import)
        .ok_or("unexpected export import")?;
    let combined = evidence.join(format!("{model}Correspondence.lean"));
    fs::write(&combined, format!("{proof}\n{decisions}"))?;
    let log = evidence.join(format!("{model}-lean.log"));
    let output = run(
        Command::new("lean")
            .arg(&combined)
            .current_dir(root.join("proofs")),
        &log,
    )?;
    if output.lines().ne(expected.iter().map(String::as_str)) {
        eprint!("{output}");
        return Err(format!(
            "{namespace}: theorem inventory or exact axiom audit differs; {}",
            log.display()
        )
        .into());
    }
    println!("{namespace}: exact theorem and axiom audit passed");
    Ok(())
}

fn check(root: &Path, target: &Path, evidence: &Path) -> Result<()> {
    cargo(
        root,
        target,
        evidence,
        &[
            "test",
            "--offline",
            "--locked",
            "--all-targets",
            "--",
            "--include-ignored",
            "--nocapture",
        ],
        "tests",
    )?;
    cargo(
        root,
        target,
        evidence,
        &[
            "clippy",
            "--offline",
            "--locked",
            "--all-targets",
            "--",
            "-D",
            "warnings",
        ],
        "clippy",
    )?;
    cargo(
        root,
        target,
        evidence,
        &[
            "check",
            "--offline",
            "--locked",
            "--lib",
            "--target",
            "thumbv8m.main-none-eabi",
        ],
        "pico2",
    )?;
    cargo(
        root,
        target,
        evidence,
        &["test", "--offline", "--locked", "--doc"],
        "ownership",
    )?;

    let poll_names = [
        "matching_is_sublist",
        "matching_bounds_events",
        "matching_never_fabricates",
        "matching_preserves_occurrence_count",
        "write_outside",
        "write_inside",
        "rejection_preserves_memory",
        "invalid_last_input_rejects",
        "invalid_full_output_rejects",
        "overlapping_outputs_reject",
        "prepared_event_bytes_fit",
        "prepared_output_regions_disjoint",
        "commit_preserves_event_bytes",
        "commit_writes_count_bytes",
        "commit_preserves_outside",
        "duplicate_occurrences_allowed",
        "extra_occurrence_rejected",
        "reordered_occurrences_rejected",
        "changed_kind_rejected",
        "missing_userdata_rejected",
    ];
    let mut poll_expected = Vec::new();
    for (index, name) in poll_names.iter().enumerate() {
        let axioms = if index >= 15 {
            " does not depend on any axioms"
        } else if index < 3 {
            " depends on axioms: [propext]"
        } else {
            " depends on axioms: [propext, Quot.sound]"
        };
        poll_expected.push(format!("'WasiPoll.{name}'{axioms}"));
    }
    poll_expected.extend(
        (0..520)
            .map(|n| format!("'WasiPoll.actual_vm_decision_{n}' does not depend on any axioms")),
    );
    correspondence(
        root,
        evidence,
        "PollWriteback",
        "decisions.lean",
        "WasiPoll",
        &poll_expected,
    )?;

    let path_names = [
        "accepted_has_capability",
        "accepted_has_exact_io_mode",
        "mismatched_io_mode_rejected",
        "writable_requires_write",
        "read_request_cannot_open_writer",
        "empty_material_rejected",
        "read_write_requires_requested_io",
        "read_write_accepts_read",
        "read_write_accepts_write",
        "read_write_accepts_both",
    ];
    let mut path_expected: Vec<_> = path_names
        .iter()
        .map(|name| format!("'WasiPathRights.{name}' depends on axioms: [propext]"))
        .collect();
    path_expected.extend((0..160).map(|n| {
        format!("'WasiPathRights.actual_path_decision_{n}' depends on axioms: [propext]")
    }));
    correspondence(
        root,
        evidence,
        "PathRights",
        "path-decisions.lean",
        "WasiPathRights",
        &path_expected,
    )?;

    let smt = run(
        Command::new("z3")
            .arg(root.join("proofs/PathRights.smt2"))
            .current_dir(root),
        &evidence.join("path-z3.log"),
    )?;
    if smt
        .lines()
        .ne(["unsat", "unsat", "unsat", "unsat", "sat", "sat", "sat"])
    {
        return Err(format!("unexpected Z3 results: {smt}").into());
    }
    println!(
        "20 poll theorems, 520 VM decisions, 10 path-rights theorems, 160 ChoreoFS decisions and 7 Z3 queries passed"
    );
    Ok(())
}

fn main() -> Result<()> {
    let root = env::current_dir()?.canonicalize()?;
    let mut args = env::args_os().skip(1);
    let evidence = match args.next() {
        Some(path) => {
            let path = PathBuf::from(path);
            fs::create_dir(&path)?;
            path.canonicalize()?
        }
        None => fresh("wasi-poll-evidence")?,
    };
    if args.next().is_some() {
        return Err("usage: check-poll [NEW_EVIDENCE_DIRECTORY]".into());
    }
    println!("Evidence: {}", evidence.display());
    let target = fresh("wasi-poll-target")?;
    let result = check(&root, &target, &evidence);
    let cleanup = fs::remove_dir_all(&target);
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
