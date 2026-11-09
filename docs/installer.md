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
- The app and setup icon is a generated plain letter G mark (`crates/dc-setup/src/icon_gen.rs`, embedded by the build scripts).

## Test options

`--silent --dir <folder> --no-register` installs without a window and without shortcuts or registry. `--reg-key <name>` and `--shortcut-name <name>` change the names used, `--desktop` adds the desktop shortcut. The uninstaller takes `--silent`, `--purge-data` and `--data-dir <folder>`. `--screenshot <png> [--state welcome|welcome-webview|progress|done|failed]` renders a page and exits.
