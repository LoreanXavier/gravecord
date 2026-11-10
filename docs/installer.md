# Installer

`cargo xtask package` builds the Windows installer `C:\Projects\gravecord-dist\gravecord-setup.exe` and refreshes the portable folder next to it (`gravecord.exe` and `app\assets`). Nothing is published; the file stays local.

## Parts

- `crates/dc-setup` builds the setup stub (`gravecord-setup.exe`): one native Win32 window painted by hand in the app's dark colours (tokens from `dc-tokens`), no installer framework. The same exe, copied into the install folder as `uninstall.exe`, is the uninstaller.
- The payload is appended to the stub: `[stub][zstd archive][SHA-256 of the archive][archive length u64][GRVCDST2]`. The archive holds `gravecord.exe` and `app/assets/**`. The stub never needs a rebuild to change the payload (`payload.rs`). The hash is checked before anything is decompressed or written: a mismatch refuses in the window (message box) and in `--silent` (stderr, `%TEMP%\gravecord-setup.log`, exit 1) with "The setup file is damaged ... Download gravecord-setup.exe again." A setup of the first format (`GRVCDSET`, no hash) is refused the same way, with its own message, never half-read. The setup exe is read once: the stub, then the archive.
- `xtask package` builds `gravecord` and `dc-setup` in release with the static C runtime (`-C target-feature=+crt-static`, into `target/dist`), so neither imports VCRUNTIME140.

## Behaviour

- Per user, no administrator rights. Default folder `%LOCALAPPDATA%\Programs\gravecord`; Change opens the folder picker. A chosen folder that already holds other files (and is not already one of our installs) gets a `gravecord` subfolder. `--silent --dir <folder>` applies the same rule and the same refusals as the picker (`paths::resolve_from_disk`).
- Refused as install folders, checked on the chosen folder, on the folder after the subfolder rule, and on what each resolves to (junctions, 8.3 names, case, slashes): drive roots, `%USERPROFILE%` itself and the folder holding the profiles, `X:\Users`, `X:\ProgramData`, `Windows`, `Program Files`, `Program Files (x86)` (and `%SystemRoot%`/`%ProgramFiles%` as the environment names them), and `%LOCALAPPDATA%\gravecord` with everything inside it (settings and sign-in live there and the uninstaller may delete it).
- Updating over an install is staged: every file (exe, assets, uninstaller, `install.manifest`) is first written beside its target as `<name>.gc-new` and read back; only if all of that worked are they swapped in, `gravecord.exe` first and the record last, each old file parked as `<name>.gc-old` until the whole swap is done. A failure before the swap removes the staged files and leaves the old install untouched; a failure during the swap puts the earlier steps back. A crash between swaps leaves a mix that running setup again completes (it sweeps stray `.gc-new`/`.gc-old` files of the record's names first). The uninstaller removes such strays too.
- Start Menu shortcut; Desktop shortcut is an unticked checkbox; Apps & features entry under `HKCU\...\Uninstall\gravecord` (icon, version, size, publisher LoreanXavier).
- Everything created is listed in `install.manifest` in the install folder; the uninstaller removes exactly that, then deletes itself. It asks whether to keep `%LOCALAPPDATA%\gravecord` (settings and sign-in), keeping by default. Installing never touches that folder.
- Updating over an install posts a cooperative setup-quit message, followed by WM_CLOSE for older clients and windows without a tray hook. It offers Retry/Cancel if the process stays open. Older tray builds that predate this message may need Quit from their tray menu first. No process is forcibly terminated.
- WebView2 (captcha, activities) is checked; if missing the window says so and links the Microsoft page. Nothing is downloaded.
- The exe is unsigned, so SmartScreen warns on first run.
- The app and setup embed the supplied Gravecord Side-eye ICO. Setup uses its supplied 64/128px raster; the in-app icon-set preference does not change executable or shortcut resources.

## Test options

`--silent --dir <folder> --no-register` installs without a window and without shortcuts or registry. `--reg-key <name>` and `--shortcut-name <name>` change the names used, `--desktop` adds the desktop shortcut. The uninstaller takes `--silent`, `--purge-data` and `--data-dir <folder>`. `--screenshot <png> [--state welcome|welcome-webview|progress|done|failed]` renders a page and exits.

Silent install and uninstall return failure if the running app does not close; they do not open a Retry dialog.
Uninstaller cleanup: after the "gravecord was removed." dialog has been answered (or, silent, as the last act), a hidden
PowerShell helper is started that waits for the uninstaller's process id (`Wait-Process`, up to 5 minutes), then deletes
the executable by literal path with retries, then removes the install directory only when it is empty. Paths and the pid
pass as environment data, never in the script text. The running uninstaller is recognised by place, not spelling:
`UNINSTALL.EXE`, forward slashes, a junction or an 8.3 name still skip the running file, and a differently spelled
`--dir` still matches the folder it names. Running-app detection (`winsys::running`) compares image paths the same way; a
process named `gravecord.exe` whose image path cannot be read counts as possibly running (it blocks, it is never sent a
close message).

`cargo xtask package` writes into `gravecord-dist` unless `GRAVECORD_DIST_DIR` names another folder (tests of the
packaging); `GRAVECORD_PACKAGE_SKIP_BUILD=1` reuses the release build already in `target/dist`. If the portable
`gravecord.exe` is locked, nothing in the existing portable folder changes: the complete new build (exe and
`app\assets`) is written to the sibling folder `gravecord-next\` and the command says so.

## Verification, 2026-11-09

- Unit test: the 18 setup tests passed (payload, paths, manifest, update/removal and icon encoding).
- Ran live on the hidden desktop: silent install and reinstall of the packaged payload into a scratch folder with
  `--no-register`; removal with the rebuilt uninstaller preserved a foreign file and removed its own executable.
  The refreshed package was also installed into a second scratch folder: its app hash matched the portable exe,
  and its bundled uninstaller removed the installed files while preserving a foreign file.
- Frame read: the welcome window captured on the hidden desktop. Other installer pages and interactive installation
  are unverified.
- Import inspection: the refreshed packaged app and setup import no VCRUNTIME, MSVCP140 or MSVCRT DLL. This checks direct
  imports on this machine, not operation on a clean Windows installation.
- Registration, shortcuts, running-app refusal, locked portable fallback and data deletion are not live-tested.

## Desktop notification identity

New shortcuts carry `gravecord.desktop` as their AppUserModelID, matching the WinRT notifier and the process identity.
The value lives once, in `crates/dc-setup/src/app_id.rs`; the app includes that same file with `#[path]` (the app cannot
depend on the installer). Both crates run a test that pins the value. The scratch-shortcut unit test creates a link
outside the Start Menu, reloads the property and removes the link.

What the official client does (read in `design/snapshot/desktop/bootstrap/bundle.js` and desktop core):
- It sets the identity on the process at startup, `app.setAppUserModelId(Constants.APP_ID)`, with
  `APP_ID = com.squirrel.<APP_NAME>.<APP_NAME>`.
- Shortcuts get the identity from Squirrel: the `--squirrel-install` / `--squirrel-updated` hooks run
  `createShortcuts`, and `updateShortcuts` passes `app_user_model_id: bootstrapConstants.APP_ID` to
  `updater.createShortcut`.
- On first run it only repairs: `fs.existsSync(shortcutPath) && updater.createShortcut(...)` for the Desktop and
  `Programs\<Company>\<Name>.lnk` shortcuts. It never creates a shortcut that is not there.

So gravecord does not create a Start Menu shortcut on first run either. A portable folder, or an install made with
`--no-register`, has no shortcut carrying the identity, so Windows may never show its toasts. That is a documented limit,
not something half-done here. (The installer already gives every registered install the identity.)

Toast activation after the process has exited: the official bundles contain no `ToastActivatorCLSID`, no COM activator and
no toast-specific relaunch (searched for CLSID, activator and toast terms in bootstrap, desktop core and the preloads); a
notification click is handled in the running renderer. The only relaunch path in the bundle is the registered `Discord://`
protocol (`installProtocol`, `--url -- "%1"`), which toasts do not use. gravecord's shortcut has no activator CLSID and the
app implements no COM server, so a click on a toast whose process has exited does nothing. Not implemented.

The taskbar overlay description names the real count ("N notifications"). The official client builds it from the
zero-based icon index (`${index} notifications`), so it reads one too low and tops out at "9 notifications"; this is a
deliberate deviation from that bug, the words are the official ones.

## Setup shutdown and the tray

The setup-quit message marks the receiving tray binding as exiting and queues the existing Quit
action. A following WM_CLOSE bypasses close-to-tray even if another update refreshes the tray state.
The callback unit test exercises dispatch and that persistent bypass without a window, shell icon,
user profile or actual quit. Native running-app shutdown through setup remains unverified; the
silent failure/Retry path still handles uncooperative or older clients. Old setup executables only
send WM_CLOSE and cannot request this bypass.
