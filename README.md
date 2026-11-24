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

## Status

| Step | What | State |
| --- | --- | --- |
| 5 | App window: catalog, filters, search, settings (design from the plug-ins) | **this build** |
| 6 | Install on the Mac: download, SHA-256, password prompt, `installed.json` | next |
| 7 | Update, remove, DAW check, "uninstall all" | |
| 8 | Windows (UAC helper) | |
| 9 | Self-update (signed updates) | |

In this build the INSTALL / UPDATE / OPTIONS buttons only show a note; nothing on the computer is changed.

## Project layout

| Path | What it is |
| --- | --- |
| `src/` | The window's code (TypeScript, no framework): `catalog.ts` (load + check catalog.json), `state.ts` (settings, "new", installed), `view.ts` (the rack units), `i18n/en.ts` (all texts) |
| `ui/` | What the window loads: `index.html`, `css/central.css` (colour tokens from the plug-ins), `fonts/` (Inter, OFL licence), `js/` (built from `src/`) |
| `src-tauri/` | The app shell (Tauri 2, Rust): `tauri.conf.json` (window, security rules, bundles), `icons/` |
| `.github/workflows/build-app.yml` | Builds the Mac `.dmg` and the Windows installer on GitHub |

Build: github.com > eifel-central > Actions > "Build app" > Run workflow. Texts: all in `src/i18n/en.ts`
(English); another language = a copy of that file.
