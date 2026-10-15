use std::{
    fs::{self, OpenOptions},
    io::{self, Read, Write},
};

fn main() {
    std::process::exit(run_app());
}

fn run_app() -> i32 {
    if write_stdout(b"wasi std shell app\n").is_err() {
        return 1;
    }
    if print_prompt().is_err() {
        return 1;
    }

    let mut input = [0u8; 64];
    loop {
        let read = match io::stdin().read(&mut input) {
            Ok(0) => return 0,
            Ok(read) => read,
            Err(_) => return 1,
        };
        let line = trim_line(&input[..read]);
        if line.is_empty() {
            if print_prompt().is_err() {
                return 1;
            }
            continue;
        }
        if line == b"exit" {
            return 0;
        }
        if run_command(line).is_err() {
            let _ = print_usage();
        }
        if print_prompt().is_err() {
            return 1;
        }
    }
}

fn run_command(line: &[u8]) -> Result<(), ()> {
    if line == b"help" {
        return print_help().map_err(|_| ());
    }
    if let Some(path) = strip_prefix(line, b"ls ") {
        return list(as_utf8(trim_ascii(path))?);
    }
    if let Some(path) = strip_prefix(line, b"cat ") {
        return cat(as_utf8(trim_ascii(path))?);
    }
    if let Some(command) = strip_prefix(line, b"echo ")
        && let Some((text, path)) = split_once(command, b" > ")
    {
        return write_text(text, as_utf8(trim_ascii(path))?);
    }
    if let Some(command) = strip_prefix(line, b"apply ")
        && let Some((source, target)) = split_once(command, b" ")
    {
        return apply(as_utf8(trim_ascii(source))?, as_utf8(trim_ascii(target))?);
    }
    Err(())
}

fn print_help() -> io::Result<()> {
    write_stdout(b"commands:\n")?;
    write_stdout(b"  help\n")?;
    write_stdout(b"  ls /objects\n")?;
    write_stdout(b"  cat /objects/log\n")?;
    write_stdout(b"  echo 1 > ")?;
    write_stdout(b"/outputs/led/green\n")?;
    write_stdout(b"  apply /objects/log ")?;
    write_stdout(b"/outputs/led/green\n")?;
    write_stdout(b"  exit\n")
}

fn print_usage() -> io::Result<()> {
    write_stderr(b"usage: help | ls PATH | cat PATH | echo TEXT > PATH | ")?;
    write_stderr(b"apply SOURCE TARGET | exit\n")
}

fn print_prompt() -> io::Result<()> {
    write_stdout(b"wasi> ")
}

#[link(wasm_import_module = "wasi_snapshot_preview1")]
unsafe extern "C" {
    #[link_name = "path_open"]
    fn wasi_path_open(
        fd: u32,
        lookup: u32,
        path: *const u8,
        len: u32,
        oflags: u32,
        base: u64,
        inheriting: u64,
        flags: u32,
        opened: *mut u32,
    ) -> u32;
    #[link_name = "fd_readdir"]
    fn wasi_readdir(fd: u32, buf: *mut u8, len: u32, cookie: u64, used: *mut u32) -> u32;
    #[link_name = "fd_fdstat_get"]
    fn wasi_fdstat(fd: u32, stat: *mut u8) -> u32;
    #[link_name = "fd_close"]
    fn wasi_close(fd: u32) -> u32;
}

fn list(path: &str) -> Result<(), ()> {
    let path = path.strip_prefix('/').unwrap_or(path).as_bytes();
    let mut stat = [0u8; 24];
    // SAFETY: stat is the complete WASI fdstat output region.
    if unsafe { wasi_fdstat(3, stat.as_mut_ptr()) } != 0 {
        return Err(());
    }
    let rights = u64::from_le_bytes(stat[8..16].try_into().map_err(|_| ())?);
    if stat[0] != 3 || rights & (1 << 14) == 0 {
        return Err(());
    }
    let mut fd = 0;
    // SAFETY: path and opened fd are live guest-memory regions. The bounded
    // runtime still checks their ranges and ChoreoFS admits the directory.
    if unsafe {
        wasi_path_open(
            3,
            0,
            path.as_ptr(),
            path.len() as u32,
            2,
            1 << 14,
            0,
            4,
            &mut fd,
        )
    } != 0
    {
        return Err(());
    }
    let result = list_open(fd);
    // SAFETY: the descriptor was returned by the matching successful path_open.
    if unsafe { wasi_close(fd) } != 0 {
        return Err(());
    }
    result
}
fn list_open(fd: u32) -> Result<(), ()> {
    let mut cookie = 0;
    loop {
        let mut bytes = [0u8; 96];
        let mut used = 0;
        // SAFETY: both output regions are valid and disjoint. A 96-byte request
        // respects this runtime's bounded directory-completion profile.
        if unsafe { wasi_readdir(fd, bytes.as_mut_ptr(), 96, cookie, &mut used) } != 0 {
            return Err(());
        }
        let mut offset = 0;
        while offset + 24 <= used as usize {
            let len =
                u32::from_le_bytes(bytes[offset + 16..offset + 20].try_into().map_err(|_| ())?)
                    as usize;
            let end = offset + 24 + len;
            if end > used as usize {
                break;
            }
            cookie = u64::from_le_bytes(bytes[offset..offset + 8].try_into().map_err(|_| ())?);
            write_stdout(&bytes[offset + 24..end]).map_err(|_| ())?;
            write_stdout(b"\n").map_err(|_| ())?;
            offset = end;
        }
        if used < 96 {
            return Ok(());
        }
        if offset == 0 {
            return Err(());
        }
    }
}

fn cat(path: &str) -> Result<(), ()> {
    let text = fs::read(path).map_err(|_| ())?;
    write_stdout(&text).map_err(|_| ())
}

fn write_text(text: &[u8], path: &str) -> Result<(), ()> {
    let mut object = match OpenOptions::new().write(true).open(path) {
        Ok(object) => object,
        Err(error) => {
            write_stderr(b"write denied errno=").map_err(|_| ())?;
            write_errno(error.raw_os_error()).map_err(|_| ())?;
            write_stderr(b"\n").map_err(|_| ())?;
            return Ok(());
        }
    };
    if let Err(error) = object.write_all(text) {
        print_write_denied(error).map_err(|_| ())?;
        return Ok(());
    }
    if let Err(error) = object.write_all(b"\n") {
        print_write_denied(error).map_err(|_| ())?;
    }
    Ok(())
}

fn apply(source_path: &str, target_path: &str) -> Result<(), ()> {
    let metadata = fs::metadata(source_path).map_err(|_| ())?;
    let source = fs::read(source_path).map_err(|_| ())?;
    if metadata.len() != source.len() as u64 {
        return Err(());
    }
    let mut target = OpenOptions::new()
        .write(true)
        .open(target_path)
        .map_err(|_| ())?;
    target.write_all(b"1").map_err(|_| ())?;
    target.write_all(b"\n").map_err(|_| ())?;
    write_stdout(b"applied\n").map_err(|_| ())
}

fn as_utf8(bytes: &[u8]) -> Result<&str, ()> {
    std::str::from_utf8(bytes).map_err(|_| ())
}

fn trim_line(bytes: &[u8]) -> &[u8] {
    trim_ascii(bytes.trim_ascii_end())
}

fn trim_ascii(bytes: &[u8]) -> &[u8] {
    let mut start = 0usize;
    let mut end = bytes.len();
    while start < end && bytes[start] == b' ' {
        start += 1;
    }
    while end > start && bytes[end - 1] == b' ' {
        end -= 1;
    }
    &bytes[start..end]
}

fn strip_prefix<'a>(bytes: &'a [u8], prefix: &[u8]) -> Option<&'a [u8]> {
    bytes.get(..prefix.len()).filter(|head| *head == prefix)?;
    bytes.get(prefix.len()..)
}

fn split_once<'a>(bytes: &'a [u8], needle: &[u8]) -> Option<(&'a [u8], &'a [u8])> {
    let index = bytes
        .windows(needle.len())
        .position(|window| window == needle)?;
    Some((&bytes[..index], &bytes[index + needle.len()..]))
}

fn write_stdout(bytes: &[u8]) -> io::Result<()> {
    let mut stdout = io::stdout();
    stdout.write_all(bytes)?;
    stdout.flush()
}

fn write_stderr(bytes: &[u8]) -> io::Result<()> {
    let mut stderr = io::stderr();
    stderr.write_all(bytes)?;
    stderr.flush()
}

fn print_write_denied(error: io::Error) -> io::Result<()> {
    write_stderr(b"write denied errno=")?;
    write_errno(error.raw_os_error())?;
    write_stderr(b"\n")
}

fn write_errno(errno: Option<i32>) -> io::Result<()> {
    let Some(errno) = errno else {
        return write_stderr(b"unknown");
    };
    write_decimal(errno as u64)
}

fn write_decimal(mut value: u64) -> io::Result<()> {
    let mut bytes = [0u8; 10];
    let mut pos = bytes.len();
    loop {
        pos -= 1;
        bytes[pos] = b'0' + (value % 10) as u8;
        value /= 10;
        if value == 0 {
            break;
        }
    }
    write_stderr(&bytes[pos..])
}
