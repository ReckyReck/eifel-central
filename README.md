<img src="ui/icon.png" width="128" alt="Eifel Mastering Central icon">

# Eifel Mastering Central

One app for all of Recky's plug-ins (Eifel Mastering): install, update and remove them with one click, on
macOS and Windows. New plug-ins and updates appear as soon as they are published – no installer per version.

Free, no accounts, no licences, no tracking. The app reads one file,
<https://reckyreck.github.io/eifel-catalog/catalog.json>, and downloads plug-ins only from
`github.com/ReckyReck/eifel-catalog/releases`, checking every download's SHA-256 checksum.

## First start

The app is free and not registered with Apple or Microsoft, so the system asks once:

**Mac (macOS 15 and newer)**
1. Open the downloaded `.dmg` and drag **Eifel Mastering Central** into **Applications**.
2. Open the app. macOS says it cannot check it – click **Done** (right-click > Open no longer works on macOS 15).
3. Open **System Settings > Privacy & Security**, scroll down to the message about Eifel Mastering Central and
   click **Open Anyway**, then enter your password.

Only needed the first time.

**Windows 10 / 11**
1. Run the downloaded `Eifel Mastering Central_…_x64-setup.exe`.
2. If SmartScreen says "Windows protected your PC": click **More info**, then **Run anyway**.
3. Central itself installs without administrator rights. Installing a plug-in asks once per install
   ("Do you want to allow this app to make changes?" – Eifel Mastering Central, unknown publisher): click **Yes**.

## Versions

Current: **0.4.0** (alpha). Alpha numbers 0.MINOR.PATCH: PATCH = fixes only, MINOR = new features or look.
Version lives in three files and must match: `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml`, `package.json`
(plus the fallback in `src/main.ts`).

| Version | Date | What changed |
| --- | --- | --- |
| 0.1.0 | 24 Nov 2026 | Catalog window: rack units, filters, search, settings (step 5) |
| 0.2.0 | 25 Nov 2026 | INSTALL works on the Mac (step 6); test releases for testers |
| 0.3.0 | 27 Nov 2026 | INSTALL works on Windows (VST3, one UAC prompt, step 8); test release carries the Windows Setup too |
| 0.4.0 | 28 Nov 2026 | Step 7: UPDATE (with what's new), UPDATE ALL (one password prompt), OPTIONS ▾ (what's new, reinstall, remove), UNINSTALL ALL, DAW check. SEND FEEDBACK ▾ in the header and "Report a bug" / "Request a feature" in every plug-in's OPTIONS |

## Status

| Step | What | State |
| --- | --- | --- |
| 5 | App window: catalog, filters, search, settings (design from the plug-ins) | done (0.1.0) |
| 6 | Install on the Mac: download, SHA-256, password prompt, `installed.json` | done (0.2.0) |
| 7 | Update, remove, DAW check, "uninstall all" (Mac and Windows) + "Send feedback" | **this build (0.4.0)** – in test |
| 8 | Windows: install VST3 with one UAC prompt, recognise installed copies | 0.3.0 – in test |
| 9 | Self-update (signed updates) | |

## What INSTALL does (Mac, 0.2.0)

1. Downloads the plug-in's ZIP – only from `https://github.com/ReckyReck/eifel-catalog/releases/download/`, over HTTPS.
   The LED chain shows the progress; CANCEL stops it (nothing changed).
2. Checks the SHA-256 against the catalog. A mismatch stops everything with a clear message.
3. Unpacks into a private temporary folder (paths with `..` or absolute paths are refused) and checks that the
   expected bundles are inside and carry the plug-in's bundle id.
4. Copies the formats switched on in Settings (AU, VST3) into the chosen place:
   **System** = `/Library/Audio/Plug-Ins` – macOS asks for the administrator password **once** for the whole copy;
   **Only my user** = `~/Library/Audio/Plug-Ins` – no password.
   An older copy is moved aside first and put back if anything fails; it is deleted once the new one is in place.
5. Notes the install in `~/Library/Application Support/com.mystudio.central/installed.json`
   (id, version, formats, location, paths, date).
6. Restarts macOS's Audio Unit list service (`AudioComponentRegistrar`, it starts again by itself), so the DAW
   sees the new Audio Unit at its next start.

Safeguard: a plug-in found in the plug-in folders that Central did not install (or that changed since) shows
**INSTALLED (NOT BY CENTRAL)**; its button is **REPLACE…** and asks first. Central removes no quarantine flags.

Checks: `cd src-tauri && cargo test` (offline tests: request checks, safe unpacking, copy with rollback, the whole
install into a scratch folder, recognising installed copies; the Windows way – moduleinfo.json, copy with rollback,
the administrator helper's plan check – runs on every computer). "Build app" also runs the ignored test on GitHub's Mac:
it downloads every Mac plug-in of the live catalog, checks its SHA-256, unpacks it and verifies the code signature.

## What INSTALL does (Windows, 0.3.0)

Same checks as on the Mac (address, size, SHA-256, safe unpacking). Then:
1. The download must contain `<Name>.vst3` (a folder with `Contents/x86_64-win/<Name>.vst3` and
   `Contents/Resources/moduleinfo.json`). Windows VST3 bundles have no Info.plist: Central reads the **version**
   from moduleinfo.json and checks that the plug-in's class id carries Recky's maker code **Mstd**.
2. Copies it into `C:\Program Files\Common Files\VST3` – the folder every Windows DAW (Cubase/Nuendo included)
   scans. For that Central starts itself once more "as administrator" (`--copy-plugins <plan.json>`): **one** UAC
   prompt per install. That helper copies only bundles named in the plan, only into that VST3 folder.
   An older copy is renamed to `<Name>.vst3.central-old` first and renamed back if anything fails. If the old copy
   cannot be moved (a DAW has it open), Central says so: close the DAW and try again.
3. Notes the install in `%APPDATA%\com.mystudio.central\installed.json`.
4. No "Only my user" choice on Windows: the per-user VST3 folder (`%LOCALAPPDATA%\Programs\Common\VST3`) is in
   Steinberg's VST3 rules, but not every DAW scans it and Cubase cannot add VST3 folders. Central still recognises
   copies found there.
5. Unsigned app: SmartScreen asks once at the first start (More info > Run anyway); a code-signing certificate
   would remove that (costs money, not planned).

Checks on Windows: "Build app" runs `cargo test -- --include-ignored` on GitHub's Windows computer too: offline
tests, a real "Run as administrator" round trip, and download + SHA-256 + unpack of every Windows plug-in of the live
catalog (version and maker code checked, nothing installed).

## Update, remove, DAW check (step 7, 0.4.0)

* **UPDATE** opens a dialog with the new version's notes (catalog changelog), then installs it **where the old copy
  is, with the formats it has** (so no old copy is left behind). If anything fails, the old copy is put back.
* **UPDATE ALL** (header): one dialog for every plug-in with an update. Central downloads and checks them one after
  the other, then copies all of them with **one** password prompt (Mac) / **one** UAC prompt (Windows). A plug-in
  whose download fails is left out and shown with a red LED chain (hover for the reason); the others still go
  ahead. CANCEL or a cancelled prompt changes nothing.
* **OPTIONS ▾** on every plug-in: *What's new in x.y.z*, *Reinstall* (same version again, same place), *Remove…*,
  *Report a bug*, *Request a feature*. For plug-ins that are not installed or need an update, OPTIONS is the small
  key under the main button.
* **Remove…** lists the exact files first. It deletes every copy of the plug-in in both plug-in folders (also copies
  Central did not install - the dialog says so) and nothing else: Central finds them with the same check as for
  "installed" (Mac: bundle id; Windows: maker code Mstd). All-or-nothing: the bundles are moved aside first and put
  back if one cannot be moved. Mac: one password prompt for the system folder. Windows: the same administrator
  helper as INSTALL (`--copy-plugins <plan.json>`, now also with a "remove" list, only bundles directly in
  `Common Files\VST3`). `installed.json` forgets the plug-in.
* **Settings > Uninstall all**: the same for every installed plug-in, one prompt for all.
* **DAW check**: the footer shows "No DAW open" or "Open: Logic Pro" (checked every 5 seconds: Mac `ps`, Windows
  `tasklist`; Logic, MainStage, GarageBand, Live, Cubase, Nuendo, WaveLab, Studio One, REAPER, Bitwig, Pro Tools,
  FL Studio, Reason, DP, LUNA, Waveform, Ardour, Mixbus, Audition). The Update / Replace / Remove dialogs warn when a
  DAW is open. It is a warning, not a lock: on Windows a plug-in the DAW has loaded cannot be replaced (Central then
  says "is your DAW open?" and keeps the old copy).

## Send feedback (0.4.0)

* Header key **SEND FEEDBACK ▾**: *Report a bug*, *Feature request*, *Idea* - about Central itself.
  Every plug-in's **OPTIONS ▾**: *Report a bug*, *Request a feature* - about that plug-in.
* Central only opens a web address in the default browser: a new issue form in the private repo
  `ReckyReck/eifel-feedback` (GitHub Issue forms, spec in the project note plan-Central-feedback.md). No token, no
  server, no GitHub API. Filled in: title (`[Bug] Recky's Rider 0.1.0: `), labels (`type: bug,status: new,plugin:
  reckys-rider`, or `app: central`), version, and for bugs the system (`macOS 26.1 (Apple Silicon)` /
  `Windows 11 26100.2314 (x64)` / `(ARM)`). Dropdowns cannot be pre-selected by GitHub; the tester picks them.
* Security: the window has no permission to open web pages. The Rust command `open_feedback` opens only addresses
  starting with `https://github.com/ReckyReck/eifel-feedback/issues/new` (URL-encoded, at most 6000 characters).
* The repo is private (invited testers only). **Before any public release, hide or change these links.**

## Test release for testers

github.com > eifel-central > Actions > "Build app" > Run workflow, tick **Publish a test release**. When it is green,
the app is at <https://github.com/ReckyReck/eifel-central/releases> (pre-release `central-v0.4.0`, with the Mac
`.dmg` and the Windows `_x64-setup.exe`); testers need no GitHub account. A version can only be published once – raise it for the next test build.

## Project layout

| Path | What it is |
| --- | --- |
| `src/` | The window's code (TypeScript, no framework): `catalog.ts` (load + check catalog.json), `state.ts` (settings, "new", installed), `install.ts` (install / update all / remove, follows the progress), `feedback.ts` (feedback form addresses), `computer.ts` (DAW check, system), `view.ts` (rack units, menus, dialogs), `i18n/en.ts` (all texts) |
| `ui/` | What the window loads: `index.html`, `css/central.css` (colour tokens from the plug-ins), `fonts/` (Inter, OFL licence), `js/` (built from `src/`) |
| `src-tauri/` | The app shell (Tauri 2, Rust): `src/main.rs` (commands for the window), `src/installer.rs` (download, checksum, unpack, copy, remove, installed.json + tests), `src/system.rs` (DAW check, system text, feedback address check + tests), `tauri.conf.json` (window, security rules, bundles), `icons/` |
| `.github/workflows/build-app.yml` | Builds the Mac `.dmg` and the Windows installer on GitHub |

Build: github.com > eifel-central > Actions > "Build app" > Run workflow. Texts: all in `src/i18n/en.ts`
(English); another language = a copy of that file.
