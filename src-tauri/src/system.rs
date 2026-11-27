// Eifel Mastering Central - small questions about the computer (step 7 + "Send feedback").
//   running_daws  - which DAWs are open right now (footer LED, warning before update / remove)
//   system_text   - "macOS 26.1 (Apple Silicon)" / "Windows 11 26100.2314 (x64)" for the feedback form
//   is_feedback_url - the ONLY addresses Central opens in the browser
// No Tauri dependency, so the tests run with plain `cargo test`.

use std::process::Command;

/// The only web address Central opens in the browser: a new issue in Recky's (private) feedback repo.
pub const FEEDBACK_PREFIX: &str = "https://github.com/ReckyReck/eifel-feedback/issues/new";

/// True only for FEEDBACK_PREFIX, optionally followed by "?" and a URL-encoded query (only what JavaScript's
/// encodeURIComponent leaves as it is, plus & and =: no spaces, double quotes, "#" or "@"), at most 6000 characters.
pub fn is_feedback_url(url: &str) -> bool {
    let Some(rest) = url.strip_prefix(FEEDBACK_PREFIX) else { return false };
    if url.len() > 6000 {
        return false;
    }
    if rest.is_empty() {
        return true;
    }
    let Some(query) = rest.strip_prefix('?') else { return false };
    query.bytes().all(|b| b.is_ascii_alphanumeric() || b"-._~%&=+'()!*".contains(&b))
}

/// Names of DAWs, matched at the start of the app name (Mac: "Logic Pro.app", Windows: "Cubase14.exe"), lower case.
const DAWS: &[(&str, &str)] = &[
    ("logic pro", "Logic Pro"),
    ("mainstage", "MainStage"),
    ("garageband", "GarageBand"),
    ("ableton live", "Ableton Live"),
    ("cubase", "Cubase"),
    ("nuendo", "Nuendo"),
    ("wavelab", "WaveLab"),
    ("dorico", "Dorico"),
    ("studio one", "Studio One"),
    ("reaper", "REAPER"),
    ("bitwig studio", "Bitwig Studio"),
    ("pro tools", "Pro Tools"),
    ("protools", "Pro Tools"),
    ("fl studio", "FL Studio"),
    ("fl64", "FL Studio"),
    ("reason", "Reason"),
    ("digital performer", "Digital Performer"),
    ("luna", "LUNA"),
    ("waveform", "Waveform"),
    ("ardour", "Ardour"),
    ("mixbus", "Mixbus"),
    ("audition", "Audition"),
];

/// The DAW an app or program name belongs to, if any.
pub fn daw_name(app: &str) -> Option<&'static str> {
    let a = app.trim().to_lowercase();
    let a = a.strip_suffix(".exe").unwrap_or(&a);
    DAWS.iter().find(|(prefix, _)| {
        // "reason" must not match "reasonable helper": after the prefix only the end, a digit, a space or "-"
        a.strip_prefix(prefix).is_some_and(|rest| rest.is_empty() || rest.starts_with(|c: char| c.is_ascii_digit() || c == ' ' || c == '-'))
    }).map(|(_, name)| *name)
}

/// Mac: the app a process belongs to - "/Applications/Logic Pro.app/Contents/MacOS/Logic Pro" -> "Logic Pro".
/// Processes outside an app bundle are ignored.
pub fn app_of_mac_process(path: &str) -> Option<&str> {
    path.split('/').find_map(|part| part.strip_suffix(".app"))
}

/// Windows: the program names in `tasklist /fo csv /nh` output ("Cubase14.exe","1234",...).
pub fn programs_in_tasklist(csv: &str) -> Vec<String> {
    csv.lines()
        .filter_map(|l| l.trim().strip_prefix('"').and_then(|r| r.split('"').next()))
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// Sorted, without duplicates.
pub fn daws_in<'a>(apps: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut out: Vec<String> = apps.into_iter().filter_map(daw_name).map(str::to_string).collect();
    out.sort();
    out.dedup();
    out
}

/// A command without a console window on Windows.
fn quiet(program: &str) -> Command {
    #[allow(unused_mut)]
    let mut c = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    c
}

fn output_of(c: &mut Command) -> Option<String> {
    let out = c.output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Which DAWs are open right now. Empty if it cannot be found out.
pub fn running_daws() -> Vec<String> {
    if cfg!(windows) {
        let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        let text = output_of(quiet(&format!(r"{system_root}\System32\tasklist.exe")).args(["/fo", "csv", "/nh"])).unwrap_or_default();
        let programs = programs_in_tasklist(&text);
        daws_in(programs.iter().map(|s| s.as_str()))
    } else {
        let text = output_of(quiet("/bin/ps").args(["-axo", "comm="])).unwrap_or_default();
        daws_in(text.lines().filter_map(app_of_mac_process))
    }
}

/// "Microsoft Windows [Version 10.0.26100.2314]" (output of `ver`, in any language) -> ("26100.2314", 26100).
pub fn parse_windows_ver(text: &str) -> Option<(String, u32)> {
    text.split(|c: char| !(c.is_ascii_digit() || c == '.')).find_map(|word| {
        let parts: Vec<&str> = word.split('.').collect();
        if parts.len() < 3 || parts.iter().any(|p| p.is_empty()) {
            return None;
        }
        let build: u32 = parts[2].parse().ok()?;
        Some((parts[2..].join("."), build))
    })
}

/// "Windows 11 26100.2314 (x64)". Windows 11 = build 22000 and later (it still says "10.0" itself).
pub fn windows_text(ver_output: &str, native_arch: &str) -> String {
    let arch = if native_arch.to_ascii_uppercase().contains("ARM") { "ARM" } else { "x64" };
    match parse_windows_ver(ver_output) {
        Some((build, n)) => format!("Windows {} {build} ({arch})", if n >= 22000 { "11" } else { "10" }),
        None => format!("Windows ({arch})"),
    }
}

/// "macOS 26.1 (Apple Silicon)".
pub fn mac_text(product_version: &str, apple_silicon: bool) -> String {
    let v = product_version.trim();
    let chip = if apple_silicon { "Apple Silicon" } else { "Intel" };
    if v.is_empty() { format!("macOS ({chip})") } else { format!("macOS {v} ({chip})") }
}

/// The system, for the feedback form's "Operating system version" field.
pub fn system_text() -> String {
    if cfg!(windows) {
        let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        let ver = output_of(quiet(&format!(r"{system_root}\System32\cmd.exe")).args(["/c", "ver"])).unwrap_or_default();
        // The machine's own processor (Central itself may run as x64 under Windows 11's emulation on ARM).
        let reg = output_of(quiet(&format!(r"{system_root}\System32\reg.exe")).args([
            "query",
            r"HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\Environment",
            "/v",
            "PROCESSOR_ARCHITECTURE",
        ]))
        .unwrap_or_else(|| std::env::consts::ARCH.to_string());
        windows_text(&ver, &reg)
    } else {
        let v = output_of(quiet("/usr/bin/sw_vers").arg("-productVersion")).unwrap_or_default();
        // hw.optional.arm64 is 1 on Apple Silicon, also when Central runs as Intel code under Rosetta
        let arm = output_of(quiet("/usr/sbin/sysctl").args(["-n", "hw.optional.arm64"])).is_some_and(|s| s.trim() == "1")
            || std::env::consts::ARCH == "aarch64";
        mac_text(&v, arm)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feedback_urls() {
        assert!(is_feedback_url(FEEDBACK_PREFIX));
        assert!(is_feedback_url(&format!(
            "{FEEDBACK_PREFIX}?template=bug.yml&title=%5BBug%5D%20Recky's%20Rider&labels=type%3A%20bug%2Cstatus%3A%20new"
        )));
        for bad in [
            "https://github.com/ReckyReck/eifel-feedback/issues",
            "https://github.com/ReckyReck/eifel-feedback/issues/newer",
            "https://github.com/ReckyReck/other/issues/new",
            "http://github.com/ReckyReck/eifel-feedback/issues/new",
            "https://github.com/ReckyReck/eifel-feedback/issues/new/../../x",
            "https://github.com/ReckyReck/eifel-feedback/issues/new?a=b c",
            "https://github.com/ReckyReck/eifel-feedback/issues/new?a=\"x\"",
            "https://github.com/ReckyReck/eifel-feedback/issues/new?a=b#x",
            "https://github.com/ReckyReck/eifel-feedback/issues/new@evil.example",
            "file:///etc/passwd",
        ] {
            assert!(!is_feedback_url(bad), "{bad}");
        }
        // encodeURIComponent leaves ' as it is ("Recky's"); too long is refused
        assert!(!is_feedback_url(&format!("{FEEDBACK_PREFIX}?{}", "a".repeat(7000))));
    }

    #[test]
    fn finds_daws() {
        assert_eq!(app_of_mac_process("/Applications/Logic Pro.app/Contents/MacOS/Logic Pro"), Some("Logic Pro"));
        assert_eq!(app_of_mac_process("/usr/libexec/coreaudiod"), None);
        let ps = "/sbin/launchd\n/Applications/Logic Pro.app/Contents/MacOS/Logic Pro\n\
                  /Applications/Ableton Live 12 Suite.app/Contents/MacOS/Live\n\
                  /Applications/Logic Pro.app/Contents/XPCServices/x.xpc/Contents/MacOS/x\n\
                  /Applications/Reasonable.app/Contents/MacOS/Reasonable\n/Applications/Safari.app/Contents/MacOS/Safari";
        assert_eq!(daws_in(ps.lines().filter_map(app_of_mac_process)), vec!["Ableton Live", "Logic Pro"]);

        let tasklist = "\"System Idle Process\",\"0\",\"Services\",\"0\",\"8 K\"\r\n\"Cubase14.exe\",\"4242\",\"Console\",\"1\",\"900,000 K\"\r\n\
                        \"Nuendo 13.exe\",\"1\",\"Console\",\"1\",\"1 K\"\r\n\"reaper.exe\",\"2\",\"Console\",\"1\",\"1 K\"\r\n\"explorer.exe\",\"3\",\"Console\",\"1\",\"1 K\"\r\n";
        let progs = programs_in_tasklist(tasklist);
        assert_eq!(progs[1], "Cubase14.exe");
        assert_eq!(daws_in(progs.iter().map(|s| s.as_str())), vec!["Cubase", "Nuendo", "REAPER"]);
        assert_eq!(daw_name("FL64.exe"), Some("FL Studio"));
        assert_eq!(daw_name("ReasonableHelper.exe"), None);
        assert_eq!(daw_name("lunatic"), None);
    }

    #[test]
    fn system_texts() {
        assert_eq!(mac_text("26.1\n", true), "macOS 26.1 (Apple Silicon)");
        assert_eq!(mac_text("15.7", false), "macOS 15.7 (Intel)");
        let ver = "\r\nMicrosoft Windows [Version 10.0.26100.2314]\r\n";
        assert_eq!(parse_windows_ver(ver), Some(("26100.2314".into(), 26100)));
        let reg_arm = "\r\nHKEY_LOCAL_MACHINE\\SYSTEM\\...\r\n    PROCESSOR_ARCHITECTURE    REG_SZ    ARM64\r\n";
        let reg_x64 = "    PROCESSOR_ARCHITECTURE    REG_SZ    AMD64\r\n";
        assert_eq!(windows_text(ver, reg_arm), "Windows 11 26100.2314 (ARM)");
        assert_eq!(windows_text(ver, reg_x64), "Windows 11 26100.2314 (x64)");
        assert_eq!(windows_text("Microsoft Windows [Version 10.0.19045.5131]", "AMD64"), "Windows 10 19045.5131 (x64)");
        assert_eq!(windows_text("", "x86_64"), "Windows (x64)");
        assert_eq!(windows_text("Microsoft Windows [Versión 10.0.22631.4460]", "AMD64"), "Windows 11 22631.4460 (x64)");
    }
}
