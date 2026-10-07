#!/usr/bin/env python3
"""Verify bounded poll, deleting Rust products after every individual check."""
import os
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
MODEL = ROOT / "proofs/PollWriteback.lean"
PROOF_WORKSPACE = ROOT.parent / "hibana/proofs/lean"


def rust(evidence: Path, name: str, arguments: list[str]) -> None:
    with tempfile.TemporaryDirectory(prefix=f"wasi-poll-{name}-") as target:
        env = dict(os.environ, CARGO_TARGET_DIR=target,
                   HIBANA_WASIP1_POLL_LEAN_EXPORT=str(evidence / "decisions.lean"),
                   HIBANA_WASIP1_PATH_LEAN_EXPORT=str(evidence / "path-decisions.lean"))
        log = evidence / f"{name}.log"
        with log.open("w") as output:
            result = subprocess.run(["cargo", "+1.95.0", *arguments], cwd=ROOT,
                                    env=env, stdout=output, stderr=subprocess.STDOUT, check=False)
    print(f"{name}: Rust target removed", flush=True)
    if result.returncode:
        print(log.read_text())
        raise SystemExit(result.returncode)
    print(f"{name}: passed ({log})", flush=True)


def main() -> None:
    evidence = Path(tempfile.mkdtemp(prefix="wasi-poll-evidence-"))
    print(f"Evidence: {evidence}", flush=True)
    rust(evidence, "tests", ["test", "--offline", "--locked", "--all-targets",
                             "--", "--include-ignored", "--nocapture"])
    rust(evidence, "clippy", ["clippy", "--offline", "--locked", "--all-targets",
                              "--", "-D", "warnings"])
    rust(evidence, "pico2", ["check", "--offline", "--locked", "--lib", "--target", "thumbv8m.main-none-eabi"])
    rust(evidence, "ownership", ["test", "--offline", "--locked", "--doc"])
    combined = evidence / "Correspondence.lean"
    combined.write_text(MODEL.read_text() + (evidence / "decisions.lean").read_text().replace("import PollWriteback\n", ""))
    proof = subprocess.run(["lake", "env", "lean", str(combined)], cwd=PROOF_WORKSPACE,
                           text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, check=False)
    (evidence / "lean.log").write_text(proof.stdout)
    names = [
        "matching_is_sublist", "matching_bounds_events", "matching_never_fabricates",
        "matching_preserves_occurrence_count", "write_outside", "write_inside",
        "rejection_preserves_memory", "invalid_last_input_rejects", "invalid_full_output_rejects",
        "overlapping_outputs_reject", "prepared_event_bytes_fit", "prepared_output_regions_disjoint",
        "commit_preserves_event_bytes", "commit_writes_count_bytes", "commit_preserves_outside",
        "duplicate_occurrences_allowed", "extra_occurrence_rejected", "reordered_occurrences_rejected",
        "changed_kind_rejected", "missing_userdata_rejected",
    ]
    expected = []
    for index, name in enumerate(names):
        axiom_text = " does not depend on any axioms" if index >= 15 else (
            " depends on axioms: [propext]" if index < 3 else " depends on axioms: [propext, Quot.sound]")
        expected.append(f"'WasiPoll.{name}'{axiom_text}")
    expected.extend(f"'WasiPoll.actual_vm_decision_{index}' does not depend on any axioms" for index in range(520))
    if proof.returncode or proof.stdout.splitlines() != expected:
        print(proof.stdout)
        raise SystemExit("Lean proof or exact axiom audit failed")
    print(f"Lean: 20 model theorems and 520 actual VM decisions passed ({evidence / 'lean.log'})")
    path_combined = evidence / "PathCorrespondence.lean"
    path_combined.write_text((ROOT / "proofs/PathRights.lean").read_text() +
                             (evidence / "path-decisions.lean").read_text().replace("import PathRights\n", ""))
    path_proof = subprocess.run(["lake", "env", "lean", str(path_combined)], cwd=PROOF_WORKSPACE,
                                text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, check=False)
    (evidence / "path-lean.log").write_text(path_proof.stdout)
    path_names = ["accepted_has_capability", "accepted_has_exact_io_mode", "mismatched_io_mode_rejected",
                  "writable_requires_write", "read_request_cannot_open_writer", "empty_material_rejected"]
    path_expected = [f"'WasiPathRights.{name}' depends on axioms: [propext]" for name in path_names]
    path_expected.extend(f"'WasiPathRights.actual_path_decision_{index}' depends on axioms: [propext]"
                         for index in range(128))
    if path_proof.returncode or path_proof.stdout.splitlines() != path_expected:
        print(path_proof.stdout)
        raise SystemExit("Path-rights Lean proof or exact axiom audit failed")
    print("Lean: 6 path-rights theorems and 128 actual ChoreoFS decisions passed")
    smt = subprocess.run(["z3", str(ROOT / "proofs/PathRights.smt2")], cwd=ROOT,
                         text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, check=False)
    (evidence / "path-z3.log").write_text(smt.stdout)
    if smt.returncode or smt.stdout.splitlines() != ["unsat"] * 4 + ["sat"] * 3:
        print(smt.stdout)
        raise SystemExit("Path-rights Z3 queries failed")
    print("Z3: 4 UNSAT guarantees and 3 SAT witnesses passed over 64-bit rights")


if __name__ == "__main__":
    main()
