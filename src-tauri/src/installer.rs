// Eifel Mastering Central - installing plug-ins on the Mac (step 6) and on Windows (step 8).
//
// What happens when the user clicks INSTALL:
//   1. check the request (download address must be one of Recky's catalog releases, names must be plain bundle names)
//   2. download the ZIP over HTTPS into a private temporary folder, with progress, cancellable
//   3. SHA-256 of the download must equal the catalog's value - otherwise stop, nothing is changed
//   4. unpack (entries with ".." or absolute paths are refused), check the expected bundles are there and are
//      Recky's (Mac: bundle id in Info.plist; Windows: maker code "Mstd" in the VST3 moduleinfo.json)
//   5. the copy: old bundles are moved aside, new ones copied in, on any error the old ones are put back.
//      Mac: ONE shell script; for the system folder (/Library/Audio/Plug-Ins) with ONE administrator prompt.
//      Windows: Central copies itself (copy_into_place); for the system folder (C:\Program Files\Common Files\VST3)
//      it starts a second copy of itself ONCE with "Run as administrator" (one Windows UAC prompt).
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
/// Recky's manufacturer code (never changes). Windows VST3 bundles carry it in their class ids.
pub const MAKER_CODE: &str = "Mstd";
/// Windows: an old bundle is renamed to "<name>.central-old" next to itself while the new one is copied in.
pub const OLD_SUFFIX: &str = ".central-old";
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
    /// true = Windows way: Central copies itself (no shell script), old bundles become "<name>.central-old" next to
    /// themselves; the system folder through the elevated helper. false = Mac way (sh script, osascript).
    pub native_copy: bool,
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
    /// shared name, only for the password prompt when removing
    #[serde(default)]
    pub name: Option<String>,
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

/// What a bundle on disk says about itself.
#[derive(Default, Debug, PartialEq)]
pub struct BundleInfo {
    /// Mac: CFBundleIdentifier from Contents/Info.plist
    pub bundle_id: Option<String>,
    /// Mac: CFBundleShortVersionString; Windows: "Version" from Contents/Resources/moduleinfo.json
    pub version: Option<String>,
    /// Windows: the 4-letter manufacturer code inside the processor's class id (JUCE: ABCDEF01 9182FAEB <maker> <code>)
    pub maker: Option<String>,
}

impl BundleInfo {
    /// Some(description) if the bundle is clearly a different plug-in than the catalog expects.
    fn other_plugin(&self, want_bundle_id: Option<&str>) -> Option<String> {
        match (&self.bundle_id, want_bundle_id, &self.maker) {
            (Some(have), Some(want), _) if have != want => Some(have.clone()),
            (None, _, Some(maker)) if maker != MAKER_CODE => Some(format!("maker code {maker}")),
            _ => None,
        }
    }
}

/// Mac bundles: Contents/Info.plist. Windows VST3 bundles have no Info.plist: Contents/Resources/moduleinfo.json.
fn bundle_info(bundle: &Path) -> BundleInfo {
    let contents = bundle.join("Contents");
    if let Ok(value) = plist::Value::from_file(contents.join("Info.plist")) {
        if let Some(dict) = value.as_dictionary() {
            let get = |k: &str| dict.get(k).and_then(|v| v.as_string()).map(|s| s.trim().to_string());
            return BundleInfo { bundle_id: get("CFBundleIdentifier"), version: get("CFBundleShortVersionString"), maker: None };
        }
    }
    fs::read_to_string(contents.join("Resources").join("moduleinfo.json"))
        .map(|t| module_info(&t))
        .unwrap_or_default()
}

/// Reads a VST3 moduleinfo.json. The VST3 SDK writes it with trailing commas ("JSON5"), which are removed first.
fn module_info(text: &str) -> BundleInfo {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&strip_trailing_commas(text)) else {
        return BundleInfo::default();
    };
    let version = v["Version"].as_str().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    let maker = v["Classes"].as_array().and_then(|classes| {
        classes
            .iter()
            .find(|c| c["Category"] == "Audio Module Class")
            .and_then(|c| c["CID"].as_str())
            .and_then(|cid| cid.get(16..24))
            .and_then(|hex4| {
                let bytes: Option<Vec<u8>> =
                    (0..4).map(|i| u8::from_str_radix(&hex4[i * 2..i * 2 + 2], 16).ok()).collect();
                bytes.and_then(|b| String::from_utf8(b).ok())
            })
    });
    BundleInfo { bundle_id: None, version, maker }
}

/// Removes commas that stand right before } or ] (outside strings).
fn strip_trailing_commas(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let (mut in_str, mut esc) = (false, false);
    for (i, &c) in chars.iter().enumerate() {
        if in_str {
            out.push(c);
            if esc {
                esc = false;
            } else if c == '\\' {
                esc = true;
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        if c == '"' {
            in_str = true;
        } else if c == ',' {
            let next = chars[i + 1..].iter().find(|ch| !ch.is_whitespace());
            if matches!(next, Some('}') | Some(']')) {
                continue;
            }
        }
        out.push(c);
    }
    out
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
                let info = bundle_info(&path);
                // Same file name but a different plug-in inside: not ours, ignore it.
                if info.other_plugin(q.bundle_id.as_deref()).is_some() {
                    continue;
                }
                found.push(Found { format, location, path, version: info.version });
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
                    p.is_dir() && bundle_info(p).version.as_deref() == Some(r.version.as_str())
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
#[derive(Serialize, Deserialize, Debug, Clone)]
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

/// Runs a generated script (copy or remove) - with one administrator prompt if `admin`.
fn run_script(script_path: &Path, admin: bool, prompt: &str, failed: &str, after: &str) -> Result<(), InstallError> {
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
    let output = output.map_err(|e| InstallError::Failed(format!("{failed}: the step could not start ({e}). Nothing was changed.")))?;
    if output.status.success() {
        return Ok(());
    }
    let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if admin && (err.contains("-128") || err.to_lowercase().contains("user canceled")) {
        return Err(InstallError::Cancelled);
    }
    let detail = err.trim_start_matches("0:").trim();
    fail(format!("{failed} ({}). {after}", if detail.is_empty() { "no details" } else { detail }))
}

// ---- copy without a shell (Windows) ---------------------------------------------------------------------------------

fn copy_dir(src: &Path, dest: &Path) -> std::io::Result<()> {
    fs::create_dir(dest)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let to = dest.join(entry.file_name());
        let kind = entry.file_type()?;
        if kind.is_dir() {
            copy_dir(&entry.path(), &to)?;
        } else if kind.is_file() {
            fs::copy(entry.path(), &to)?;
        } else {
            return Err(std::io::Error::other(format!("unexpected link {}", entry.path().display())));
        }
    }
    Ok(())
}

fn exists(p: &Path) -> bool {
    fs::symlink_metadata(p).is_ok()
}

fn remove_any(p: &Path) -> std::io::Result<()> {
    match fs::symlink_metadata(p) {
        Ok(m) if m.is_dir() => fs::remove_dir_all(p),
        Ok(_) => fs::remove_file(p),
        Err(_) => Ok(()),
    }
}

/// All-or-nothing copy, done by Central itself: old bundles are renamed to their backup name, new ones copied in;
/// on any error the new ones are removed and the old ones renamed back. Returns a sentence for the user on error.
pub fn copy_into_place(items: &[CopyItem]) -> Result<(), String> {
    let mut moved = vec![false; items.len()];
    let mut copied = vec![false; items.len()];
    let mut run = || -> Result<(), String> {
        for (i, it) in items.iter().enumerate() {
            let name = it.dest.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            if let Some(parent) = it.dest.parent() {
                fs::create_dir_all(parent).map_err(|e| format!("cannot create the folder {} ({e})", parent.display()))?;
            }
            if let Some(parent) = it.backup.parent() {
                fs::create_dir_all(parent).map_err(|e| format!("cannot create the backup folder ({e})"))?;
            }
            remove_any(&it.backup).map_err(|e| format!("cannot remove an old backup of \"{name}\" ({e})"))?;
            if exists(&it.dest) {
                fs::rename(&it.dest, &it.backup).map_err(|e| {
                    format!("cannot move the old \"{name}\" aside - is your DAW open? Close it and try again ({e})")
                })?;
                moved[i] = true;
            }
            copied[i] = true;
            copy_dir(&it.src, &it.dest).map_err(|e| format!("cannot copy \"{name}\" ({e})"))?;
        }
        Ok(())
    };
    let result = run();
    if result.is_err() {
        for (i, it) in items.iter().enumerate().rev() {
            if copied[i] || moved[i] {
                let _ = remove_any(&it.dest);
            }
            if moved[i] {
                let _ = fs::rename(&it.backup, &it.dest);
            }
        }
        return result;
    }
    for it in items {
        let _ = remove_any(&it.backup);
    }
    Ok(())
}

/// All-or-nothing removal, done by Central itself (Windows way): every bundle is first renamed to
/// "<name>.central-old" next to itself; if one cannot be renamed (a DAW has it open), the others are renamed back and
/// nothing is removed. Then the renamed copies are deleted.
pub fn remove_bundles(paths: &[PathBuf]) -> Result<(), String> {
    let backup = |p: &Path| {
        let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        p.with_file_name(format!("{name}{OLD_SUFFIX}"))
    };
    let mut moved: Vec<&PathBuf> = Vec::new();
    for p in paths {
        if !exists(p) {
            continue;
        }
        let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let b = backup(p);
        let step = remove_any(&b)
            .map_err(|e| format!("cannot remove an old backup of \"{name}\" ({e})"))
            .and_then(|_| {
                fs::rename(p, &b)
                    .map_err(|e| format!("cannot remove \"{name}\" - is your DAW open? Close it and try again ({e})"))
            });
        if let Err(msg) = step {
            for m in moved.iter().rev() {
                let _ = fs::rename(backup(m), m);
            }
            return Err(msg);
        }
        moved.push(p);
    }
    for m in moved {
        let _ = remove_any(&backup(m));
    }
    Ok(())
}

/// What the elevated helper gets: the copy steps and/or bundles to remove. Written into Central's private temporary
/// folder, read by `Eifel Mastering Central.exe --copy-plugins <plan.json>` running as administrator.
#[derive(Serialize, Deserialize, Debug, Default)]
pub struct CopyPlan {
    #[serde(default)]
    pub items: Vec<CopyItem>,
    /// bundles to remove (step 7)
    #[serde(default)]
    pub remove: Vec<PathBuf>,
}

/// The helper only copies or removes Recky's plug-in bundles in the given plug-in folders, nothing else: every
/// destination (and every bundle to remove) must be a plain bundle name directly inside one of `allowed_dirs`, a
/// copy's backup "<dest>.central-old" next to it, and its source a folder.
pub fn check_plan(plan: &CopyPlan, allowed_dirs: &[PathBuf]) -> Result<(), String> {
    if plan.items.is_empty() && plan.remove.is_empty() {
        return Err("nothing to copy".into());
    }
    let place_ok = |p: &Path| {
        let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let parent_ok = p.parent().is_some_and(|d| allowed_dirs.iter().any(|a| a == d));
        let name_ok = [Format::Vst3, Format::Clap, Format::Au].iter().any(|f| is_bundle_name(&name, *f));
        (name, parent_ok && name_ok)
    };
    for it in &plan.items {
        let (name, ok) = place_ok(&it.dest);
        let backup_ok = it.backup == it.dest.with_file_name(format!("{name}{OLD_SUFFIX}"));
        if !ok || !backup_ok || !it.src.is_dir() {
            return Err(format!("refused copy step for \"{name}\""));
        }
    }
    for p in &plan.remove {
        let (name, ok) = place_ok(p);
        if !ok {
            return Err(format!("refused remove step for \"{name}\""));
        }
    }
    Ok(())
}

/// Entry point of the elevated helper. Exit code 0 = done; otherwise the reason is in "<plan>.log".
pub fn run_copy_helper(plan_path: &Path, allowed_dirs: &[PathBuf]) -> i32 {
    let log = plan_path.with_extension("log");
    let result = fs::read(plan_path)
        .map_err(|e| format!("cannot read the copy plan ({e})"))
        .and_then(|b| serde_json::from_slice::<CopyPlan>(&b).map_err(|e| format!("damaged copy plan ({e})")))
        .and_then(|plan| check_plan(&plan, allowed_dirs).map(|_| plan))
        .and_then(|plan| {
            if !plan.items.is_empty() {
                copy_into_place(&plan.items)?;
            }
            remove_bundles(&plan.remove)
        });
    match result {
        Ok(()) => 0,
        Err(msg) => {
            let _ = fs::write(&log, msg);
            1
        }
    }
}

/// Windows result of "Run as administrator": the exit code, or the user clicked No in the UAC prompt.
#[cfg_attr(not(windows), allow(dead_code))] // used by run_elevated (Windows only)
#[derive(Debug, PartialEq)]
pub enum Elevated {
    Exit(i32),
    Declined,
}

/// Exit code PowerShell uses below when the UAC prompt was declined (ERROR_CANCELLED).
#[cfg_attr(not(windows), allow(dead_code))] // used by run_elevated (Windows only)
const UAC_DECLINED: i32 = 1223;

/// PowerShell single-quoted literal ('' for '; PowerShell also treats the curly quotes as quotes).
#[cfg_attr(not(windows), allow(dead_code))] // used by run_elevated (Windows only)
fn ps_quote(s: &str) -> String {
    let mut out = String::from("'");
    for c in s.chars() {
        if matches!(c, '\'' | '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}') {
            out.push(c);
        }
        out.push(c);
    }
    out.push('\'');
    out
}

/// The PowerShell command that starts `exe args` with "Run as administrator" (one UAC prompt), waits, and passes on
/// its exit code. A declined prompt is recognised by the Windows error number 1223, not by the (translated) text.
#[cfg_attr(not(windows), allow(dead_code))] // used by run_elevated (Windows only)
pub fn elevate_command(exe: &Path, args: &[String]) -> String {
    // Start-Process joins the list with spaces: only arguments with spaces (or empty ones) get double quotes.
    let list: Vec<String> = args
        .iter()
        .map(|a| ps_quote(&if a.is_empty() || a.contains(char::is_whitespace) { format!("\"{a}\"") } else { a.clone() }))
        .collect();
    format!(
        "$ErrorActionPreference = 'Stop'; try {{ $p = Start-Process -FilePath {} -ArgumentList @({}) -Verb RunAs \
         -WindowStyle Hidden -Wait -PassThru; exit $p.ExitCode }} catch {{ $e = $_.Exception; \
         while ($e -and -not ($e -is [System.ComponentModel.Win32Exception])) {{ $e = $e.InnerException }}; \
         if ($e -and $e.NativeErrorCode -eq {UAC_DECLINED}) {{ exit {UAC_DECLINED} }}; \
         [Console]::Error.WriteLine($_.Exception.Message); exit 2 }}",
        ps_quote(&exe.to_string_lossy()),
        list.join(",")
    )
}

/// Runs `exe args` as administrator on Windows (one UAC prompt) and waits for it.
#[cfg(windows)]
pub fn run_elevated(exe: &Path, args: &[String]) -> Result<Elevated, String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let system_root = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    let ps = system_root.join(r"System32\WindowsPowerShell\v1.0\powershell.exe");
    let out = std::process::Command::new(ps)
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command"])
        .arg(elevate_command(exe, args))
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("Windows could not start the administrator step ({e})"))?;
    match out.status.code() {
        Some(UAC_DECLINED) => Ok(Elevated::Declined),
        Some(2) | None => Err(String::from_utf8_lossy(&out.stderr).trim().to_string()),
        Some(code) => Ok(Elevated::Exit(code)),
    }
}
#[cfg(not(windows))]
pub fn run_elevated(_exe: &Path, _args: &[String]) -> Result<Elevated, String> {
    Err("Run as administrator exists only on Windows".into())
}

/// Result of starting the Windows administrator helper.
enum HelperOutcome {
    Done,
    Declined,
    /// the helper ran but refused or failed; the reason from its log
    Failed(String),
    /// the helper could not be started
    NotStarted(String),
}

/// Writes `plan` into the private work folder and runs `Central.exe --copy-plugins <plan.json>` as administrator
/// (one Windows UAC prompt).
fn run_helper(plan: &CopyPlan, work: &Path) -> HelperOutcome {
    let plan_path = work.join("plan.json");
    let written = serde_json::to_vec_pretty(plan).map_err(|e| e.to_string()).and_then(|b| fs::write(&plan_path, b).map_err(|e| e.to_string()));
    if let Err(e) = written {
        return HelperOutcome::NotStarted(format!("cannot write the plan: {e}"));
    }
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => return HelperOutcome::NotStarted(format!("Central cannot find itself: {e}")),
    };
    let args = vec!["--copy-plugins".to_string(), plan_path.to_string_lossy().into_owned()];
    match run_elevated(&exe, &args) {
        Ok(Elevated::Declined) => HelperOutcome::Declined,
        Ok(Elevated::Exit(0)) => HelperOutcome::Done,
        Ok(Elevated::Exit(code)) => {
            let log = fs::read_to_string(plan_path.with_extension("log")).unwrap_or_default();
            HelperOutcome::Failed(if log.trim().is_empty() { format!("code {code}") } else { log.trim().to_string() })
        }
        Err(e) => HelperOutcome::NotStarted(if e.is_empty() { "no details".into() } else { e }),
    }
}

/// Windows copy: in-process for the user's own folder, through the elevated helper (one UAC prompt) for the system
/// folder.
fn copy_native(items: &[CopyItem], work: &Path, admin: bool) -> Result<(), InstallError> {
    let put_back = |detail: &str| {
        InstallError::Failed(format!(
            "Copying the plug-in failed ({detail}). Your previous version, if there was one, has been put back."
        ))
    };
    if !admin {
        return copy_into_place(items).map_err(|m| put_back(&m));
    }
    match run_helper(&CopyPlan { items: items.to_vec(), remove: vec![] }, work) {
        HelperOutcome::Done => Ok(()),
        HelperOutcome::Declined => Err(InstallError::Cancelled),
        HelperOutcome::Failed(detail) => Err(put_back(&detail)),
        HelperOutcome::NotStarted(e) => fail(format!("Windows could not start the administrator step ({e}). Nothing was installed.")),
    }
}

/// Logic and other hosts ask the per-user AudioComponentRegistrar for Audio Units; restarting it makes a freshly
/// copied .component visible without logging out (it starts again by itself).
fn refresh_audio_units() {
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("/usr/bin/killall").args(["-9", "AudioComponentRegistrar"]).output();
    }
}

/// A private temporary folder for one job (install, update all, remove); removed again when the job ends.
fn work_dir(env: &Env) -> Result<WorkDir, InstallError> {
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let work = env.temp_dir.join(format!("eifel-central-{}-{stamp}", std::process::id()));
    fs::create_dir_all(&work)?;
    set_mode(&work, 0o700)?;
    Ok(WorkDir(work))
}

/// "A, B and C" - at most four names, then "and 3 more".
fn name_list(names: &[String]) -> String {
    let shown: Vec<&str> = names.iter().take(if names.len() > 4 { 3 } else { 4 }).map(|s| s.as_str()).collect();
    let rest = names.len() - shown.len();
    match (shown.len(), rest) {
        (0, _) => String::new(),
        (1, 0) => shown[0].to_string(),
        (_, 0) => format!("{} and {}", shown[..shown.len() - 1].join(", "), shown[shown.len() - 1]),
        _ => format!("{} and {rest} more", shown.join(", ")),
    }
}

/// One plug-in of a job, downloaded, checked and unpacked: what goes where.
struct Prepared<'a> {
    req: &'a InstallRequest,
    /// format, unpacked bundle, destination
    targets: Vec<(Format, PathBuf, PathBuf)>,
}

/// A plug-in of an "update all" (or single install) that did not work, with a sentence for the user.
#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BatchFailure {
    pub id: String,
    pub message: String,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct BatchResult {
    pub installed: Vec<InstallResult>,
    pub failed: Vec<BatchFailure>,
}

fn message_of(e: InstallError) -> String {
    match e {
        InstallError::Failed(m) => m,
        InstallError::Cancelled => "Cancelled. Nothing was changed.".into(),
        InstallError::Busy => "Another plug-in is being installed.".into(),
    }
}

/// Checksum, unpack, check the bundles (into `dir`). Changes nothing on the computer.
fn prepare<'a>(
    env: &Env,
    req: &'a InstallRequest,
    checked: &Checked,
    dir: &Path,
    zip_path: &Path,
    sha: &str,
    report: &dyn Fn(&'static str, u64, u64),
) -> Result<Prepared<'a>, InstallError> {
    report("verify", 0, 1);
    if !sha.eq_ignore_ascii_case(&req.sha256) {
        return fail("The download does not match the catalog's checksum (SHA-256), so it may be damaged or not Recky's file. Nothing was installed.");
    }
    report("unpack", 0, 1);
    let unpacked = dir.join("unpacked");
    unpack(zip_path, &unpacked)?;
    for (_, name) in &checked.targets {
        let b = unpacked.join(name);
        if !b.is_dir() {
            return fail(format!("The download does not contain \"{name}\". Nothing was installed."));
        }
        if let Some(have) = bundle_info(&b).other_plugin(req.bundle_id.as_deref()) {
            return fail(format!("\"{name}\" in the download is a different plug-in ({have}). Nothing was installed."));
        }
    }
    if cancelled() {
        return Err(InstallError::Cancelled);
    }
    let root = env.root(req.location);
    let targets = checked
        .targets
        .iter()
        .map(|(format, name)| (*format, unpacked.join(name), root.join(format.folder()).join(name)))
        .collect();
    Ok(Prepared { req, targets })
}

/// The copy for all prepared plug-ins: first everything for the system folder (ONE password / UAC prompt), then
/// everything for the user's own folder. Each of the two is all-or-nothing. Then installed.json.
fn place(env: &Env, prepared: &[Prepared], work: &Path, report: &dyn Fn(&str, &'static str, u64, u64)) -> Result<BatchResult, InstallError> {
    let mut result = BatchResult { installed: Vec::new(), failed: Vec::new() };
    let mut placed: Vec<&Prepared> = Vec::new();
    for system in [true, false] {
        let group: Vec<&Prepared> = prepared.iter().filter(|p| (p.req.location == Location::System) == system).collect();
        if group.is_empty() {
            continue;
        }
        let admin = system && env.use_admin_prompt;
        let backup_dir = work.join(if system { "previous-system" } else { "previous-user" });
        let mut items = Vec::new();
        for (i, p) in group.iter().enumerate() {
            for (j, (_, src, dest)) in p.targets.iter().enumerate() {
                let name = dest.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let backup = if env.native_copy {
                    dest.with_file_name(format!("{name}{OLD_SUFFIX}"))
                } else {
                    backup_dir.join(format!("{i}-{j}-{name}"))
                };
                items.push(CopyItem { src: src.clone(), dest: dest.clone(), backup });
            }
        }
        for p in &group {
            report(&p.req.plugin_id, if admin { "password" } else { "copy" }, 0, 1);
        }
        let outcome = if env.native_copy {
            copy_native(&items, work, admin)
        } else {
            let script_path = work.join(if system { "copy-system.sh" } else { "copy-user.sh" });
            fs::write(&script_path, copy_script(&items, &backup_dir, admin))?;
            set_mode(&script_path, 0o700)?;
            let names: Vec<String> = group.iter().map(|p| format!("{} {}", p.req.plugin_name, p.req.version)).collect();
            let prompt = format!(
                "Eifel Mastering Central wants to install {} into the plug-in folder for all users (/Library/Audio/Plug-Ins).",
                name_list(&names)
            );
            run_script(&script_path, admin, &prompt, "Copying the plug-in failed", "Your previous version, if there was one, has been put back.")
        };
        match outcome {
            Ok(()) => {
                for p in &group {
                    report(&p.req.plugin_id, "copy", 1, 1);
                }
                placed.extend(group);
            }
            // the system folder comes first: a cancelled password prompt means nothing was changed at all
            Err(InstallError::Cancelled) if placed.is_empty() => return Err(InstallError::Cancelled),
            Err(e) => {
                let msg = message_of(e);
                result.failed.extend(group.iter().map(|p| BatchFailure { id: p.req.plugin_id.clone(), message: msg.clone() }));
            }
        }
    }
    if placed.iter().any(|p| p.targets.iter().any(|(f, _, _)| *f == Format::Au)) {
        refresh_audio_units();
    }

    // remember them
    if !placed.is_empty() {
        let mut file = read_installed(env);
        file.schema_version = 1;
        for p in &placed {
            let formats: Vec<Format> = p.targets.iter().map(|(f, _, _)| *f).collect();
            let paths: Vec<String> = p.targets.iter().map(|(_, _, d)| d.to_string_lossy().into_owned()).collect();
            file.plugins.insert(
                p.req.plugin_id.clone(),
                InstalledRecord {
                    id: p.req.plugin_id.clone(),
                    version: p.req.version.clone(),
                    formats: formats.clone(),
                    location: p.req.location,
                    paths: paths.clone(),
                    installed_at: now_utc(),
                },
            );
            result.installed.push(InstallResult { id: p.req.plugin_id.clone(), version: p.req.version.clone(), formats, location: p.req.location, paths });
        }
        write_installed(env, &file).map_err(|e| {
            InstallError::Failed(format!(
                "The plug-in was installed, but Central could not note it in installed.json ({e}). It will show as \"not by Central\"."
            ))
        })?;
        for p in &placed {
            report(&p.req.plugin_id, "done", 1, 1);
        }
    }
    Ok(result)
}

/// One install (INSTALL, REPLACE, UPDATE, REINSTALL).
pub fn install(env: &Env, req: &InstallRequest, progress: &dyn Fn(Progress)) -> Result<InstallResult, InstallError> {
    let mut r = install_many(env, std::slice::from_ref(req), progress)?;
    if let Some(f) = r.failed.pop() {
        return Err(InstallError::Failed(f.message));
    }
    r.installed.pop().ok_or_else(|| InstallError::Failed("Nothing was installed.".into()))
}

/// Several plug-ins in one go ("Update all"): downloads and checks them one after the other, then copies all of
/// them with ONE password / UAC prompt. A plug-in whose download or check fails is left out (and reported); the
/// others still go ahead. Cancel (button or password prompt) stops everything and changes nothing.
pub fn install_many(env: &Env, reqs: &[InstallRequest], progress: &dyn Fn(Progress)) -> Result<BatchResult, InstallError> {
    if BUSY.swap(true, Ordering::SeqCst) {
        return Err(InstallError::Busy);
    }
    let _busy = BusyGuard;
    CANCEL.store(false, Ordering::SeqCst);
    let report = |id: &str, phase: &'static str, done: u64, total: u64| progress(Progress { id: id.to_string(), phase, done, total });

    let work = work_dir(env)?;
    let mut prepared: Vec<Prepared> = Vec::new();
    let mut failed: Vec<BatchFailure> = Vec::new();
    for (i, req) in reqs.iter().enumerate() {
        if cancelled() {
            return Err(InstallError::Cancelled);
        }
        if prepared.iter().any(|p| p.req.plugin_id == req.plugin_id) || failed.iter().any(|f| f.id == req.plugin_id) {
            continue; // the same plug-in twice
        }
        let checked = match check_request(req) {
            Ok(c) => c,
            Err(e) => {
                failed.push(BatchFailure { id: req.plugin_id.clone(), message: message_of(e) });
                continue;
            }
        };
        let dir = work.0.join(i.to_string());
        fs::create_dir_all(&dir)?;
        let zip_path = dir.join("download.zip");
        let id = req.plugin_id.as_str();
        let r = download(&req.url, req.size, &zip_path, &|d, t| report(id, "download", d, t))
            .and_then(|sha| prepare(env, req, &checked, &dir, &zip_path, &sha, &|ph, d, t| report(id, ph, d, t)));
        match r {
            Ok(p) => prepared.push(p),
            Err(InstallError::Cancelled) => return Err(InstallError::Cancelled),
            Err(e) => failed.push(BatchFailure { id: req.plugin_id.clone(), message: message_of(e) }),
        }
    }
    let mut result = place(env, &prepared, &work.0, &report)?;
    failed.append(&mut result.failed);
    result.failed = failed;
    Ok(result)
}

/// Everything after the download for one plug-in (the tests run it offline).
#[cfg(test)]
fn install_downloaded(
    env: &Env,
    req: &InstallRequest,
    checked: &Checked,
    work: &Path,
    zip_path: &Path,
    sha: &str,
    report: &dyn Fn(&'static str, u64, u64),
) -> Result<InstallResult, InstallError> {
    let p = prepare(env, req, checked, work, zip_path, sha, report)?;
    let mut r = place(env, std::slice::from_ref(&p), work, &|_, ph, d, t| report(ph, d, t))?;
    if let Some(f) = r.failed.pop() {
        return Err(InstallError::Failed(f.message));
    }
    r.installed.pop().ok_or_else(|| InstallError::Failed("Nothing was installed.".into()))
}

// ---- remove (step 7) ------------------------------------------------------------------------------------------------

#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RemoveResult {
    /// plug-ins that are completely gone now
    pub removed: Vec<String>,
    /// the bundles that were deleted
    pub paths: Vec<String>,
}

/// The shell script that removes bundles (Mac). All-or-nothing: every bundle is first moved into `backup_dir`; if one
/// cannot be moved, the others are moved back. Then `backup_dir` is deleted.
pub fn remove_script(paths: &[PathBuf], backup_dir: &Path) -> String {
    let mut s = String::new();
    s.push_str("#!/bin/sh\n# Eifel Mastering Central - removes plug-ins (generated, deleted afterwards)\nset -u\n");
    for i in 0..paths.len() {
        s.push_str(&format!("M{i}=0\n"));
    }
    s.push_str("rollback() {\n");
    for (i, p) in paths.iter().enumerate().rev() {
        let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let b = sh_quote(&backup_dir.join(format!("{i}-{name}")).to_string_lossy());
        s.push_str(&format!("  if [ $M{i} = 1 ]; then mv {b} {}; fi\n", sh_quote(&p.to_string_lossy())));
    }
    s.push_str("  :\n}\n");
    s.push_str("fail() { echo \"$1\" >&2; rollback; exit 1; }\n");
    s.push_str(&format!("mkdir -p {} || fail 'cannot create the backup folder'\n", sh_quote(&backup_dir.to_string_lossy())));
    for (i, p) in paths.iter().enumerate() {
        let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let d = sh_quote(&p.to_string_lossy());
        let b = sh_quote(&backup_dir.join(format!("{i}-{name}")).to_string_lossy());
        s.push_str(&format!("if [ -e {d} ] || [ -L {d} ]; then mv {d} {b} || fail \"cannot remove \"{}; M{i}=1; fi\n", sh_quote(&name)));
    }
    s.push_str(&format!("rm -rf {}\nexit 0\n", sh_quote(&backup_dir.to_string_lossy())));
    s
}

/// Removes plug-ins (OPTIONS > Remove, Settings > Uninstall all): every copy found in both plug-in folders (the
/// same scan as get_installed, so only bundles that are really these plug-ins). The system folder first with ONE
/// password / UAC prompt for everything, then the user's own folder. installed.json forgets the removed ones.
pub fn remove(env: &Env, plugins: &[PluginQuery]) -> Result<RemoveResult, InstallError> {
    if BUSY.swap(true, Ordering::SeqCst) {
        return Err(InstallError::Busy);
    }
    let _busy = BusyGuard;
    let found = get_installed(env, plugins);
    let in_folder = |p: &Path, root: &Path| {
        [Format::Au, Format::Vst3, Format::Clap].iter().any(|f| {
            p.parent() == Some(root.join(f.folder()).as_path())
                && p.file_name().is_some_and(|n| is_bundle_name(&n.to_string_lossy(), *f))
        })
    };
    let (mut system, mut user) = (Vec::new(), Vec::new());
    for info in &found {
        for p in &info.paths {
            let p = PathBuf::from(p);
            if in_folder(&p, &env.system_root) {
                system.push(p);
            } else if in_folder(&p, &env.user_root) {
                user.push(p);
            }
        }
    }
    let all: Vec<PathBuf> = system.iter().chain(user.iter()).cloned().collect();
    if all.is_empty() {
        return Ok(RemoveResult { removed: Vec::new(), paths: Vec::new() });
    }
    let work = work_dir(env)?;
    let names: Vec<String> = found
        .iter()
        .map(|i| plugins.iter().find(|q| q.id == i.id).and_then(|q| q.name.clone()).unwrap_or_else(|| i.id.clone()))
        .collect();
    let removing = |detail: &str| {
        InstallError::Failed(format!("Removing failed ({detail}). Nothing was removed from that folder."))
    };

    let mut error: Option<InstallError> = None;
    for (is_system, paths) in [(true, &system), (false, &user)] {
        if paths.is_empty() || error.is_some() {
            continue;
        }
        let admin = is_system && env.use_admin_prompt;
        let outcome = if env.native_copy {
            if admin {
                match run_helper(&CopyPlan { items: vec![], remove: paths.clone() }, &work.0) {
                    HelperOutcome::Done => Ok(()),
                    HelperOutcome::Declined => Err(InstallError::Cancelled),
                    HelperOutcome::Failed(d) => Err(removing(&d)),
                    HelperOutcome::NotStarted(e) => fail(format!("Windows could not start the administrator step ({e}). Nothing was removed.")),
                }
            } else {
                remove_bundles(paths).map_err(|m| removing(&m))
            }
        } else {
            let tag = if is_system { "system" } else { "user" };
            let script_path = work.0.join(format!("remove-{tag}.sh"));
            fs::write(&script_path, remove_script(paths, &work.0.join(format!("removed-{tag}"))))?;
            set_mode(&script_path, 0o700)?;
            let prompt = format!(
                "Eifel Mastering Central wants to remove {} from the plug-in folder for all users (/Library/Audio/Plug-Ins).",
                name_list(&names)
            );
            run_script(&script_path, admin, &prompt, "Removing failed", "Nothing was removed from that folder.")
        };
        match outcome {
            Ok(()) => {}
            // system folder first: a cancelled prompt means nothing was changed at all
            Err(InstallError::Cancelled) if is_system => return Err(InstallError::Cancelled),
            Err(e) => error = Some(e),
        }
    }

    // what is gone now
    let gone: Vec<String> = all.iter().filter(|p| !exists(p)).map(|p| p.to_string_lossy().into_owned()).collect();
    let removed: Vec<String> = found.iter().filter(|i| i.paths.iter().all(|p| !exists(Path::new(p)))).map(|i| i.id.clone()).collect();
    if gone.iter().any(|p| p.ends_with(Format::Au.suffix())) {
        refresh_audio_units();
    }
    if !removed.is_empty() {
        let mut file = read_installed(env);
        let before = file.plugins.len();
        file.plugins.retain(|id, _| !removed.contains(id));
        if file.plugins.len() != before {
            write_installed(env, &file)?;
        }
    }
    match error {
        Some(e) => Err(e),
        None => Ok(RemoveResult { removed, paths: gone }),
    }
}

// ---- tests ----------------------------------------------------------------------------------------------------------
#[cfg(test)]
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
            native_copy: false,
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
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(&bin).unwrap().permissions().mode() & 0o777, 0o755);
        }
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

    #[cfg(unix)]
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

    #[cfg(unix)]
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
        run_script(&script, false, "", "Copying the plug-in failed", "Your previous version, if there was one, has been put back.").unwrap();
        assert_eq!(bundle_info(&old_au).version.as_deref(), Some("0.2.0"));
        assert_eq!(bundle_info(&env.user_root.join("VST3/V.vst3")).version.as_deref(), Some("0.2.0"));
        assert!(!work.join("previous").exists(), "backup is removed after success");

        // second run where the VST3 source is missing: the AU must be rolled back to 0.2.0, the VST3 too
        make_bundle(&work.join("unpacked/V.component"), "com.mystudio.comp165", "0.3.0");
        fs::write(work.join("unpacked/V.component/Contents/Info.plist"), info_plist("com.mystudio.comp165", "0.3.0")).unwrap();
        fs::remove_dir_all(work.join("unpacked/V.vst3")).unwrap();
        fs::write(&script, copy_script(&items, &work.join("previous"), false)).unwrap();
        let r = run_script(&script, false, "", "Copying the plug-in failed", "Your previous version, if there was one, has been put back.");
        assert!(matches!(r, Err(InstallError::Failed(ref m)) if m.contains("put back")), "{r:?}");
        assert_eq!(bundle_info(&old_au).version.as_deref(), Some("0.2.0"), "AU rolled back");
        assert_eq!(bundle_info(&env.user_root.join("VST3/V.vst3")).version.as_deref(), Some("0.2.0"), "VST3 rolled back");

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
        let q = vec![PluginQuery { id: "reckys-velvet".into(), bundle_names: names, bundle_id: Some("com.mystudio.comp165".into()), name: None }];

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

    #[cfg(unix)]
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
        let q = vec![PluginQuery { id: "reckys-velvet".into(), bundle_names: names, bundle_id: Some("com.mystudio.comp165".into()), name: None }];
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

    /// Runs on GitHub's Mac and Windows computers (workflow "Build app"): downloads every plug-in of the LIVE catalog
    /// for this platform, checks its SHA-256, unpacks it and checks the bundles inside (Mac: bundle id, version,
    /// signature; Windows: maker code, version, the DLL). Installs nothing.  cargo test -- --include-ignored
    #[test]
    #[ignore]
    fn real_catalog_downloads() {
        let platform = if cfg!(windows) { "windows" } else { "macos" };
        let text = ureq::get("https://reckyreck.github.io/eifel-catalog/catalog.json")
            .call()
            .expect("catalog")
            .into_string()
            .expect("catalog text");
        let catalog: serde_json::Value = serde_json::from_str(&text).expect("catalog json");
        let base = scratch("real");
        let mut checked = 0;
        for p in catalog["plugins"].as_array().expect("plugins") {
            let id = p["id"].as_str().unwrap();
            let bundle_id = p["bundleId"].as_str();
            for a in p["latest"]["assets"].as_array().unwrap() {
                if a["platform"] != platform {
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
                    let info = bundle_info(&b);
                    assert_eq!(info.other_plugin(bundle_id), None, "{id}: {name} is another plug-in");
                    assert_eq!(info.version.as_deref(), p["latest"]["version"].as_str(), "{id}: {name} version");
                    if platform == "macos" {
                        assert!(b.join("Contents/Info.plist").is_file(), "{id}: {name} missing");
                        if let Some(want) = bundle_id {
                            assert_eq!(info.bundle_id.as_deref(), Some(want), "{id}: {name} bundle id");
                        }
                    } else {
                        assert_eq!(info.maker.as_deref(), Some(MAKER_CODE), "{id}: {name} maker code");
                        assert!(b.join("Contents/x86_64-win").join(name).is_file(), "{id}: {name} has no 64-bit Windows DLL");
                    }
                    #[cfg(target_os = "macos")]
                    {
                        let st = std::process::Command::new("/usr/bin/codesign").args(["--verify", "--deep", "--strict"]).arg(&b).status().unwrap();
                        assert!(st.success(), "{id}: {name} signature broken after unpacking");
                    }
                    println!("OK {id} {name} {}", info.version.unwrap_or_default());
                    checked += 1;
                }
            }
        }
        assert!(checked > 0, "no {platform} plug-in in the catalog");
        println!("{checked} {platform} bundles checked");
        fs::remove_dir_all(&base).unwrap();
    }

    // ---- Windows way (runs everywhere: plain Rust file operations) ----

    /// The moduleinfo.json of Recky's Velvet 0.1.0 for Windows, as built (with the SDK's trailing commas).
    const VELVET_MODULEINFO: &str = r#"{
  "Name": "Recky's Velvet",
  "Version": "0.1.0",
  "Factory Info": {
    "Vendor": "Eifel Mastering",
    "URL": "",
    "E-Mail": "",
    "Flags": {
      "Unicode": true,
      "Classes Discardable": false,
      "Component Non Discardable": false,
    },
  },
  "Classes": [
    {
      "CID": "ABCDEF019182FAEB4D7374644D313635",
      "Category": "Audio Module Class",
      "Name": "Recky's Velvet",
      "Vendor": "Eifel Mastering",
      "Version": "0.1.0",
      "SDKVersion": "VST 3.8.0",
      "Sub Categories": [
        "Fx",
        "Dynamics",
      ],
      "Class Flags": 0,
      "Cardinality": 2147483647,
      "Snapshots": [
      ],
    },
    {
      "CID": "ABCDEF011234ABCD4D7374644D313635",
      "Category": "Component Controller Class",
      "Name": "Recky's Velvet",
      "Vendor": "Eifel Mastering",
      "Version": "0.1.0",
      "SDKVersion": "VST 3.8.0",
      "Sub Categories": [
        "Fx",
        "Dynamics",
      ],
      "Class Flags": 0,
      "Cardinality": 2147483647,
      "Snapshots": [
      ],
    },
  ],
}"#;

    fn module_info_text(version: &str, maker_hex: &str) -> String {
        VELVET_MODULEINFO.replace("0.1.0", version).replace("4D7374644D313635", &format!("{maker_hex}4D313635"))
    }

    fn make_win_bundle(dir: &Path, version: &str, maker_hex: &str) {
        let name = dir.file_name().unwrap().to_string_lossy().into_owned();
        fs::create_dir_all(dir.join("Contents/x86_64-win")).unwrap();
        fs::create_dir_all(dir.join("Contents/Resources")).unwrap();
        fs::write(dir.join("Contents/Resources/moduleinfo.json"), module_info_text(version, maker_hex)).unwrap();
        fs::write(dir.join("Contents/x86_64-win").join(name), format!("dll {version}")).unwrap();
    }

    fn win_version(dir: &Path) -> Option<String> {
        bundle_info(dir).version
    }

    #[test]
    fn reads_windows_moduleinfo() {
        let i = module_info(VELVET_MODULEINFO);
        assert_eq!(i.version.as_deref(), Some("0.1.0"));
        assert_eq!(i.maker.as_deref(), Some("Mstd"));
        assert_eq!(i.bundle_id, None);
        assert_eq!(i.other_plugin(Some("com.mystudio.comp165")), None, "no bundle id on Windows: maker code decides");
        let other = module_info(&module_info_text("2.0.0", "41424344")); // "ABCD"
        assert_eq!(other.other_plugin(Some("com.mystudio.comp165")).as_deref(), Some("maker code ABCD"));
        // commas inside strings stay
        assert_eq!(strip_trailing_commas(r#"{"a": "x,]", "b": [1,2,],}"#), r#"{"a": "x,]", "b": [1,2]}"#);
        assert_eq!(module_info("not json"), BundleInfo::default());
    }

    #[test]
    fn copy_into_place_replaces_and_rolls_back() {
        let base = scratch("native-copy");
        let vst3 = base.join("Common Files/VST3");
        let work = base.join("work");
        make_win_bundle(&work.join("unpacked/A.vst3"), "0.2.0", "4D737464");
        make_win_bundle(&work.join("unpacked/B.vst3"), "0.2.0", "4D737464");
        make_win_bundle(&vst3.join("A.vst3"), "0.1.0", "4D737464"); // old copy
        let item = |n: &str| CopyItem {
            src: work.join("unpacked").join(n),
            dest: vst3.join(n),
            backup: vst3.join(format!("{n}{OLD_SUFFIX}")),
        };
        let items = vec![item("A.vst3"), item("B.vst3")];
        copy_into_place(&items).unwrap();
        assert_eq!(win_version(&vst3.join("A.vst3")).as_deref(), Some("0.2.0"));
        assert_eq!(win_version(&vst3.join("B.vst3")).as_deref(), Some("0.2.0"));
        assert_eq!(fs::read_to_string(vst3.join("A.vst3/Contents/x86_64-win/A.vst3")).unwrap(), "dll 0.2.0");
        assert!(!exists(&vst3.join("A.vst3.central-old")), "backup removed after success");

        // second run, B's source is missing: A goes back to 0.2.0, B stays 0.2.0
        fs::remove_dir_all(work.join("unpacked")).unwrap();
        make_win_bundle(&work.join("unpacked/A.vst3"), "0.3.0", "4D737464");
        let err = copy_into_place(&items).unwrap_err();
        assert!(err.contains("cannot copy \"B.vst3\""), "{err}");
        assert_eq!(win_version(&vst3.join("A.vst3")).as_deref(), Some("0.2.0"), "A rolled back");
        assert_eq!(win_version(&vst3.join("B.vst3")).as_deref(), Some("0.2.0"), "B rolled back");
        assert!(!exists(&vst3.join("A.vst3.central-old")) && !exists(&vst3.join("B.vst3.central-old")));

        // a stale backup from an earlier crash does not block the next install
        make_win_bundle(&work.join("unpacked/B.vst3"), "0.3.0", "4D737464");
        make_win_bundle(&vst3.join("A.vst3.central-old"), "0.0.1", "4D737464");
        copy_into_place(&items).unwrap();
        assert_eq!(win_version(&vst3.join("A.vst3")).as_deref(), Some("0.3.0"));
        assert!(!exists(&vst3.join("A.vst3.central-old")));
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn helper_plan_is_checked() {
        let base = scratch("plan");
        let vst3 = base.join("Common Files/VST3");
        let src = base.join("work/unpacked/A.vst3");
        make_win_bundle(&src, "0.1.0", "4D737464");
        let ok = CopyItem { src: src.clone(), dest: vst3.join("A.vst3"), backup: vst3.join("A.vst3.central-old") };
        let allowed = vec![vst3.clone()];
        assert!(check_plan(&CopyPlan { items: vec![ok.clone()], remove: vec![] }, &allowed).is_ok());
        let bad = [
            CopyItem { dest: base.join("Windows/System32/A.vst3"), backup: base.join("Windows/System32/A.vst3.central-old"), ..ok.clone() },
            CopyItem { dest: vst3.join("evil.dll"), backup: vst3.join("evil.dll.central-old"), ..ok.clone() },
            CopyItem { backup: base.join("elsewhere"), ..ok.clone() },
            CopyItem { dest: vst3.join("sub/A.vst3"), backup: vst3.join("sub/A.vst3.central-old"), ..ok.clone() },
            CopyItem { src: base.join("missing.vst3"), ..ok.clone() },
        ];
        for b in bad {
            assert!(check_plan(&CopyPlan { items: vec![ok.clone(), b.clone()], remove: vec![] }, &allowed).is_err(), "{b:?}");
        }
        assert!(check_plan(&CopyPlan { items: vec![], remove: vec![] }, &allowed).is_err());

        // the helper itself: plan file in, copy done, exit code 0; a refused plan writes the reason into plan.log
        let plan_path = base.join("work/plan.json");
        fs::write(&plan_path, serde_json::to_vec(&CopyPlan { items: vec![ok.clone()], remove: vec![] }).unwrap()).unwrap();
        assert_eq!(run_copy_helper(&plan_path, &allowed), 0);
        assert_eq!(win_version(&vst3.join("A.vst3")).as_deref(), Some("0.1.0"));
        assert_eq!(run_copy_helper(&plan_path, &[base.join("other")]), 1);
        assert!(fs::read_to_string(base.join("work/plan.log")).unwrap().contains("refused"));

        // step 7: a plan that only removes - only bundles directly in the allowed folder
        let rm_ok = CopyPlan { items: vec![], remove: vec![vst3.join("A.vst3")] };
        assert!(check_plan(&rm_ok, &allowed).is_ok());
        for bad in [base.join("Windows/System32/A.vst3"), vst3.join("evil.dll"), vst3.join("sub/A.vst3"), vst3.clone()] {
            let plan = CopyPlan { items: vec![], remove: vec![vst3.join("A.vst3"), bad.clone()] };
            assert!(check_plan(&plan, &allowed).is_err(), "{bad:?}");
        }
        fs::write(&plan_path, serde_json::to_vec(&rm_ok).unwrap()).unwrap();
        assert_eq!(run_copy_helper(&plan_path, &allowed), 0);
        assert!(!vst3.join("A.vst3").exists() && !vst3.join("A.vst3.central-old").exists());
        // an old helper plan without "remove" still reads
        let old: CopyPlan = serde_json::from_str(r#"{"items":[]}"#).unwrap();
        assert!(old.remove.is_empty());
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn whole_install_offline_windows_way() {
        let base = scratch("whole-win");
        let mut env = env_in(&base);
        env.native_copy = true;
        let mi = module_info_text("0.1.0", "4D737464");
        let data = zip_of(&[
            ("Reckys Velvet.vst3/Contents/Resources/moduleinfo.json", mi.as_bytes(), 0o644),
            ("Reckys Velvet.vst3/Contents/x86_64-win/Reckys Velvet.vst3", b"dll", 0o644),
        ]);
        let sha = hex(&Sha256::digest(&data));
        let mut names = BTreeMap::new();
        names.insert("au".to_string(), "Reckys Velvet.component".to_string());
        names.insert("vst3".to_string(), "Reckys Velvet.vst3".to_string());
        let req = InstallRequest {
            plugin_id: "reckys-velvet".into(),
            plugin_name: "Recky's Velvet".into(),
            version: "0.1.0".into(),
            url: format!("{DOWNLOAD_PREFIX}reckys-velvet-v0.1.0/ReckysVelvet-0.1.0-windows.zip"),
            sha256: sha.clone(),
            size: data.len() as u64,
            bundle_names: names.clone(),
            bundle_id: Some("com.mystudio.comp165".into()),
            formats: vec!["vst3".into()],
            location: Location::System,
        };
        let checked = check_request(&req).unwrap();
        let work = base.join("work");
        fs::create_dir_all(&work).unwrap();
        let zip_path = work.join("download.zip");
        fs::write(&zip_path, &data).unwrap();
        let report = |_: &'static str, _: u64, _: u64| {};

        // an old copy from another installer is replaced
        let dest = env.system_root.join("VST3/Reckys Velvet.vst3");
        make_win_bundle(&dest, "0.0.9", "4D737464");
        let q = vec![PluginQuery { id: "reckys-velvet".into(), bundle_names: names, bundle_id: Some("com.mystudio.comp165".into()), name: None }];
        let before = get_installed(&env, &q);
        assert_eq!((before[0].version.as_str(), before[0].by_central), ("0.0.9", false));

        let r = install_downloaded(&env, &req, &checked, &work, &zip_path, &sha, &report).unwrap();
        assert_eq!(r.formats, vec![Format::Vst3]);
        assert_eq!(fs::read(dest.join("Contents/x86_64-win/Reckys Velvet.vst3")).unwrap(), b"dll");
        assert!(!exists(&dest.with_file_name("Reckys Velvet.vst3.central-old")));
        let found = get_installed(&env, &q);
        assert_eq!(found.len(), 1);
        assert!(found[0].by_central);
        assert_eq!(found[0].version, "0.1.0");
        assert_eq!(found[0].location, Some(Location::System));

        // a download whose bundle carries another maker code is refused, the installed copy stays
        let other = module_info_text("0.1.0", "41424344");
        let data2 = zip_of(&[
            ("Reckys Velvet.vst3/Contents/Resources/moduleinfo.json", other.as_bytes(), 0o644),
            ("Reckys Velvet.vst3/Contents/x86_64-win/Reckys Velvet.vst3", b"evil", 0o644),
        ]);
        fs::write(&zip_path, &data2).unwrap();
        let sha2 = hex(&Sha256::digest(&data2));
        let req2 = InstallRequest { sha256: sha2.clone(), size: data2.len() as u64, ..req };
        let _ = fs::remove_dir_all(work.join("unpacked"));
        let r = install_downloaded(&env, &req2, &checked, &work, &zip_path, &sha2, &report);
        assert!(matches!(r, Err(InstallError::Failed(ref m)) if m.contains("maker code ABCD")), "{r:?}");
        assert_eq!(fs::read(dest.join("Contents/x86_64-win/Reckys Velvet.vst3")).unwrap(), b"dll");
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn elevation_command_quotes_paths() {
        assert_eq!(ps_quote("C:\\Users\\O'Brien"), "'C:\\Users\\O''Brien'");
        assert_eq!(ps_quote("a\u{2019}b"), "'a\u{2019}\u{2019}b'");
        let cmd = elevate_command(
            Path::new(r"C:\Users\Jürgen\AppData\Local\Eifel Mastering Central\Eifel Mastering Central.exe"),
            &["--copy-plugins".into(), r"C:\Users\Jürgen Ö\AppData\Local\Temp\eifel-central-x\plan.json".into()],
        );
        assert!(cmd.contains(r"-FilePath 'C:\Users\Jürgen\AppData\Local\Eifel Mastering Central\Eifel Mastering Central.exe'"), "{cmd}");
        assert!(cmd.contains(r#"-ArgumentList @('--copy-plugins','"C:\Users\Jürgen Ö\AppData\Local\Temp\eifel-central-x\plan.json"')"#), "{cmd}");
        assert!(elevate_command(Path::new("cmd.exe"), &["/c".into(), "exit".into(), "7".into()]).contains("-ArgumentList @('/c','exit','7')"));
        assert!(cmd.contains("-Verb RunAs") && cmd.contains("NativeErrorCode -eq 1223"));
    }

    /// GitHub's Windows computers run as administrator without a UAC prompt, so this checks the real elevation path
    /// (PowerShell Start-Process -Verb RunAs, exit code passed back). Ignored elsewhere.
    #[cfg(windows)]
    #[test]
    #[ignore]
    fn real_elevation_passes_exit_code() {
        let cmd = PathBuf::from(std::env::var("ComSpec").unwrap_or_else(|_| r"C:\Windows\System32\cmd.exe".into()));
        let r = run_elevated(&cmd, &["/c".into(), "exit".into(), "7".into()]);
        assert_eq!(r, Ok(Elevated::Exit(7)));
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
        let names = |n: &[&str]| name_list(&n.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        assert_eq!(names(&["A"]), "A");
        assert_eq!(names(&["A", "B"]), "A and B");
        assert_eq!(names(&["A", "B", "C", "D"]), "A, B, C and D");
        assert_eq!(names(&["A", "B", "C", "D", "E", "F"]), "A, B, C and 3 more");
    }

    /// remove() and install_many() refuse to run twice at once ("busy"): tests that call them take turns.
    static JOB_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn velvet_query(au: &str, vst3: &str) -> Vec<PluginQuery> {
        let mut names = BTreeMap::new();
        names.insert("au".to_string(), au.to_string());
        names.insert("vst3".to_string(), vst3.to_string());
        vec![PluginQuery { id: "reckys-velvet".into(), bundle_names: names, bundle_id: Some("com.mystudio.comp165".into()), name: Some("Recky's Velvet".into()) }]
    }

    fn record(env: &Env, id: &str, version: &str, loc: Location, paths: &[&Path]) {
        let mut file = read_installed(env);
        file.plugins.insert(
            id.into(),
            InstalledRecord {
                id: id.into(),
                version: version.into(),
                formats: vec![Format::Au],
                location: loc,
                paths: paths.iter().map(|p| p.to_string_lossy().into_owned()).collect(),
                installed_at: now_utc(),
            },
        );
        write_installed(env, &file).unwrap();
    }

    /// Step 7, Mac way: REMOVE takes every copy (system and user folder), keeps other plug-ins and same-named
    /// bundles of someone else, and forgets the plug-in in installed.json.
    #[cfg(unix)]
    #[test]
    fn remove_offline_mac_way() {
        let _turn = JOB_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let base = scratch("remove");
        let env = env_in(&base);
        let sys_au = env.system_root.join("Components/V.component");
        let user_vst = env.user_root.join("VST3/V.vst3");
        let other = env.user_root.join("Components/Other.component");
        let foreign = env.system_root.join("VST3/V.vst3"); // same file name, someone else's plug-in
        make_bundle(&sys_au, "com.mystudio.comp165", "0.1.0");
        make_bundle(&user_vst, "com.mystudio.comp165", "0.1.0");
        make_bundle(&other, "com.mystudio.rider", "0.1.0");
        make_bundle(&foreign, "com.someone.else", "1.0.0");
        record(&env, "reckys-velvet", "0.1.0", Location::System, &[&sys_au]);
        record(&env, "reckys-rider", "0.1.0", Location::User, &[&other]);

        let q = velvet_query("V.component", "V.vst3");
        let r = remove(&env, &q).unwrap();
        assert_eq!(r.removed, vec!["reckys-velvet".to_string()]);
        assert_eq!(r.paths.len(), 2);
        assert!(!sys_au.exists() && !user_vst.exists());
        assert!(other.exists(), "another plug-in stays");
        assert!(foreign.exists(), "a different plug-in with the same file name stays");
        let file = read_installed(&env);
        assert!(!file.plugins.contains_key("reckys-velvet") && file.plugins.contains_key("reckys-rider"));
        assert!(get_installed(&env, &q).is_empty());
        // nothing left to remove: fine, nothing happens
        assert_eq!(remove(&env, &q).unwrap(), RemoveResult { removed: vec![], paths: vec![] });
        // no temporary folders left
        assert_eq!(fs::read_dir(&env.temp_dir).map(|d| d.count()).unwrap_or(0), 0);
        fs::remove_dir_all(&base).unwrap();
    }

    /// The remove script is all-or-nothing: if the second bundle cannot be moved away, the first is put back.
    #[cfg(unix)]
    #[test]
    fn remove_script_rolls_back() {
        let base = scratch("remove-script");
        let a = base.join("Plug-Ins/Components/A.component");
        let b = base.join("Plug-Ins/VST3/A.vst3");
        make_bundle(&a, "x", "0.1.0");
        make_bundle(&b, "x", "0.1.0");
        let backup = base.join("work/removed");
        fs::create_dir_all(&backup).unwrap();
        fs::write(backup.join("1-A.vst3"), b"in the way").unwrap(); // mv of a folder onto a file fails
        let script = base.join("work/remove.sh");
        fs::write(&script, remove_script(&[a.clone(), b.clone()], &backup)).unwrap();
        let r = run_script(&script, false, "", "Removing failed", "Nothing was removed from that folder.");
        assert!(matches!(r, Err(InstallError::Failed(ref m)) if m.contains("Removing failed")), "{r:?}");
        assert!(a.join("Contents/Info.plist").exists(), "first bundle put back");
        assert!(b.join("Contents/Info.plist").exists());
        fs::remove_file(backup.join("1-A.vst3")).unwrap();
        run_script(&script, false, "", "Removing failed", "").unwrap();
        assert!(!a.exists() && !b.exists() && !backup.exists());
        fs::remove_dir_all(&base).unwrap();
    }

    /// Windows way: rename to ".central-old", then delete; one that cannot be renamed puts the others back.
    #[cfg(unix)]
    #[test]
    fn remove_bundles_windows_way() {
        let _turn = JOB_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let base = scratch("remove-win");
        let vst3 = base.join("Common Files/VST3");
        let a = vst3.join("A.vst3");
        make_win_bundle(&a, "0.1.0", "4D737464");
        // a name so long that "<name>.central-old" is too long for the file system: the rename fails
        let long = vst3.join(format!("{}.vst3", "L".repeat(240)));
        make_win_bundle(&long, "0.1.0", "4D737464");
        let r = remove_bundles(&[a.clone(), long.clone()]);
        assert!(matches!(r, Err(ref m) if m.contains("DAW open")), "{r:?}");
        assert!(a.join("Contents/Resources/moduleinfo.json").exists(), "first bundle renamed back");
        assert!(!vst3.join("A.vst3.central-old").exists());
        remove_bundles(&[a.clone(), vst3.join("Missing.vst3")]).unwrap();
        assert!(!a.exists() && !vst3.join("A.vst3.central-old").exists());

        // whole remove() the Windows way (user folder, no UAC in tests)
        let env = Env { native_copy: true, ..env_in(&base) };
        let user_vst = env.user_root.join("VST3/V.vst3");
        make_win_bundle(&user_vst, "0.1.0", "4D737464");
        let mut names = BTreeMap::new();
        names.insert("vst3".to_string(), "V.vst3".to_string());
        let q = vec![PluginQuery { id: "reckys-velvet".into(), bundle_names: names, bundle_id: None, name: None }];
        assert_eq!(remove(&env, &q).unwrap().removed, vec!["reckys-velvet".to_string()]);
        assert!(!user_vst.exists());
        fs::remove_dir_all(&base).unwrap();
    }

    /// "Update all": two plug-ins, one for the system folder and one for the user's folder, copied in one job and
    /// both noted in installed.json.
    #[cfg(unix)]
    #[test]
    fn several_plugins_in_one_job() {
        let base = scratch("batch");
        let env = env_in(&base);
        let work = base.join("work");
        let mut reqs = Vec::new();
        let mut zips = Vec::new();
        for (i, (id, bundle_id, loc)) in [("reckys-velvet", "com.mystudio.comp165", Location::System), ("reckys-rider", "com.mystudio.reckysrider", Location::User)]
            .into_iter()
            .enumerate()
        {
            let pl = info_plist(bundle_id, "0.2.0");
            let name = format!("P{i}.component");
            let data = zip_of(&[(&format!("{name}/Contents/Info.plist"), &pl, 0o644)]);
            let mut names = BTreeMap::new();
            names.insert("au".to_string(), name.clone());
            reqs.push(InstallRequest {
                plugin_id: id.into(),
                plugin_name: id.into(),
                version: "0.2.0".into(),
                url: format!("{DOWNLOAD_PREFIX}{id}/x.zip"),
                sha256: hex(&Sha256::digest(&data)),
                size: data.len() as u64,
                bundle_names: names,
                bundle_id: Some(bundle_id.into()),
                formats: vec!["au".into()],
                location: loc,
            });
            zips.push(data);
        }
        // an older Velvet is in the system folder already
        make_bundle(&env.system_root.join("Components/P0.component"), "com.mystudio.comp165", "0.1.0");
        let mut prepared = Vec::new();
        for (i, (req, data)) in reqs.iter().zip(&zips).enumerate() {
            let dir = work.join(i.to_string());
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("download.zip"), data).unwrap();
            let checked = check_request(req).unwrap();
            prepared.push(prepare(&env, req, &checked, &dir, &dir.join("download.zip"), &req.sha256, &|_, _, _| {}).unwrap());
        }
        let seen = std::cell::RefCell::new(Vec::new());
        let r = place(&env, &prepared, &work, &|id, ph, _, _| seen.borrow_mut().push(format!("{id}:{ph}"))).unwrap();
        assert!(r.failed.is_empty());
        assert_eq!(r.installed.len(), 2);
        assert_eq!(bundle_info(&env.system_root.join("Components/P0.component")).version.as_deref(), Some("0.2.0"));
        assert_eq!(bundle_info(&env.user_root.join("Components/P1.component")).version.as_deref(), Some("0.2.0"));
        let file = read_installed(&env);
        assert_eq!(file.plugins["reckys-velvet"].location, Location::System);
        assert_eq!(file.plugins["reckys-rider"].location, Location::User);
        assert!(seen.borrow().contains(&"reckys-velvet:done".to_string()) && seen.borrow().contains(&"reckys-rider:done".to_string()));
        fs::remove_dir_all(&base).unwrap();
    }
}
