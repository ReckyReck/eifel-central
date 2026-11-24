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

## Versions

Current: **0.2.0** (alpha). Alpha numbers 0.MINOR.PATCH: PATCH = fixes only, MINOR = new features or look.
Version lives in three files and must match: `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml`, `package.json`.

| Version | Date | What changed |
| --- | --- | --- |
| 0.1.0 | 24 Nov 2026 | Catalog window: rack units, filters, search, settings (step 5) |
| 0.2.0 | 25 Nov 2026 | INSTALL works on the Mac (step 6); test releases for testers |

## Status

| Step | What | State |
| --- | --- | --- |
| 5 | App window: catalog, filters, search, settings (design from the plug-ins) | done (0.1.0) |
| 6 | Install on the Mac: download, SHA-256, password prompt, `installed.json` | **this build (0.2.0)** – in test |
| 7 | Update, remove, DAW check, "uninstall all" | next |
| 8 | Windows (UAC helper) | |
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
install into a scratch folder, recognising installed copies). "Build app" also runs the ignored test on GitHub's Mac:
it downloads every Mac plug-in of the live catalog, checks its SHA-256, unpacks it and verifies the code signature.

## Test release for testers

github.com > eifel-central > Actions > "Build app" > Run workflow, tick **Publish a test release**. When it is green,
the app is at <https://github.com/ReckyReck/eifel-central/releases> (pre-release `central-v0.2.0`); testers need no
GitHub account. A version can only be published once – raise it for the next test build.

## Project layout

| Path | What it is |
| --- | --- |
| `src/` | The window's code (TypeScript, no framework): `catalog.ts` (load + check catalog.json), `state.ts` (settings, "new", installed), `install.ts` (starts an install, follows its progress), `view.ts` (the rack units), `i18n/en.ts` (all texts) |
| `ui/` | What the window loads: `index.html`, `css/central.css` (colour tokens from the plug-ins), `fonts/` (Inter, OFL licence), `js/` (built from `src/`) |
| `src-tauri/` | The app shell (Tauri 2, Rust): `src/main.rs` (commands for the window), `src/installer.rs` (download, checksum, unpack, copy, installed.json + tests), `tauri.conf.json` (window, security rules, bundles), `icons/` |
| `.github/workflows/build-app.yml` | Builds the Mac `.dmg` and the Windows installer on GitHub |

Build: github.com > eifel-central > Actions > "Build app" > Run workflow. Texts: all in `src/i18n/en.ts`
(English); another language = a copy of that file.
