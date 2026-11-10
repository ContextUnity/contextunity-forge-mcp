use anyhow::{bail, Result};
use serde_json::Value;

#[cfg(any(target_os = "linux", target_os = "macos"))]
/// Sends SIGHUP to matching servers and returns their process IDs.
pub fn run() -> Result<Value> {
    let executable = std::env::current_exe()?;
    // SAFETY: geteuid has no pointer arguments or caller preconditions.
    let uid = unsafe { libc::geteuid() };
    let mut signaled = Vec::new();
    for pid in matching_servers(&executable, uid)? {
        // SAFETY: discovery admitted this PID through executable, user and CLI checks.
        if unsafe { libc::kill(pid, libc::SIGHUP) } == 0 {
            signaled.push(pid);
        } else {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() != Some(libc::ESRCH) {
                return Err(error.into());
            }
        }
    }
    if signaled.is_empty() {
        bail!("no running contextunity-forge-mcp MCP server found for this executable and user");
    }
    Ok(serde_json::json!({"signaled_pids": signaled}))
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn server_invocation(argv: Vec<std::ffi::OsString>) -> bool {
    use clap::Parser;
    !argv.is_empty()
        && super::Cli::try_parse_from(argv)
            .is_ok_and(|cli| matches!(cli.command, None | Some(super::Command::Serve)))
}

#[cfg(target_os = "linux")]
fn matching_servers(executable: &std::path::Path, uid: libc::uid_t) -> Result<Vec<i32>> {
    use std::os::unix::{
        ffi::{OsStrExt, OsStringExt},
        fs::MetadataExt,
    };

    let identity = |metadata: &std::fs::Metadata| (metadata.dev(), metadata.ino());
    let current_image = identity(&std::fs::metadata("/proc/self/exe")?);
    let mut matches = Vec::new();
    for entry in std::fs::read_dir("/proc")? {
        let entry = entry?;
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<i32>().ok())
        else {
            continue;
        };
        if pid == std::process::id() as i32 {
            continue;
        }
        let proc = entry.path();
        let Ok(metadata) = std::fs::metadata(&proc) else {
            continue;
        };
        if metadata.uid() != uid {
            continue;
        }
        let executable_link = proc.join("exe");
        let Ok(target) = std::fs::read_link(&executable_link) else {
            continue;
        };
        let direct_path = target.as_os_str().as_bytes() == executable.as_os_str().as_bytes();
        if !direct_path
            && target.as_os_str().as_bytes().strip_suffix(b" (deleted)")
                != Some(executable.as_os_str().as_bytes())
        {
            continue;
        }
        let Ok(image) = std::fs::metadata(&executable_link) else {
            continue;
        };
        let image_identity = identity(&image);
        if direct_path {
            if image_identity != current_image {
                continue;
            }
        } else {
            if image.dev() != current_image.0 {
                continue;
            }
            // d_path's deletion marker is ambiguous with literal filenames.
            // A live literal path identifying this image cannot establish a
            // deleted installed dentry. Other retained hardlinks are harmless.
            match std::fs::metadata(&target) {
                Ok(literal) if identity(&literal) != image_identity => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                _ => continue,
            }
        }
        let Ok(cmdline) = std::fs::read(proc.join("cmdline")) else {
            continue;
        };
        let Some(cmdline) = cmdline.strip_suffix(b"\0") else {
            continue;
        };
        if !server_invocation(
            cmdline
                .split(|byte| *byte == 0)
                .map(|arg| std::ffi::OsString::from_vec(arg.to_vec()))
                .collect(),
        ) {
            continue;
        }
        if std::fs::read_link(&executable_link).ok().as_ref() != Some(&target)
            || std::fs::metadata(&executable_link)
                .ok()
                .as_ref()
                .map(identity)
                != Some(image_identity)
        {
            continue;
        }
        matches.push(pid);
    }
    Ok(matches)
}

#[cfg(target_os = "macos")]
fn matching_servers(executable: &std::path::Path, uid: libc::uid_t) -> Result<Vec<i32>> {
    use std::{
        mem::{size_of, MaybeUninit},
        os::unix::ffi::OsStrExt,
        ptr,
    };

    // libproc counts PIDs, not bytes, for this convenience entry point.
    // SAFETY: a null output buffer requests the required process count.
    let count = unsafe { libc::proc_listallpids(ptr::null_mut(), 0) };
    if count <= 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let mut pids = vec![0i32; count as usize];
    loop {
        let bytes = i32::try_from(
            pids.len()
                .checked_mul(size_of::<i32>())
                .ok_or_else(|| anyhow::anyhow!("process inventory size overflow"))?,
        )?;
        // SAFETY: pids is initialized and writable for exactly bytes bytes.
        let count = unsafe { libc::proc_listallpids(pids.as_mut_ptr().cast(), bytes) };
        if count <= 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        if (count as usize) < pids.len() {
            pids.truncate(count as usize);
            break;
        }
        let capacity = pids
            .len()
            .checked_mul(2)
            .ok_or_else(|| anyhow::anyhow!("process inventory size overflow"))?;
        pids.resize(capacity, 0);
    }

    let mut argmax: libc::c_int = 0;
    let mut length = size_of::<libc::c_int>();
    let mut mib = [libc::CTL_KERN, libc::KERN_ARGMAX];
    // SAFETY: mib and both output pointers refer to correctly sized live objects.
    if unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            2,
            (&mut argmax as *mut libc::c_int).cast(),
            &mut length,
            ptr::null_mut(),
            0,
        )
    } != 0
        || argmax <= 0
        || length != size_of::<libc::c_int>()
    {
        bail!(
            "cannot obtain macOS process argument limit: {}",
            std::io::Error::last_os_error()
        );
    }
    let mut arguments = vec![0u8; argmax as usize];
    let mut path = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    let mut matches = Vec::new();
    for pid in pids {
        if pid <= 0 || pid == std::process::id() as i32 {
            continue;
        }
        let mut info = MaybeUninit::<libc::proc_bsdinfo>::uninit();
        // SAFETY: info provides enough writable storage for the requested BSD info.
        let copied = unsafe {
            libc::proc_pidinfo(
                pid,
                libc::PROC_PIDTBSDINFO,
                0,
                info.as_mut_ptr().cast(),
                size_of::<libc::proc_bsdinfo>() as i32,
            )
        };
        if copied != size_of::<libc::proc_bsdinfo>() as i32 {
            continue;
        }
        // SAFETY: proc_pidinfo reported a complete initialized proc_bsdinfo value.
        let info = unsafe { info.assume_init() };
        if info.pbi_uid != uid {
            continue;
        }
        path.fill(0);
        // SAFETY: path is writable for its advertised length.
        let copied =
            unsafe { libc::proc_pidpath(pid, path.as_mut_ptr().cast(), path.len() as u32) };
        if copied <= 0 {
            continue;
        }
        let Some(end) = path.iter().position(|byte| *byte == 0) else {
            continue;
        };
        if &path[..end] != executable.as_os_str().as_bytes() {
            continue;
        }
        let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid];
        let mut length = arguments.len();
        // SAFETY: arguments is writable for length bytes; mib has three elements.
        if unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                3,
                arguments.as_mut_ptr().cast(),
                &mut length,
                ptr::null_mut(),
                0,
            )
        } != 0
            || length > arguments.len()
        {
            continue;
        }
        let Some(argv) = darwin_argv(&arguments[..length]) else {
            continue;
        };
        if server_invocation(argv) {
            matches.push(pid);
        }
    }
    Ok(matches)
}

#[cfg(any(target_os = "macos", all(test, unix)))]
fn darwin_argv(bytes: &[u8]) -> Option<Vec<std::ffi::OsString>> {
    use std::os::unix::ffi::OsStringExt;
    // KERN_PROCARGS2: native argc, saved executable path, NUL padding, argc
    // NUL-terminated arguments, then environment. Never parse environment as argv.
    let (count, mut strings) = bytes.split_at_checked(std::mem::size_of::<i32>())?;
    let count = usize::try_from(i32::from_ne_bytes(count.try_into().ok()?)).ok()?;
    if count == 0 || count > strings.len() {
        return None;
    }
    strings = &strings[strings.iter().position(|byte| *byte == 0)? + 1..];
    let first = strings.iter().position(|byte| *byte != 0)?;
    strings = &strings[first..];
    let mut argv = Vec::with_capacity(count);
    for _ in 0..count {
        let end = strings.iter().position(|byte| *byte == 0)?;
        argv.push(std::ffi::OsString::from_vec(strings[..end].to_vec()));
        strings = &strings[end + 1..];
    }
    Some(argv)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
/// Reports that local process discovery is unavailable on this platform.
pub fn run() -> Result<Value> {
    bail!("reload process discovery is supported on Linux and macOS only")
}

#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
mod tests {
    use super::{darwin_argv, server_invocation};

    #[test]
    fn canonical_cli_grammar_selects_only_server_invocations() {
        for (args, expected) in [
            (vec!["forge"], true),
            (vec!["forge", "serve"], true),
            (vec!["forge", "--root", "serve"], true),
            (
                vec!["forge", "--root", "project with spaces", "serve"],
                true,
            ),
            (vec!["forge", "read", "serve"], false),
            (vec!["forge", "--help"], false),
            (vec!["forge", "--version"], false),
            (vec!["forge", "--root"], false),
            (vec!["forge", "unknown-command"], false),
            (vec![], false),
        ] {
            assert_eq!(
                server_invocation(args.iter().map(|arg| (*arg).into()).collect()),
                expected,
                "{args:?}"
            );
        }
    }

    #[test]
    fn darwin_argument_boundary_respects_argc_padding_and_empty_values() {
        let mut bytes = 4i32.to_ne_bytes().to_vec();
        bytes.extend_from_slice(b"/installed/forge\0\0\0forge\0read\0\0serve\0FORGE_FAKE=serve\0");
        let args = darwin_argv(&bytes).unwrap();
        assert_eq!(args, ["forge", "read", "", "serve"]);
        assert!(!server_invocation(args));

        let mut default = 1i32.to_ne_bytes().to_vec();
        default.extend_from_slice(b"/installed/forge\0\0forge\0ignored-environment\0");
        assert!(server_invocation(darwin_argv(&default).unwrap()));

        for corrupt in [
            Vec::new(),
            0i32.to_ne_bytes().to_vec(),
            (-1i32).to_ne_bytes().to_vec(),
            {
                let mut b = 2i32.to_ne_bytes().to_vec();
                b.extend_from_slice(b"/forge\0\0forge\0");
                b
            },
            {
                let mut b = 1i32.to_ne_bytes().to_vec();
                b.extend_from_slice(b"/forge\0\0unterminated");
                b
            },
        ] {
            assert!(darwin_argv(&corrupt).is_none(), "{corrupt:?}");
        }
    }
}
