//! Self-update: pull the latest CI build from the `latest` GitHub release.
//!
//! aello uses a rolling release — every push to `main` rebuilds the binaries
//! and replaces a single release tagged `latest`. There are no version tags,
//! so `aello update` always fetches `latest` and replaces the running binary.

use anyhow::{Context, Result};

const RELEASE_API: &str = "https://api.github.com/repos/ryha0008-boop/aello/releases/latest";
const RELEASES_PAGE: &str = "https://github.com/ryha0008-boop/aello/releases";

pub fn run() -> Result<()> {
    let ua = format!("aello/{}", env!("CARGO_PKG_VERSION"));
    print!("Fetching latest build... ");
    std::io::Write::flush(&mut std::io::stdout()).ok();

    let release: serde_json::Value = match ureq::get(RELEASE_API).set("User-Agent", &ua).call() {
        Ok(r) => r.into_json().context("failed to parse release JSON")?,
        Err(e) => {
            println!("failed ({e})");
            println!("Download manually from: {RELEASES_PAGE}");
            return Ok(());
        }
    };

    // Asset naming: aello-<arch>-<os>[.exe]. Only the CI-built targets exist.
    let expected = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => "aello-x86_64-windows.exe",
        ("linux", "x86_64") => "aello-x86_64-linux",
        (os, arch) => {
            return Err(anyhow::anyhow!(
                "no pre-built binary for {arch}-{os} — build from source: cargo install --git https://github.com/ryha0008-boop/aello"
            ))
        }
    };

    let empty = vec![];
    let assets = release["assets"].as_array().unwrap_or(&empty);
    let asset = assets
        .iter()
        .find(|a| a["name"].as_str() == Some(expected))
        .with_context(|| format!("no {expected} in latest release — download from: {RELEASES_PAGE}"))?;

    let url = asset["browser_download_url"].as_str().context("missing download URL")?;
    // CI writes "Rolling build from <sha>" into the release notes; pull the
    // commit from there (target_commitish is just the branch name).
    let sha = release["body"]
        .as_str()
        .and_then(|b| b.split_whitespace().last())
        .filter(|s| s.len() >= 7 && s.chars().all(|c| c.is_ascii_hexdigit()))
        .map(|s| &s[..7])
        .unwrap_or("");
    println!("ok");
    println!("Downloading {expected}{}...", if sha.is_empty() { String::new() } else { format!(" ({sha})") });

    let mut reader = ureq::get(url)
        .set("User-Agent", &ua)
        .call()
        .context("download failed")?
        .into_reader();
    let mut buf = Vec::new();
    std::io::Read::read_to_end(&mut reader, &mut buf).context("failed to read download")?;

    // Guard against a truncated download or an HTML/error page silently becoming
    // the installed binary (a short read isn't an I/O error). The real binaries
    // are multi-MB; anything under 1 MiB is not a valid aello build.
    if buf.len() < 1024 * 1024 {
        anyhow::bail!(
            "download was only {} bytes — likely a network error or error page, not a binary. \
             Try again or download manually from: {RELEASES_PAGE}",
            buf.len()
        );
    }

    let exe = std::env::current_exe().context("could not find current exe path")?;
    replace_binary(&exe, &buf)?;

    println!("Updated. Restart aello to use the new build.");
    Ok(())
}

/// Replace the running binary with `buf`. Windows can't overwrite a running
/// exe, so rename it aside first (cleaned up on next launch); Unix overwrites
/// in place and restores the exec bit.
fn replace_binary(exe: &std::path::Path, buf: &[u8]) -> Result<()> {
    #[cfg(windows)]
    {
        // Rename the running exe aside under a UNIQUE name. A fixed name
        // (aello.exe.old) collides + access-denies when another aello instance
        // is still running from a previous .old. Cleaned up on next launch.
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let mut old = exe.to_path_buf().into_os_string();
        old.push(format!(".old-{nanos}"));
        let old = std::path::PathBuf::from(old);

        std::fs::rename(exe, &old)
            .context("could not rename current binary — close any other running aello, then retry")?;
        if let Err(e) = std::fs::write(exe, buf) {
            let _ = std::fs::rename(&old, exe); // restore on failure
            return Err(e).context("failed to write new binary");
        }
    }
    // Unix can't write() over a running executable (ETXTBSY). Write the new
    // binary to a temp file in the same directory, then rename it over the
    // original — rename swaps the path atomically even while the old binary
    // is still executing.
    #[cfg(not(windows))]
    {
        use std::os::unix::fs::PermissionsExt;
        let dir = exe.parent().unwrap_or_else(|| std::path::Path::new("."));
        let tmp = dir.join(format!(".aello-update-{}", std::process::id()));
        std::fs::write(&tmp, buf).with_context(|| {
            format!(
                "failed to write to {} — need write access to the install dir (use a user-writable PATH dir like ~/.local/bin, or run with sudo)",
                dir.display()
            )
        })?;
        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755));
        if let Err(e) = std::fs::rename(&tmp, exe) {
            let _ = std::fs::remove_file(&tmp);
            return Err(e).context("failed to replace binary — need write access to the install dir");
        }
    }
    Ok(())
}
