// Eifel Mastering Central - installing plug-ins on the Mac (step 6).
//
// What happens when the user clicks INSTALL:
//   1. check the request (download address must be one of Recky's catalog releases, names must be plain bundle names)
//   2. download the ZIP over HTTPS into a private temporary folder, with progress, cancellable
//   3. SHA-256 of the download must equal the catalog's value - otherwise stop, nothing is changed
//   4. unpack (entries with ".." or absolute paths are refused), check the expected bundles are there
//   5. ONE shell script does the copy: old bundles are moved aside, new ones copied in, on any error the old ones are
//      put back. For the system folder (/Library/Audio/Plug-Ins) the script runs with ONE administrator prompt.
//   6. installed.json in the app's data folder records what Central installed (id, version, formats, paths, date)
// No quarantine flags are removed: Central's own downloads are not quarantined in the first place.
//
// This file does not depend on Tauri, so its tests run with plain `cargo test`.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// The only place Central downloads plug-ins from.
pub const DOWNLOAD_PREFIX: &str = "https://github.com/ReckyReck/eifel-catalog/releases/download/";
const MAX_UNPACKED_BYTES: u64 = 2 * 1024 * 1024 * 1024; // 2 GB
const MAX_ENTRIES: usize = 50_000;

static BUSY: AtomicBool = AtomicBool::new(false);
static CANCEL: AtomicBool = AtomicBool::new(false);

// ---- types ----------------------------------------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Au,
    Vst3,
    Clap,
}

impl Format {
    pub fn parse(s: &str) -> Option<Format> {
        match s {
            "au" => Some(Format::Au),
            "vst3" => Some(Format::Vst3),
            "clap" => Some(Format::Clap),
            _ => None,
        }
    }
    fn suffix(self) -> &'static str {
        match self {
            Format::Au => ".component",
            Format::Vst3 => ".vst3",
            Format::Clap => ".clap",
        }
    }
    fn folder(self) -> &'static str {
        match self {
            Format::Au => "Components",
            Format::Vst3 => "VST3",
            Format::Clap => "CLAP",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Location {
    System,
    User,
}

/// Where things live. In the app: /Library/Audio/Plug-Ins, ~/Library/Audio/Plug-Ins, the app data folder and the
/// system temp folder. Tests point these at a scratch folder.
#[derive(Clone, Debug)]
pub struct Env {
    pub system_root: PathBuf,
    pub user_root: PathBuf,
    pub data_dir: PathBuf,
    pub temp_dir: PathBuf,
    /// Run the system-folder copy with an administrator prompt (false in tests).
    pub use_admin_prompt: bool,
}

impl Env {
    fn root(&self, loc: Location) -> &Path {
        match loc {
            Location::System => &self.system_root,
            Location::User => &self.user_root,
        }
    }
    fn installed_json(&self) -> PathBuf {
        self.data_dir.join("installed.json")
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InstalledRecord {
    pub id: String,
    pub version: String,
    pub formats: Vec<Format>,
    pub location: Location,
    pub paths: Vec<String>,
    pub installed_at: String,
}

#[derive(Serialize, Deserialize, Default, Debug)]
#[serde(rename_all = "camelCase")]
pub struct InstalledFile {
    #[serde(default = "one")]
    pub schema_version: u32,
    #[serde(default)]
    pub plugins: BTreeMap<String, InstalledRecord>,
}
fn one() -> u32 {
    1
}

/// One plug-in the window asks about (from the catalog).
#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PluginQuery {
    pub id: String,
    #[serde(default)]
    pub bundle_names: BTreeMap<String, String>,
    #[serde(default)]
    pub bundle_id: Option<String>,
}

/// What the window gets back for each plug-in found on disk.
#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InstalledInfo {
    pub id: String,
    pub version: String,
    pub formats: Vec<Format>,
    /// true = installed by Central (listed in installed.json and unchanged since)
    pub by_central: bool,
    pub location: Option<Location>,
    /// every copy found, in both plug-in folders
    pub paths: Vec<String>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct InstallRequest {
    pub plugin_id: String,
    pub plugin_name: String,
    pub version: String,
    pub url: String,
    pub sha256: String,
    pub size: u64,
    pub bundle_names: BTreeMap<String, String>,
    #[serde(default)]
    pub bundle_id: Option<String>,
    pub formats: Vec<String>,
    pub location: Location,
}

#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub id: String,
    /// download | verify | unpack | password | copy | done
    pub phase: &'static str,
    pub done: u64,
    pub total: u64,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct InstallResult {
    pub id: String,
    pub version: String,
    pub formats: Vec<Format>,
    pub location: Location,
    pub paths: Vec<String>,
}

#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase", tag = "kind", content = "message")]
pub enum InstallError {
    /// The user cancelled (Cancel button or the password dialog). Nothing was changed.
    Cancelled,
    /// Another install is still running.
    Busy,
    /// Anything else, with a sentence for the user.
    Failed(String),
}

impl From<std::io::Error> for InstallError {
    fn from(e: std::io::Error) -> Self {
        InstallError::Failed(format!("A file could not be written ({e}). Nothing was installed."))
    }
}

fn fail<T>(msg: impl Into<String>) -> Result<T, InstallError> {
    Err(InstallError::Failed(msg.into()))
}

// ---- small helpers --------------------------------------------------------------------------------------------------

fn is_semver(v: &str) -> bool {
    let parts: Vec<&str> = v.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()) && (p.len() == 1 || !p.starts_with('0')))
}

fn is_plugin_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// A bundle name must be a plain file name like "Reckys Velvet.component".
fn is_bundle_name(name: &str, f: Format) -> bool {
    name.len() > f.suffix().len()
        && name.len() <= 200
        && name.ends_with(f.suffix())
        && !name.starts_with('.')
        && !name.contains('/')
        && !name.contains('\\')
        && !name.contains('\0')
        && name != ".."
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Sorts "0.10.0" after "0.9.0". Unreadable versions count as 0.0.0.
fn version_key(v: &str) -> (u64, u64, u64) {
    let mut it = v.split('.').map(|p| p.trim().parse::<u64>().unwrap_or(0));
    (it.next().unwrap_or(0), it.next().unwrap_or(0), it.next().unwrap_or(0))
}

/// "2026-11-24T22:58:01Z" without extra crates.
fn now_utc() -> String {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    // civil date from days since 1970-01-01 (H. Hinnant's algorithm)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

/// Single-quotes a string for /bin/sh.
fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Escapes a string for an AppleScript "..." literal.
fn applescript_string(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// (identifier, short version) from a bundle's Contents/Info.plist.
fn bundle_info(bundle: &Path) -> (Option<String>, Option<String>) {
    let Ok(value) = plist::Value::from_file(bundle.join("Contents").join("Info.plist")) else {
        return (None, None);
    };
    let Some(dict) = value.as_dictionary() else {
        return (None, None);
    };
    let get = |k: &str| dict.get(k).and_then(|v| v.as_string()).map(|s| s.trim().to_string());
    (get("CFBundleIdentifier"), get("CFBundleShortVersionString"))
}

// ---- installed.json -------------------------------------------------------------------------------------------------

pub fn read_installed(env: &Env) -> InstalledFile {
    fs::read(env.installed_json())
        .ok()
        .and_then(|b| serde_json::from_slice::<InstalledFile>(&b).ok())
        .unwrap_or_else(|| InstalledFile { schema_version: 1, plugins: BTreeMap::new() })
}

fn write_installed(env: &Env, file: &InstalledFile) -> std::io::Result<()> {
    fs::create_dir_all(&env.data_dir)?;
    let tmp = env.data_dir.join("installed.json.tmp");
    let mut f = fs::File::create(&tmp)?;
    f.write_all(&serde_json::to_vec_pretty(file).map_err(std::io::Error::other)?)?;
    f.sync_all()?;
    fs::rename(&tmp, env.installed_json())
}

// ---- what is installed ----------------------------------------------------------------------------------------------

struct Found {
    format: Format,
    location: Location,
    path: PathBuf,
    version: Option<String>,
}

/// For every plug-in of the catalog: is it on disk (system or user folder), which version, and did Central put it
/// there? A bundle on disk that installed.json does not know (or that changed since) is "not by Central".
pub fn get_installed(env: &Env, plugins: &[PluginQuery]) -> Vec<InstalledInfo> {
    let file = read_installed(env);
    let mut out = Vec::new();
    for q in plugins {
        let mut found: Vec<Found> = Vec::new();
        for (fmt_name, bundle) in &q.bundle_names {
            let Some(format) = Format::parse(fmt_name) else { continue };
            if !is_bundle_name(bundle, format) {
                continue;
            }
            for location in [Location::System, Location::User] {
                let path = env.root(location).join(format.folder()).join(bundle);
                if !path.is_dir() {
                    continue;
                }
                let (bid, version) = bundle_info(&path);
                // Same file name but a different plug-in inside: not ours, ignore it.
                if let (Some(want), Some(have)) = (&q.bundle_id, &bid) {
                    if want != have {
                        continue;
                    }
                }
                found.push(Found { format, location, path, version });
            }
        }
        if found.is_empty() {
            continue;
        }

        let record = file.plugins.get(&q.id);
        let central_ok = record.is_some_and(|r| {
            !r.paths.is_empty()
                && r.paths.iter().all(|p| {
                    let p = Path::new(p);
                    p.is_dir() && bundle_info(p).1.as_deref() == Some(r.version.as_str())
                })
        });

        let mut formats: Vec<Format> = found.iter().map(|f| f.format).collect();
        formats.sort();
        formats.dedup();
        let paths: Vec<String> = found.iter().map(|f| f.path.to_string_lossy().into_owned()).collect();

        let info = match (central_ok, record) {
            (true, Some(r)) => InstalledInfo {
                id: q.id.clone(),
                version: r.version.clone(),
                formats,
                by_central: true,
                location: Some(r.location),
                paths,
            },
            _ => {
                let best = found.iter().max_by_key(|f| version_key(f.version.as_deref().unwrap_or("0.0.0")));
                InstalledInfo {
                    id: q.id.clone(),
                    version: best.and_then(|f| f.version.clone()).unwrap_or_else(|| "?".into()),
                    formats,
                    by_central: false,
                    location: best.map(|f| f.location),
                    paths,
                }
            }
        };
        out.push(info);
    }
    out
}

// ---- install --------------------------------------------------------------------------------------------------------

struct Checked {
    targets: Vec<(Format, String)>, // format, bundle name
}

fn check_request(r: &InstallRequest) -> Result<Checked, InstallError> {
    if !is_plugin_id(&r.plugin_id) {
        return fail("This plug-in entry in the catalog is not valid (id). Nothing was installed.");
    }
    if !is_semver(&r.version) {
        return fail("This plug-in entry in the catalog is not valid (version). Nothing was installed.");
    }
    let rest = r.url.strip_prefix(DOWNLOAD_PREFIX).unwrap_or("");
    if rest.is_empty() || rest.contains("..") || rest.contains('?') || rest.contains('#') || rest.contains('\\') {
        return fail("The download address is not one of Recky's catalog releases, so Central will not use it.");
    }
    if r.sha256.len() != 64 || !r.sha256.chars().all(|c| c.is_ascii_hexdigit()) {
        return fail("The catalog has no valid checksum for this download, so Central will not install it.");
    }
    if r.size == 0 || r.size > MAX_UNPACKED_BYTES {
        return fail("The catalog gives no valid size for this download. Nothing was installed.");
    }
    let mut targets = Vec::new();
    for f in &r.formats {
        let Some(format) = Format::parse(f) else {
            return fail(format!("Unknown plug-in format \"{f}\". Nothing was installed."));
        };
        let Some(name) = r.bundle_names.get(f) else {
            return fail(format!("The catalog does not name the {} file of this plug-in.", f.to_uppercase()));
        };
        if !is_bundle_name(name, format) {
            return fail("The catalog names a plug-in file Central does not accept. Nothing was installed.");
        }
        if !targets.iter().any(|(t, _)| *t == format) {
            targets.push((format, name.clone()));
        }
    }
    if targets.is_empty() {
        return fail("No plug-in format selected (Settings > Formats). Nothing was installed.");
    }
    Ok(Checked { targets })
}

/// Removes the temporary folder when the install ends, however it ends.
struct WorkDir(PathBuf);
impl Drop for WorkDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct BusyGuard;
impl Drop for BusyGuard {
    fn drop(&mut self) {
        BUSY.store(false, Ordering::SeqCst);
    }
}

pub fn cancel() {
    CANCEL.store(true, Ordering::SeqCst);
}

fn cancelled() -> bool {
    CANCEL.load(Ordering::SeqCst)
}

/// Downloads `url` into `dest`, returns the SHA-256 (hex). Stops if more bytes come than the catalog announced.
fn download(url: &str, expected_size: u64, dest: &Path, progress: &dyn Fn(u64, u64)) -> Result<String, InstallError> {
    let agent = ureq::AgentBuilder::new()
        .https_only(true)
        .timeout_connect(Duration::from_secs(20))
        .timeout_read(Duration::from_secs(60))
        .redirects(5)
        .user_agent(concat!("EifelMasteringCentral/", env!("CARGO_PKG_VERSION")))
        .build();
    let resp = match agent.get(url).call() {
        Ok(r) => r,
        Err(ureq::Error::Status(code, _)) => {
            return fail(format!("The download is not available (server answered {code}). Please try again later."))
        }
        Err(e) => return fail(format!("The download could not start - check the internet connection. ({e})")),
    };
    let mut reader = resp.into_reader();
    let mut file = fs::File::create(dest)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    let mut done: u64 = 0;
    let mut last = Instant::now() - Duration::from_secs(1);
    progress(0, expected_size);
    loop {
        if cancelled() {
            return Err(InstallError::Cancelled);
        }
        let n = match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return fail(format!("The download was interrupted - check the internet connection. ({e})")),
        };
        done += n as u64;
        if done > expected_size {
            return fail("The download is larger than the catalog says, so Central stopped. Nothing was installed.");
        }
        hasher.update(&buf[..n]);
        file.write_all(&buf[..n])?;
        if last.elapsed() >= Duration::from_millis(100) {
            progress(done, expected_size);
            last = Instant::now();
        }
    }
    file.sync_all()?;
    progress(done, expected_size);
    if done != expected_size {
        return fail("The download is incomplete. Please try again. Nothing was installed.");
    }
    Ok(hex(&hasher.finalize()))
}

/// Unpacks `zip_path` into `dest`. Refuses absolute paths, "..", odd symlinks and archives that are too large.
pub fn unpack(zip_path: &Path, dest: &Path) -> Result<(), InstallError> {
    let bad = |what: &str| InstallError::Failed(format!("The downloaded file is not a valid plug-in archive ({what}). Nothing was installed."));
    let file = fs::File::open(zip_path)?;
    let mut archive = zip::ZipArchive::new(file).map_err(|_| bad("not a ZIP"))?;
    if archive.len() > MAX_ENTRIES {
        return Err(bad("too many files"));
    }
    fs::create_dir_all(dest)?;
    let mut total: u64 = 0;
    for i in 0..archive.len() {
        if cancelled() {
            return Err(InstallError::Cancelled);
        }
        let mut entry = archive.by_index(i).map_err(|_| bad("damaged entry"))?;
        let raw = entry.name().to_string();
        if raw.starts_with("__MACOSX/") {
            continue;
        }
        if raw.starts_with('/') || raw.contains('\\') || raw.contains('\0') || raw.split('/').any(|c| c == "..") {
            return Err(bad(&format!("unsafe path \"{raw}\"")));
        }
        let rel = Path::new(raw.trim_end_matches('/'));
        if rel.components().any(|c| !matches!(c, Component::Normal(_))) {
            return Err(bad(&format!("unsafe path \"{raw}\"")));
        }
        if rel.as_os_str().is_empty() {
            continue;
        }
        let out = dest.join(rel);
        let mode = entry.unix_mode();
        let is_link = mode.is_some_and(|m| m & 0o170000 == 0o120000);
        if entry.is_dir() {
            fs::create_dir_all(&out)?;
            continue;
        }
        if let Some(parent) = out.parent() {
            fs::create_dir_all(parent)?;
        }
        if is_link {
            let mut target = String::new();
            entry.take(4096).read_to_string(&mut target).map_err(|_| bad("damaged link"))?;
            let t = Path::new(&target);
            if target.is_empty() || t.is_absolute() || t.components().any(|c| !matches!(c, Component::Normal(_) | Component::CurDir)) {
                return Err(bad(&format!("link pointing outside \"{raw}\"")));
            }
            make_symlink(&target, &out)?;
            continue;
        }
        let mut f = fs::File::create(&out)?;
        let left = MAX_UNPACKED_BYTES.saturating_sub(total);
        let n = std::io::copy(&mut (&mut entry).take(left + 1), &mut f).map_err(|_| bad("damaged data"))?;
        total += n;
        if total > MAX_UNPACKED_BYTES {
            return Err(bad("too large"));
        }
        let fallback = if raw.contains("/Contents/MacOS/") { 0o755 } else { 0o644 };
        set_mode(&out, mode.map(|m| (m & 0o777) | 0o600).unwrap_or(fallback))?;
    }
    Ok(())
}

#[cfg(unix)]
fn make_symlink(target: &str, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}
#[cfg(not(unix))]
fn make_symlink(_target: &str, _link: &Path) -> std::io::Result<()> {
    Err(std::io::Error::other("links are not supported here"))
}

#[cfg(unix)]
fn set_mode(p: &Path, mode: u32) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(p, fs::Permissions::from_mode(mode))
}
#[cfg(not(unix))]
fn set_mode(_p: &Path, _mode: u32) -> std::io::Result<()> {
    Ok(())
}

/// One copy step: new bundle `src` goes to `dest`; an existing `dest` is first moved to `backup`.
pub struct CopyItem {
    pub src: PathBuf,
    pub dest: PathBuf,
    pub backup: PathBuf,
}

/// The shell script that does the copy. All-or-nothing: if any step fails, the new bundles are removed again and
/// the old ones moved back. With `system` the new bundles get owner root:wheel (the script then runs as root).
pub fn copy_script(items: &[CopyItem], backup_dir: &Path, system: bool) -> String {
    let mut s = String::new();
    s.push_str("#!/bin/sh\n# Eifel Mastering Central - copies plug-ins into place (generated, deleted afterwards)\nset -u\n");
    s.push_str("if [ -x /usr/bin/ditto ]; then CP=/usr/bin/ditto; else CP='cp -R'; fi\n");
    for i in 0..items.len() {
        s.push_str(&format!("M{i}=0; C{i}=0\n"));
    }
    s.push_str("rollback() {\n");
    for (i, it) in items.iter().enumerate().rev() {
        let d = sh_quote(&it.dest.to_string_lossy());
        let b = sh_quote(&it.backup.to_string_lossy());
        s.push_str(&format!("  if [ $C{i} = 1 ] || [ $M{i} = 1 ]; then rm -rf {d}; fi\n"));
        s.push_str(&format!("  if [ $M{i} = 1 ]; then mv {b} {d}; fi\n"));
    }
    s.push_str("}\n");
    s.push_str("fail() { echo \"$1\" >&2; rollback; exit 1; }\n");
    s.push_str(&format!("mkdir -p {} || fail 'cannot create the backup folder'\n", sh_quote(&backup_dir.to_string_lossy())));
    for (i, it) in items.iter().enumerate() {
        let src = sh_quote(&it.src.to_string_lossy());
        let d = sh_quote(&it.dest.to_string_lossy());
        let b = sh_quote(&it.backup.to_string_lossy());
        let parent = sh_quote(&it.dest.parent().unwrap_or(Path::new("/")).to_string_lossy());
        let name = it.dest.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let qn = sh_quote(&name);
        s.push_str(&format!("mkdir -p {parent} || fail \"cannot create the folder \"{parent}\n"));
        s.push_str(&format!("if [ -e {d} ] || [ -L {d} ]; then mv {d} {b} || fail \"cannot move the old \"{qn}\" aside\"; M{i}=1; fi\n"));
        s.push_str(&format!("C{i}=1; $CP {src} {d} || fail \"cannot copy \"{qn}\n"));
        if system {
            s.push_str(&format!("chown -R root:wheel {d} || fail \"cannot set the owner of \"{qn}\n"));
        }
        s.push_str(&format!("chmod -R u+rwX,go+rX,go-w {d} || fail \"cannot set the permissions of \"{qn}\n"));
    }
    s.push_str(&format!("rm -rf {}\nexit 0\n", sh_quote(&backup_dir.to_string_lossy())));
    s
}

/// Runs the copy script - with one administrator prompt if `admin`.
fn run_script(script_path: &Path, admin: bool, prompt: &str) -> Result<(), InstallError> {
    use std::process::Command;
    let output = if admin {
        let apple = format!(
            "do shell script \"/bin/sh \" & quoted form of {} with prompt {} with administrator privileges",
            applescript_string(&script_path.to_string_lossy()),
            applescript_string(prompt)
        );
        Command::new("/usr/bin/osascript").arg("-e").arg(apple).output()
    } else {
        Command::new("/bin/sh").arg(script_path).output()
    };
    let output = output.map_err(|e| InstallError::Failed(format!("The copy step could not start ({e}). Nothing was installed.")))?;
    if output.status.success() {
        return Ok(());
    }
    let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if admin && (err.contains("-128") || err.to_lowercase().contains("user canceled")) {
        return Err(InstallError::Cancelled);
    }
    let detail = err.trim_start_matches("0:").trim();
    fail(format!(
        "Copying the plug-in failed ({}). Your previous version, if there was one, has been put back.",
        if detail.is_empty() { "no details" } else { detail }
    ))
}

/// Logic and other hosts ask the per-user AudioComponentRegistrar for Audio Units; restarting it makes a freshly
/// copied .component visible without logging out (it starts again by itself).
fn refresh_audio_units() {
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("/usr/bin/killall").args(["-9", "AudioComponentRegistrar"]).output();
    }
}

/// The whole install. `progress` is called often during the download, once per later phase.
pub fn install(env: &Env, req: &InstallRequest, progress: &dyn Fn(Progress)) -> Result<InstallResult, InstallError> {
    if BUSY.swap(true, Ordering::SeqCst) {
        return Err(InstallError::Busy);
    }
    let _busy = BusyGuard;
    CANCEL.store(false, Ordering::SeqCst);

    let checked = check_request(req)?;
    let report = |phase: &'static str, done: u64, total: u64| progress(Progress { id: req.plugin_id.clone(), phase, done, total });

    // private temporary folder
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let work = env.temp_dir.join(format!("eifel-central-{}-{}-{stamp}", req.plugin_id, std::process::id()));
    fs::create_dir_all(&work)?;
    set_mode(&work, 0o700)?;
    let work = WorkDir(work);

    // 1. download
    let zip_path = work.0.join("download.zip");
    let sha = download(&req.url, req.size, &zip_path, &|d, t| report("download", d, t))?;
    install_downloaded(env, req, &checked, &work.0, &zip_path, &sha, &report)
}

/// Everything after the download: checksum, unpack, copy, installed.json. Separate so the tests can run it offline.
fn install_downloaded(
    env: &Env,
    req: &InstallRequest,
    checked: &Checked,
    work: &Path,
    zip_path: &Path,
    sha: &str,
    report: &dyn Fn(&'static str, u64, u64),
) -> Result<InstallResult, InstallError> {
    report("verify", 0, 1);
    if !sha.eq_ignore_ascii_case(&req.sha256) {
        return fail("The download does not match the catalog's checksum (SHA-256), so it may be damaged or not Recky's file. Nothing was installed.");
    }

    // 2. unpack + check contents
    report("unpack", 0, 1);
    let unpacked = work.join("unpacked");
    unpack(zip_path, &unpacked)?;
    for (_, name) in &checked.targets {
        let b = unpacked.join(name);
        if !b.is_dir() {
            return fail(format!("The download does not contain \"{name}\". Nothing was installed."));
        }
        let (bid, _) = bundle_info(&b);
        if let (Some(want), Some(have)) = (&req.bundle_id, &bid) {
            if want != have {
                return fail(format!("\"{name}\" in the download is a different plug-in ({have}). Nothing was installed."));
            }
        }
    }
    if cancelled() {
        return Err(InstallError::Cancelled);
    }

    // 3. copy (one password prompt for the system folder)
    let root = env.root(req.location).to_path_buf();
    let backup_dir = work.join("previous");
    let items: Vec<CopyItem> = checked
        .targets
        .iter()
        .enumerate()
        .map(|(i, (format, name))| CopyItem {
            src: unpacked.join(name),
            dest: root.join(format.folder()).join(name),
            backup: backup_dir.join(format!("{i}-{name}")),
        })
        .collect();
    let system = req.location == Location::System;
    let admin = system && env.use_admin_prompt;
    let script_path = work.join("copy.sh");
    fs::write(&script_path, copy_script(&items, &backup_dir, admin))?;
    set_mode(&script_path, 0o700)?;
    report(if admin { "password" } else { "copy" }, 0, 1);
    let prompt = format!(
        "Eifel Mastering Central wants to install {} {} into the plug-in folder for all users (/Library/Audio/Plug-Ins).",
        req.plugin_name, req.version
    );
    run_script(&script_path, admin, &prompt)?;
    report("copy", 1, 1);
    if checked.targets.iter().any(|(f, _)| *f == Format::Au) {
        refresh_audio_units();
    }

    // 4. remember it
    let formats: Vec<Format> = checked.targets.iter().map(|(f, _)| *f).collect();
    let paths: Vec<String> = items.iter().map(|it| it.dest.to_string_lossy().into_owned()).collect();
    let mut file = read_installed(env);
    file.schema_version = 1;
    file.plugins.insert(
        req.plugin_id.clone(),
        InstalledRecord {
            id: req.plugin_id.clone(),
            version: req.version.clone(),
            formats: formats.clone(),
            location: req.location,
            paths: paths.clone(),
            installed_at: now_utc(),
        },
    );
    write_installed(env, &file).map_err(|e| {
        InstallError::Failed(format!(
            "The plug-in was installed, but Central could not note it in installed.json ({e}). It will show as \"not by Central\"."
        ))
    })?;
    report("done", 1, 1);
    Ok(InstallResult { id: req.plugin_id.clone(), version: req.version.clone(), formats, location: req.location, paths })
}

// ---- tests ----------------------------------------------------------------------------------------------------------
#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn scratch(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("ec-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn env_in(base: &Path) -> Env {
        Env {
            system_root: base.join("sys/Library/Audio/Plug-Ins"),
            user_root: base.join("home/Library/Audio/Plug-Ins"),
            data_dir: base.join("data"),
            temp_dir: base.join("tmp"),
            use_admin_prompt: false,
        }
    }

    fn info_plist(id: &str, version: &str) -> Vec<u8> {
        let mut d = plist::Dictionary::new();
        d.insert("CFBundleIdentifier".into(), id.into());
        d.insert("CFBundleShortVersionString".into(), version.into());
        let mut out = Vec::new();
        plist::Value::Dictionary(d).to_writer_xml(&mut out).unwrap();
        out
    }

    fn make_bundle(dir: &Path, id: &str, version: &str) {
        fs::create_dir_all(dir.join("Contents/MacOS")).unwrap();
        fs::write(dir.join("Contents/Info.plist"), info_plist(id, version)).unwrap();
        fs::write(dir.join("Contents/MacOS/bin"), b"x").unwrap();
    }

    fn zip_of(entries: &[(&str, &[u8], u32)]) -> Vec<u8> {
        let mut buf = Cursor::new(Vec::new());
        {
            let mut w = zip::ZipWriter::new(&mut buf);
            for (name, data, mode) in entries {
                let opts = zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Deflated)
                    .unix_permissions(*mode);
                if name.ends_with('/') {
                    w.add_directory(*name, opts).unwrap();
                } else {
                    w.start_file(*name, opts).unwrap();
                    w.write_all(data).unwrap();
                }
            }
            w.finish().unwrap();
        }
        buf.into_inner()
    }

    #[test]
    fn checks_requests() {
        let mut names = BTreeMap::new();
        names.insert("au".to_string(), "Reckys Velvet.component".to_string());
        names.insert("vst3".to_string(), "Reckys Velvet.vst3".to_string());
        let ok = InstallRequest {
            plugin_id: "reckys-velvet".into(),
            plugin_name: "Recky's Velvet".into(),
            version: "0.1.0".into(),
            url: format!("{DOWNLOAD_PREFIX}reckys-velvet-v0.1.0/ReckysVelvet-0.1.0-macos.zip"),
            sha256: "a".repeat(64),
            size: 1000,
            bundle_names: names.clone(),
            bundle_id: Some("com.mystudio.comp165".into()),
            formats: vec!["au".into(), "vst3".into()],
            location: Location::User,
        };
        assert!(check_request(&ok).is_ok());
        for url in [
            "http://github.com/ReckyReck/eifel-catalog/releases/download/x.zip",
            "https://github.com/ReckyReck/other/releases/download/x.zip",
            "https://github.com/ReckyReck/eifel-catalog/releases/download/../../x.zip",
            "https://evil.example/https://github.com/ReckyReck/eifel-catalog/releases/download/x.zip",
            DOWNLOAD_PREFIX,
        ] {
            let r = InstallRequest { url: url.into(), ..ok.clone() };
            assert!(check_request(&r).is_err(), "{url}");
        }
        assert!(check_request(&InstallRequest { sha256: "zz".into(), ..ok.clone() }).is_err());
        assert!(check_request(&InstallRequest { version: "1.0".into(), ..ok.clone() }).is_err());
        assert!(check_request(&InstallRequest { plugin_id: "../x".into(), ..ok.clone() }).is_err());
        assert!(check_request(&InstallRequest { formats: vec![], ..ok.clone() }).is_err());
        let mut bad = names.clone();
        bad.insert("au".into(), "../Evil.component".into());
        assert!(check_request(&InstallRequest { bundle_names: bad, ..ok.clone() }).is_err());
        let mut bad = names;
        bad.insert("au".into(), "Velvet.vst3".into());
        assert!(check_request(&InstallRequest { bundle_names: bad, ..ok }).is_err());
    }

    #[test]
    fn unpacks_safely() {
        let base = scratch("unpack");
        let good = zip_of(&[
            ("Reckys Velvet.component/", b"", 0o755),
            ("Reckys Velvet.component/Contents/MacOS/Reckys Velvet", b"bin", 0o755),
            ("Reckys Velvet.component/Contents/Info.plist", b"<plist/>", 0o644),
            ("__MACOSX/._x", b"junk", 0o644),
        ]);
        fs::write(base.join("good.zip"), good).unwrap();
        unpack(&base.join("good.zip"), &base.join("out")).unwrap();
        let bin = base.join("out/Reckys Velvet.component/Contents/MacOS/Reckys Velvet");
        assert_eq!(fs::read(&bin).unwrap(), b"bin");
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(fs::metadata(&bin).unwrap().permissions().mode() & 0o777, 0o755);
        assert!(!base.join("out/__MACOSX").exists());

        // zip crate refuses to write "../" names itself, so patch the bytes of a same-length name
        let mut evil = zip_of(&[("aa/evil.txt", b"x", 0o644)]);
        let needle = b"aa/evil.txt";
        let mut count = 0;
        let mut i = 0;
        while i + needle.len() <= evil.len() {
            if &evil[i..i + needle.len()] == needle {
                evil[i..i + 2].copy_from_slice(b"..");
                count += 1;
            }
            i += 1;
        }
        assert_eq!(count, 2); // local header + central directory
        fs::write(base.join("evil.zip"), evil).unwrap();
        let r = unpack(&base.join("evil.zip"), &base.join("out2"));
        assert!(matches!(r, Err(InstallError::Failed(ref m)) if m.contains("unsafe")), "{r:?}");
        assert!(!base.join("evil.txt").exists());

        fs::write(base.join("notzip.zip"), b"hello").unwrap();
        assert!(unpack(&base.join("notzip.zip"), &base.join("out3")).is_err());
        fs::remove_dir_all(&base).unwrap();
    }

    fn items_for(env: &Env, work: &Path, names: &[(Format, &str)], loc: Location) -> Vec<CopyItem> {
        names
            .iter()
            .enumerate()
            .map(|(i, (f, n))| CopyItem {
                src: work.join("unpacked").join(n),
                dest: env.root(loc).join(f.folder()).join(n),
                backup: work.join("previous").join(format!("{i}-{n}")),
            })
            .collect()
    }

    #[test]
    fn copy_script_replaces_and_rolls_back() {
        let base = scratch("copy");
        let env = env_in(&base);
        let work = base.join("work");
        make_bundle(&work.join("unpacked/V.component"), "com.mystudio.comp165", "0.2.0");
        make_bundle(&work.join("unpacked/V.vst3"), "com.mystudio.comp165", "0.2.0");
        // an old AU is installed already
        let old_au = env.user_root.join("Components/V.component");
        make_bundle(&old_au, "com.mystudio.comp165", "0.1.0");

        let names = [(Format::Au, "V.component"), (Format::Vst3, "V.vst3")];
        let items = items_for(&env, &work, &names, Location::User);
        let script = work.join("copy.sh");
        fs::write(&script, copy_script(&items, &work.join("previous"), false)).unwrap();
        run_script(&script, false, "").unwrap();
        assert_eq!(bundle_info(&old_au).1.as_deref(), Some("0.2.0"));
        assert_eq!(bundle_info(&env.user_root.join("VST3/V.vst3")).1.as_deref(), Some("0.2.0"));
        assert!(!work.join("previous").exists(), "backup is removed after success");

        // second run where the VST3 source is missing: the AU must be rolled back to 0.2.0, the VST3 too
        make_bundle(&work.join("unpacked/V.component"), "com.mystudio.comp165", "0.3.0");
        fs::write(work.join("unpacked/V.component/Contents/Info.plist"), info_plist("com.mystudio.comp165", "0.3.0")).unwrap();
        fs::remove_dir_all(work.join("unpacked/V.vst3")).unwrap();
        fs::write(&script, copy_script(&items, &work.join("previous"), false)).unwrap();
        let r = run_script(&script, false, "");
        assert!(matches!(r, Err(InstallError::Failed(ref m)) if m.contains("put back")), "{r:?}");
        assert_eq!(bundle_info(&old_au).1.as_deref(), Some("0.2.0"), "AU rolled back");
        assert_eq!(bundle_info(&env.user_root.join("VST3/V.vst3")).1.as_deref(), Some("0.2.0"), "VST3 rolled back");

        // a path with a quote in it survives the shell quoting
        assert_eq!(sh_quote("Recky's"), "'Recky'\\''s'");
        assert_eq!(applescript_string("a\"b\\c"), "\"a\\\"b\\\\c\"");
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn finds_installed_bundles() {
        let base = scratch("scan");
        let env = env_in(&base);
        let mut names = BTreeMap::new();
        names.insert("au".to_string(), "V.component".to_string());
        names.insert("vst3".to_string(), "V.vst3".to_string());
        let q = vec![PluginQuery { id: "reckys-velvet".into(), bundle_names: names, bundle_id: Some("com.mystudio.comp165".into()) }];

        assert!(get_installed(&env, &q).is_empty());

        // a copy from another installer, in the system folder
        make_bundle(&env.system_root.join("Components/V.component"), "com.mystudio.comp165", "0.1.0");
        let r = get_installed(&env, &q);
        assert_eq!(r.len(), 1);
        assert!(!r[0].by_central);
        assert_eq!(r[0].version, "0.1.0");
        assert_eq!(r[0].formats, vec![Format::Au]);
        assert_eq!(r[0].location, Some(Location::System));

        // a different plug-in with the same file name is ignored
        make_bundle(&env.user_root.join("VST3/V.vst3"), "com.someone.else", "9.0.0");
        assert_eq!(get_installed(&env, &q)[0].formats, vec![Format::Au]);

        // recorded by Central -> by_central
        let p = env.system_root.join("Components/V.component").to_string_lossy().into_owned();
        let mut file = InstalledFile::default();
        file.plugins.insert(
            "reckys-velvet".into(),
            InstalledRecord {
                id: "reckys-velvet".into(),
                version: "0.1.0".into(),
                formats: vec![Format::Au],
                location: Location::System,
                paths: vec![p],
                installed_at: now_utc(),
            },
        );
        write_installed(&env, &file).unwrap();
        assert!(get_installed(&env, &q)[0].by_central);

        // replaced by something else since -> not by Central any more
        fs::write(env.system_root.join("Components/V.component/Contents/Info.plist"), info_plist("com.mystudio.comp165", "1.0.0")).unwrap();
        let r = get_installed(&env, &q);
        assert!(!r[0].by_central);
        assert_eq!(r[0].version, "1.0.0");
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn whole_install_offline() {
        let base = scratch("whole");
        let env = env_in(&base);
        let plist_bytes = info_plist("com.mystudio.comp165", "0.1.0");
        let data = zip_of(&[
            ("V.component/Contents/Info.plist", &plist_bytes, 0o644),
            ("V.component/Contents/MacOS/V", b"bin", 0o755),
            ("V.vst3/Contents/Info.plist", &plist_bytes, 0o644),
            ("V.vst3/Contents/MacOS/V", b"bin", 0o755),
        ]);
        let sha = hex(&Sha256::digest(&data));
        let mut names = BTreeMap::new();
        names.insert("au".to_string(), "V.component".to_string());
        names.insert("vst3".to_string(), "V.vst3".to_string());
        let req = InstallRequest {
            plugin_id: "reckys-velvet".into(),
            plugin_name: "Recky's Velvet".into(),
            version: "0.1.0".into(),
            url: format!("{DOWNLOAD_PREFIX}reckys-velvet-v0.1.0/x.zip"),
            sha256: sha.clone(),
            size: data.len() as u64,
            bundle_names: names.clone(),
            bundle_id: Some("com.mystudio.comp165".into()),
            formats: vec!["au".into(), "vst3".into()],
            location: Location::User,
        };
        let checked = check_request(&req).unwrap();
        let work = base.join("work");
        fs::create_dir_all(&work).unwrap();
        let zip_path = work.join("download.zip");
        fs::write(&zip_path, &data).unwrap();
        let phases = std::cell::RefCell::new(Vec::new());
        let report = |p: &'static str, _d: u64, _t: u64| phases.borrow_mut().push(p);

        // wrong checksum: nothing is touched
        let r = install_downloaded(&env, &req, &checked, &work, &zip_path, &"0".repeat(64), &report);
        assert!(matches!(r, Err(InstallError::Failed(ref m)) if m.contains("checksum")), "{r:?}");
        assert!(!env.user_root.exists());

        // right checksum
        let r = install_downloaded(&env, &req, &checked, &work, &zip_path, &sha, &report).unwrap();
        assert_eq!(r.formats, vec![Format::Au, Format::Vst3]);
        assert_eq!(*phases.borrow().last().unwrap(), "done");
        let q = vec![PluginQuery { id: "reckys-velvet".into(), bundle_names: names, bundle_id: Some("com.mystudio.comp165".into()) }];
        let found = get_installed(&env, &q);
        assert_eq!(found.len(), 1);
        assert!(found[0].by_central);
        assert_eq!(found[0].version, "0.1.0");
        assert_eq!(found[0].location, Some(Location::User));
        let rec = &read_installed(&env).plugins["reckys-velvet"];
        assert_eq!(rec.paths.len(), 2);

        // a download whose bundle is another plug-in is refused
        let other = info_plist("com.someone.else", "0.1.0");
        let data2 = zip_of(&[("V.component/Contents/Info.plist", &other, 0o644), ("V.vst3/Contents/Info.plist", &other, 0o644)]);
        fs::write(&zip_path, &data2).unwrap();
        let sha2 = hex(&Sha256::digest(&data2));
        let req2 = InstallRequest { sha256: sha2.clone(), ..req };
        let r = install_downloaded(&env, &req2, &checked, &work, &zip_path, &sha2, &report);
        assert!(matches!(r, Err(InstallError::Failed(ref m)) if m.contains("different plug-in")), "{r:?}");
        fs::remove_dir_all(&base).unwrap();
    }

    /// Runs on GitHub's Mac (workflow "Build app"): downloads every Mac plug-in of the LIVE catalog, checks its
    /// SHA-256, unpacks it and checks the bundles inside. Installs nothing.  cargo test -- --include-ignored
    #[test]
    #[ignore]
    fn real_catalog_downloads() {
        let text = ureq::get("https://reckyreck.github.io/eifel-catalog/catalog.json")
            .call()
            .expect("catalog")
            .into_string()
            .expect("catalog text");
        let catalog: serde_json::Value = serde_json::from_str(&text).expect("catalog json");
        let base = scratch("real");
        let mut checked_any = false;
        for p in catalog["plugins"].as_array().expect("plugins") {
            let id = p["id"].as_str().unwrap();
            let bundle_id = p["bundleId"].as_str().map(String::from);
            for a in p["latest"]["assets"].as_array().unwrap() {
                if a["platform"] != "macos" {
                    continue;
                }
                let url = a["url"].as_str().unwrap();
                assert!(url.starts_with(DOWNLOAD_PREFIX), "{url}");
                let size = a["size"].as_u64().unwrap();
                let zip_path = base.join(format!("{id}.zip"));
                let sha = download(url, size, &zip_path, &|_, _| {}).expect("download");
                assert_eq!(sha, a["sha256"].as_str().unwrap(), "{id}: checksum");
                let out = base.join(id);
                unpack(&zip_path, &out).expect("unpack");
                for f in a["contains"].as_array().unwrap() {
                    let name = p["bundleNames"][f.as_str().unwrap()].as_str().unwrap();
                    let b = out.join(name);
                    assert!(b.join("Contents/Info.plist").is_file(), "{id}: {name} missing");
                    let (bid, ver) = bundle_info(&b);
                    if let Some(want) = &bundle_id {
                        assert_eq!(bid.as_deref(), Some(want.as_str()), "{id}: {name} bundle id");
                    }
                    assert_eq!(ver.as_deref(), p["latest"]["version"].as_str(), "{id}: {name} version");
                    #[cfg(target_os = "macos")]
                    {
                        let st = std::process::Command::new("/usr/bin/codesign").args(["--verify", "--deep", "--strict"]).arg(&b).status().unwrap();
                        assert!(st.success(), "{id}: {name} signature broken after unpacking");
                    }
                    println!("OK {id} {name} {}", ver.unwrap_or_default());
                    checked_any = true;
                }
            }
        }
        assert!(checked_any, "no Mac plug-in in the catalog");
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn small_helpers() {
        assert!(is_semver("0.1.0") && is_semver("10.20.30"));
        assert!(!is_semver("01.0.0") && !is_semver("1.0") && !is_semver("1.0.0-beta"));
        assert!(version_key("0.10.0") > version_key("0.9.9"));
        let t = now_utc();
        assert_eq!(t.len(), 20);
        assert!(t.starts_with("20") && t.ends_with('Z'));
        assert!(is_bundle_name("Reckys Velvet.vst3", Format::Vst3));
        assert!(!is_bundle_name(".vst3", Format::Vst3));
        assert!(!is_bundle_name("a/b.vst3", Format::Vst3));
    }
}
