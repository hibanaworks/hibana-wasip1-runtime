//! Verify the Cargo archive against the same immutable core as this checkout.
#![forbid(long_running_const_eval)]
mod check_common;
use check_common::{Result, fresh, run};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

fn check(root: &Path, target: &Path, evidence: &Path) -> Result<()> {
    let identity = run(
        Command::new("cargo")
            .args(["+stable", "pkgid", "--offline", "--locked", "-p", "hibana"])
            .current_dir(root),
        &evidence.join("core-source.log"),
    )?;
    let source = identity.trim();
    let git = source
        .strip_prefix("git+")
        .ok_or("package gate requires the checkout's pinned Git core")?;
    let (repository, revision) = git
        .split_once("?rev=")
        .ok_or("core must use an immutable rev")?;
    let (revision, _) = revision
        .split_once('#')
        .ok_or("incomplete resolved core identity")?;
    if !repository.starts_with("https://")
        || !repository.is_ascii()
        || repository
            .chars()
            .any(|c| c.is_ascii_whitespace() || matches!(c, '"' | '\\'))
        || revision.len() != 40
        || !revision.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err("core identity must contain an HTTPS repository and a full commit hash".into());
    }
    let git_config = format!("patch.crates-io.hibana.git=\"{repository}\"");
    let rev_config = format!("patch.crates-io.hibana.rev=\"{revision}\"");
    fs::write(
        evidence.join("core-patch.toml"),
        format!("[patch.crates-io.hibana]\ngit = \"{repository}\"\nrev = \"{revision}\"\n"),
    )?;
    run(
        Command::new("cargo")
            .args([
                "+stable",
                "package",
                "--locked",
                "--allow-dirty",
                "--config",
                &git_config,
                "--config",
                &rev_config,
            ])
            .current_dir(root)
            .env("CARGO_BUILD_JOBS", "1")
            .env("CARGO_TARGET_DIR", target),
        &evidence.join("package.log"),
    )?;
    let mut archives = Vec::new();
    for item in fs::read_dir(target.join("package"))? {
        let path = item?.path();
        if path
            .extension()
            .is_some_and(|extension| extension == "crate")
        {
            archives.push(path);
        }
    }
    let [archive] = archives.as_slice() else {
        return Err("expected exactly one Cargo archive".into());
    };
    let manifest = target
        .join("package")
        .join(archive.file_stem().ok_or("missing package name")?)
        .join("Cargo.toml");
    let packaged_identity = run(
        Command::new("cargo")
            .args([
                "+stable",
                "pkgid",
                "--offline",
                "--locked",
                "-p",
                "hibana",
                "--manifest-path",
            ])
            .arg(&manifest)
            .args(["--config", &git_config, "--config", &rev_config])
            .current_dir(root),
        &evidence.join("packaged-core-source.log"),
    )?;
    if packaged_identity.trim() != source {
        return Err("packaged source selected a different Hibana revision".into());
    }
    fs::copy(manifest, evidence.join("packaged-Cargo.toml"))?;
    println!("Cargo archive verified with the checkout's core: {source}");
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
        None => fresh("wasi-package-evidence")?,
    };
    if args.next().is_some() {
        return Err("usage: check-package [NEW_EVIDENCE_DIRECTORY]".into());
    }
    println!("Evidence: {}", evidence.display());
    let target = fresh("wasi-package-target")?;
    let result = check(&root, &target, &evidence);
    let cleanup = fs::remove_dir_all(&target);
    match (result, cleanup) {
        (Ok(()), Ok(())) => {
            println!("Temporary Cargo products removed");
            Ok(())
        }
        (Err(error), Ok(())) => Err(error),
        (Ok(()), Err(error)) => Err(error.into()),
        (Err(error), Err(cleanup)) => {
            Err(format!("{error}; target cleanup also failed: {cleanup}").into())
        }
    }
}
