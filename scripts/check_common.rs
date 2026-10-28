use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};

pub(crate) type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub(crate) fn fresh(prefix: &str) -> Result<PathBuf> {
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let path = env::temp_dir().join(format!("{prefix}-{}-{stamp}", std::process::id()));
    fs::create_dir(&path)?;
    Ok(path)
}

pub(crate) fn run(command: &mut Command, log: &Path) -> Result<String> {
    let file = fs::File::create(log)?;
    let status = command
        .stdout(Stdio::from(file.try_clone()?))
        .stderr(Stdio::from(file))
        .status()?;
    let text = fs::read_to_string(log)?;
    if !status.success() {
        eprint!("{text}");
        return Err(format!(
            "command failed ({status}): {command:?}; log: {}",
            log.display()
        )
        .into());
    }
    Ok(text)
}
