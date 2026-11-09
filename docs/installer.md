# Installer

`cargo xtask package` builds the Windows installer `C:\Projects\gravecord-dist\gravecord-setup.exe` and refreshes the portable folder next to it (`gravecord.exe` and `app\assets`). Nothing is published; the file stays local.

## Parts

- `crates/dc-setup` builds the setup stub (`gravecord-setup.exe`): one native Win32 window painted by hand in the app's dark colours (tokens from `dc-tokens`), no installer framework. The same exe, copied into the install folder as `uninstall.exe`, is the uninstaller.
- The payload is appended to the stub: `[stub][zstd archive][archive length u64][GRVCDSET]`. The archive holds `gravecord.exe` and `app/assets/**`. The stub never needs a rebuild to change the payload (`payload.rs`).
- `xtask package` builds `gravecord` and `dc-setup` in release with the static C runtime (`-C target-feature=+crt-static`, into `target/dist`), so neither imports VCRUNTIME140.

## Behaviour

- Per user, no administrator rights. Default folder `%LOCALAPPDATA%\Programs\gravecord`; Change opens the folder picker. A chosen folder that already holds other files gets a `gravecord` subfolder.
- Start Menu shortcut; Desktop shortcut is an unticked checkbox; Apps & features entry under `HKCU\...\Uninstall\gravecord` (icon, version, size, publisher LoreanXavier).
- Everything created is listed in `install.manifest` in the install folder; the uninstaller removes exactly that, then deletes itself. It asks whether to keep `%LOCALAPPDATA%\gravecord` (settings and sign-in), keeping by default. Installing never touches that folder.
- Updating over an install asks a running gravecord to close (WM_CLOSE), then offers Retry/Cancel if it stays open.
- WebView2 (captcha, activities) is checked; if missing the window says so and links the Microsoft page. Nothing is downloaded.
- The exe is unsigned, so SmartScreen warns on first run.
- The app and setup embed the supplied Gravecord Side-eye ICO. Setup uses its supplied 64/128px raster; the in-app icon-set preference does not change executable or shortcut resources.

## Test options

`--silent --dir <folder> --no-register` installs without a window and without shortcuts or registry. `--reg-key <name>` and `--shortcut-name <name>` change the names used, `--desktop` adds the desktop shortcut. The uninstaller takes `--silent`, `--purge-data` and `--data-dir <folder>`. `--screenshot <png> [--state welcome|welcome-webview|progress|done|failed]` renders a page and exits.

Silent install and uninstall return failure if the running app does not close; they do not open a Retry dialog.
Uninstaller cleanup passes paths as environment data to a hidden helper, removes the executable by literal path,
and removes the install directory only when empty. If the portable executable is open during packaging, the new
binary is saved beside it as `gravecord-next.exe`.

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

New shortcuts carry `gravecord.desktop` as their AppUserModelID, matching the WinRT notifier. The scratch-shortcut
unit test creates a link outside the Start Menu, reloads the property and removes the link. The property was read
back successfully. Registered installation and actual Windows toast display still need a live check; a portable
folder without an installed shortcut is not covered by this notification setup.
