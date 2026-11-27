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

Current: **0.3.0** (alpha). Alpha numbers 0.MINOR.PATCH: PATCH = fixes only, MINOR = new features or look.
Version lives in three files and must match: `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml`, `package.json`.

| Version | Date | What changed |
| --- | --- | --- |
| 0.1.0 | 24 Nov 2026 | Catalog window: rack units, filters, search, settings (step 5) |
| 0.2.0 | 25 Nov 2026 | INSTALL works on the Mac (step 6); test releases for testers |
| 0.3.0 | 27 Nov 2026 | INSTALL works on Windows (VST3, one UAC prompt, step 8); test release carries the Windows Setup too |

## Status

| Step | What | State |
| --- | --- | --- |
| 5 | App window: catalog, filters, search, settings (design from the plug-ins) | done (0.1.0) |
| 6 | Install on the Mac: download, SHA-256, password prompt, `installed.json` | 0.2.0 – in test |
| 7 | Update, remove, DAW check, "uninstall all" | next (Mac and Windows) |
| 8 | Windows: install VST3 with one UAC prompt, recognise installed copies | **this build (0.3.0)** – in test |
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
UPDATE, OPTIONS (reinstall/remove), UPDATE ALL and UNINSTALL ALL still show a "next build" note (step 7).

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

## Test release for testers

github.com > eifel-central > Actions > "Build app" > Run workflow, tick **Publish a test release**. When it is green,
the app is at <https://github.com/ReckyReck/eifel-central/releases> (pre-release `central-v0.3.0`, with the Mac
`.dmg` and the Windows `_x64-setup.exe`); testers need no GitHub account. A version can only be published once – raise it for the next test build.

## Project layout

| Path | What it is |
| --- | --- |
| `src/` | The window's code (TypeScript, no framework): `catalog.ts` (load + check catalog.json), `state.ts` (settings, "new", installed), `install.ts` (starts an install, follows its progress), `view.ts` (the rack units), `i18n/en.ts` (all texts) |
| `ui/` | What the window loads: `index.html`, `css/central.css` (colour tokens from the plug-ins), `fonts/` (Inter, OFL licence), `js/` (built from `src/`) |
| `src-tauri/` | The app shell (Tauri 2, Rust): `src/main.rs` (commands for the window), `src/installer.rs` (download, checksum, unpack, copy, installed.json + tests), `tauri.conf.json` (window, security rules, bundles), `icons/` |
| `.github/workflows/build-app.yml` | Builds the Mac `.dmg` and the Windows installer on GitHub |

Build: github.com > eifel-central > Actions > "Build app" > Run workflow. Texts: all in `src/i18n/en.ts`
(English); another language = a copy of that file.
