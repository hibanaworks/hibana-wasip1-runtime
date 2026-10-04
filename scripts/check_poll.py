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
                   HIBANA_WASIP1_POLL_LEAN_EXPORT=str(evidence / "decisions.lean"))
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


if __name__ == "__main__":
    main()
