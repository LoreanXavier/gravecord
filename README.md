<div align="center">

# gravecord

**A native Discord desktop client written in Rust. No Electron, no browser engine for the interface, the same look.**

<a href="https://www.patreon.com/loreanxavier"><img src="https://img.shields.io/badge/Support%20gravecord%20on%20Patreon-F96854?style=for-the-badge&logo=patreon&logoColor=white" alt="Support gravecord on Patreon"></a>

![Status](https://img.shields.io/badge/status-pre--1.0-e0a030?style=for-the-badge)
![Platform](https://img.shields.io/badge/platform-Windows-5865F2?style=for-the-badge&logo=windows&logoColor=white)
![Built with](https://img.shields.io/badge/Rust%20%2B%20GPUI-2b2b2b?style=for-the-badge&logo=rust&logoColor=white)
![License](https://img.shields.io/badge/license-GPL--3.0--or--later-2b2b2b?style=for-the-badge)

**[What it is](#what-it-is) · [Status](#status) · [Features](#features) · [Install](#install) · [Updates](#automatic-updates) · [Building](#building-from-source) · [Layout](#project-layout) · [Contributing](#contributing) · [License](#license) · [Support](#support)**

</div>

> [!IMPORTANT]
> **gravecord is made by one person in their spare time.** If you want to see it finished, please consider supporting it on [**Patreon**](https://www.patreon.com/loreanxavier).

> [!WARNING]
> **There is no release yet.** Parts of it work well, others are not built. It is an unofficial client, not made or endorsed by Discord, and third-party clients are against Discord's terms: using one is your own decision, on your own account.

## What it is

The official Discord desktop app is a web application inside Electron. gravecord rebuilds that client as one native
Windows program. It is Rust throughout and drawn on the GPU with [GPUI](https://github.com/zed-industries/zed), the UI
framework of the Zed editor. No web page sits behind the interface.

The goal is not a Discord-like app with its own ideas. Each screen is meant to match a pinned official build (desktop
1.0.9267, web build 667580), and each control is meant to do what it does there:

- **Pixel by pixel:** screens are compared element by element against captures of that build.
- **Same behaviour:** stores, actions, routes and strings keep their official names and come from that build.

Web content that has to stay web content (Activities, the captcha, embedded video players) runs in a separate
WebView2 host. Everything else is native.

## Status

gravecord is **pre-1.0 and Windows only**, and it has not had a public release.

The project tracks its own progress (numbers from the last generated report in [PARITY.md](PARITY.md)):

- **[FEATURES.md](FEATURES.md):** 5,230 rows, one per feature of the pinned build (actions, routes, settings, stores,
  menus, keybinds, sounds, protocol). About 140 are marked done with a test reference. Many more are built but not yet
  proven to the project's standard: a row only counts as done when it has a passing test reference.
- **Parity:** no screen is formally green yet. Pixel differences and layout probes are measured per screen; matching
  probes alone do not count as full parity.
- **[BLOCKED.md](BLOCKED.md):** 65 documented gaps, with the reason for each and, where there is one, what ships in its
  place. Some entries are older than the work that has since closed them.
- **[PLUGINS.md](PLUGINS.md):** the status of every built-in plugin.

### Known missing or incomplete

See [BLOCKED.md](BLOCKED.md) for the full list with reasons.

- **Login:** phone verification and suspended accounts lead to Discord's own pages, which are not built. A notice says
  so instead.
- **Voice and video:**
  - Krisp noise suppression only loads in Discord's own signed app, so gravecord uses DeepFilterNet instead.
  - Several Go Live cases are open: games in exclusive fullscreen, lowering resolution on a weak connection, GPU colour
    conversion.
- **Game Overlay:** some widgets and menus are not built.
- **Activities:** launching works; most SDK commands beyond the basics are not exercised.
- **Nitro:** subscriber layouts (Settings > Nitro, Nitro home) are partial.
- **Shop, Discover and Quests:** several pages and dialogs behind them are missing.
- **Other UI:**
  - Deep links for invites, gifts and templates are recognised but open nothing yet.
  - There is no screen reader support yet.
- **Platforms:** macOS and Linux are not supported.

## Screenshots

Screenshots will be added before the first release. Suggested shots, all taken on a test account in a test server:

1. The login screen with the QR code
2. A text channel with messages, embeds, replies, reactions and a thread
3. The @-mention and emoji suggestions while typing
4. A voice channel call with member tiles and the control bar
5. A Go Live stream tile next to a member tile (generated test pattern as the picture)
6. The Go Live source picker
7. The soundboard picker
8. The voice connection popout with ping and packet loss
9. User Settings > Voice & Video
10. Settings > Gravecord > Plugins, with one plugin's settings unfolded
11. Settings > Gravecord > Backup & Restore
12. The title bar's Update Ready button and Settings > Gravecord > Updates
13. The installer's welcome page

## Features

### Chat and social

- Servers, channels, categories, threads and forums, with the member list loaded in ranges as it scrolls.
- **Messages:** replies, reactions, embeds, polls, uploads with previews, paste, stickers and custom emoji.
- **Message tools:** slash commands, search with filters, the image and video viewer, markdown with spoilers, code
  blocks and mentions.
- **Voice messages:** play, record and send.
- **Direct messages:** DMs, group DMs (with their management), friends, message requests, profiles and the full
  profile, and the right-click menus.
- **Server settings:** roles, AutoMod, Community, Onboarding, member safety, invites, bans, emoji, stickers, the audit
  log and Events.
- **Presence:**
  - game detection with Registered Games
  - Rich Presence through Discord's local pipe (`\\.\pipe\discord-ipc-N`), so games that talk to Discord show up as
    your activity
  - Spotify activity cards and Listen Along
- **Interface languages:** the official message catalogs.

### Voice, video and Go Live

- Voice calls in servers, DMs and group DMs, with ringing and the incoming call card.
- **Encryption:** end-to-end (DAVE), interoperating with the official clients.
- **Audio processing:** noise suppression with DeepFilterNet, echo cancellation and Reduce Room Echo.
- **Controls:** push to talk that works while another window has focus, per-user volume, and the soundboard.
- **Video:** watching Go Live, sending Go Live, and camera, with hardware H.264 encode and decode through Media
  Foundation.
- **Clips:** saving the last seconds of your own stream with a keybind.
- **Diagnostics:** the voice connection popout shows live ping and packet loss.

### Settings

The settings window has the official pages, with choices that take effect. A separate **Gravecord** group sits before
Developer and Log Out:

- **Gravecord:** a master switch for all built-in plugins.
- **Plugins:** search and a card per plugin. Settings unfold inside the card.
- **Backup & Restore:** import and export plugin settings (see below).
- **Updates:** version, last check, download progress, Restart to Update, and the update channel (Stable or Nightly).
- **Report a Bug:** where the log files are, with Open Folder and Copy Path, and where to report.

### Built-in plugins

gravecord has native rewrites of popular Vencord and Equicord plugins. They are compiled in rather than injected, so a
Discord update cannot break them the way it breaks a client-mod patch.

- **Coverage:** of the 156 plugins on the list, 86 are ported, 9 are things gravecord already does natively, 16 are
  planned and 45 do not apply to a native client. [PLUGINS.md](PLUGINS.md) lists each one with its source and what it
  does here.
- **Defaults:** every plugin is off until you turn it on. The one exception is MessageClickActions, which provides
  double-click to edit.
- **Plugins off means official:** with the plugins off, the app is the official client.
  `cargo xtask plugins-off` captures a set of scenarios three ways and fails on one differing pixel.

### Backup & Restore

- **Import:**
  - an Equicord or Vencord backup (their Backup & Restore page, Export Settings)
  - a settings-only export
  - a file exported from gravecord
- **What an import reads:** only the plugin settings, plus VoiceStats' totals. Quick CSS, themes and the rest of the
  data store are never read: the data store can hold other services' login tokens.
- **Plugins not yet ported:** they keep their imported settings, so a later port starts from them.
- **Export:** writes gravecord's own plugin settings file.

### Desktop integration

- An installer with an uninstaller and automatic updates.
- Start with Windows, start minimized, and close to tray.
- A tray icon with voice indicators and a manual update check.
- Taskbar badges and flashing, and Windows notifications.
- One instance at a time, and spellcheck.
- The login token is kept in Windows Credential Manager, never in a file.
- Branding: Gravecord artwork by default, with an Original option under Settings > Appearance. The executable and
  installer keep the Gravecord icon.

## Install

There is no published release yet. When there is one:

### Installer

1. Download `gravecord-setup.exe` from the repository's
   [Releases](https://github.com/LoreanXavier/gravecord/releases) page.
2. Run it. It installs per user, without administrator rights, into `%LOCALAPPDATA%\Programs\gravecord` by default.
   - **Change** picks another folder.
   - The desktop shortcut is optional.
   - It adds a Start Menu shortcut and an Apps & features entry.
3. The setup checks that the WebView2 Runtime is present, which the captcha and Activities need. If it is missing,
   the setup links Microsoft's download page; it downloads nothing itself.
4. The executable is not code-signed, so Windows SmartScreen warns on first run.

Running the setup again over an existing install updates it in place and keeps your settings. Settings and sign-in
live in `%LOCALAPPDATA%\gravecord`. The uninstaller removes exactly what the setup created and asks whether to keep
that folder (keeping is the default). See [docs/installer.md](docs/installer.md).

### Portable

A portable copy is the folder with `gravecord.exe` next to its `app\assets` folder. `cargo xtask package` produces
one in `gravecord-dist` next to the setup (see [Building from source](#building-from-source)). Releases carry only the
setup file, so for now the portable folder is something you build yourself. A portable copy does not update itself.

## Automatic updates

Only an installed copy updates itself: the install folder holds the setup's `install.manifest`. Portable folders and
development builds never replace themselves.

- **When it checks:** once when the app starts, then every hour. It also checks when you use the tray's update check
  or Check for Updates on Settings > Gravecord > Updates.
- **Where it looks:** GitHub's REST API, without signing in.
  - Stable channel: `https://api.github.com/repos/LoreanXavier/gravecord/releases/latest`. Pre-releases are ignored.
  - Nightly channel: the rolling `nightly` pre-release (`/releases/tags/nightly`), whose name carries the version.
- **What a release must contain:** one asset, `gravecord-setup.exe`. The app takes the asset's SHA-256 digest from
  GitHub's answer. An unchanged release answers `304 Not Modified`, so hourly checks stay far inside GitHub's limit for
  unsigned requests.
- **What counts as an update:** only a newer version is offered. After switching channel, any other version is taken,
  because a nightly sorts below the stable release it leads to.
- **Download and checks:**
  1. The setup downloads in the background into the app's data folder. An interrupted download resumes.
  2. HTTPS only, and the file must come from GitHub's own hosts.
  3. The file must match the size and digest GitHub reports.
  4. The file must carry an update manifest signed with the project's Ed25519 release key, naming that version and
     those bytes. The public key is built into the app in `app/src/release_key.rs`.
  5. A file that fails any check is deleted and never run.
- **Installing:**
  - When the update is ready, the title bar shows **Update Ready!**.
  - Clicking it (or Restart to Update on the Updates page) runs the verified setup silently and quits. If you are in a
    voice call, it asks first.
  - The setup swaps the files with rollback on failure and starts the new version.
- **Failures:** a failed check shows nothing in the title bar, as in the official client. The Updates page shows the
  error. If there is no release, the check reports "no update".

A build made by someone else with `cargo xtask package` is signed with their own key (see below). It will refuse the
official releases, and the official builds will refuse theirs.

## Building from source

**Requirements**

- Windows, x64.
- Git.
- Rust through `rustup`. The toolchain is pinned in `rust-toolchain.toml` (1.98.1, `x86_64-pc-windows-msvc`, with
  rustfmt and clippy); rustup installs it on the first build.
- The MSVC build tools (Visual Studio Build Tools with the C++ workload), as Zed's Windows build requires.

**GPUI** is built from a pinned Zed commit with one patch on top, checked out into the gitignored `vendor/zed`:

```bash
git clone https://github.com/zed-industries/zed vendor/zed
git -C vendor/zed checkout b5cd310b3b54dfc659fcc06453e6358843dfe216
cd vendor/zed
git apply ../../patches/gpui.patch
cd ../..
```

That is the only checkout the build needs. `vendor/serein` and `vendor/yapfast` are reference checkouts for
development (see [Credits](#credits-and-attributions)); nothing compiles from them.

**Build and run**

```bash
cargo build --release -p gravecord
```

Run `target\release\gravecord.exe` from the repository root, so it finds `app\assets`.

**Workspace commands** (`cargo xtask <command>`, an alias in `.cargo/config.toml`):

| command | what it does |
| - | - |
| `package` | Builds the installer `gravecord-setup.exe` and a portable folder in `gravecord-dist` next to the repository. `--channel nightly` builds a tester build. |
| `audit` | Checks `FEATURES.md`: every row's status and test reference. |
| `report` | Regenerates the table in `PARITY.md`. |
| `parity [--screen X]` | Layout-tree and pixel comparison of a screen against the official captures. |
| `drift` | Re-captures screens and compares them with the official ones. |
| `plugins-off` | Proves that the app with its plugins off matches the app without plugin code. |
| `perf` | Startup and frame-time measurements against the budgets. `--replay <folder>` runs against a recorded session instead of Discord. |
| `replay serve <folder>` | Serves a recorded session as a local Discord. |
| `headless <exe> ...` | Runs a program on a hidden desktop, for captures. |
| `gaps` | Lists interface text of the pinned build that gravecord never shows. |
| `snapshot pin`, `client` | Pin and extract an official build, and drive the official capture client. |

`parity`, `drift`, `gaps`, `snapshot`, `client` and `report` read the `design/` folder: the extracted official build
and its captures. That folder is local only and not in the repository. The capture-based commands also need a test
account token in a gitignored `secrets.toml`. Building, running, `cargo test` and `package` need neither.

**About `cargo xtask package` and the release key.** The first time it runs it creates a signing seed in
`%USERPROFILE%\gravecord-release-keys`, or in the folder named by `GRAVECORD_RELEASE_KEYS`. It then writes the matching
public key into `app/src/release_key.rs`. Do not commit that change in a pull request. If you publish your own builds,
build with `GRAVECORD_UPDATE_REPOSITORY=https://github.com/<you>/<repo>` so your copies look for updates in your
releases.

**Before sending a change:**

```bash
cargo fmt
cargo clippy -- -D warnings
cargo test
```

## Project layout

```
app/              the GPUI application (binary `gravecord`)
  src/ui/         screens, settings pages, menus, popouts, plugin hooks
  src/plugins/    built-in plugin registry and Backup & Restore
  src/updater.rs  automatic updates
  assets/         icons, branding, sounds, Lottie files, plugin data
crates/
  dc-auth         login: password, two-factor, captcha, QR, Credential Manager
  dc-gateway      gateway connection (ETF over zstd-stream)
  dc-http         REST client
  dc-state        stores, named after the official ones
  dc-markdown     message markdown
  dc-intl         interface languages and the official formatter
  dc-tokens       design tokens of the pinned build
  dc-voice        voice and video connections, DAVE end-to-end encryption
  dc-winaudio     Windows audio pieces of the voice stack
  dc-media        camera, screen and system-audio capture, H.264 encode and decode
  dc-clips        Clips: the rolling buffer of the Go Live stream
  dc-callsim      call simulator for tests
  dc-hotkey       system-wide push to talk
  dc-detect       game detection
  dc-rpc          local RPC server for Rich Presence
  dc-overlay      game overlay
  dc-webhost      WebView2 host for Activities, the captcha and embedded players
  dc-webauthn     passkeys and security keys
  dc-proto        wire formats and client identity
  dc-release      signed update manifest
  dc-setup        the Windows installer and uninstaller
xtask/            Rust tooling (cargo xtask)
tools/            Node and Python scripts for extraction and captures
tests/            parity maps, fixtures, drift and plugins-off scenarios
patches/          the patch applied to GPUI
docs/             installer and parity-system notes
```

The project's own records:

- [FEATURES.md](FEATURES.md): every feature row with its status.
- [PARITY.md](PARITY.md): the generated progress report.
- [BLOCKED.md](BLOCKED.md): the gaps and their substitutes.
- [DECISIONS.md](DECISIONS.md): why things are the way they are.
- [PLUGINS.md](PLUGINS.md): the built-in plugins.

## Contributing

If you want to contribute, a few rules keep the project what it is:

- **Rust only** in the product. Node and Python belong in `tools/` only.
- **The official client is the reference.** A change to how something looks or behaves should match the pinned build,
  not improve on it. Colors, sizes and timings come from the design tokens, never from literals.
- **A feature comes with a test:** a unit test for logic, a replay for protocol, a parity map for UI.
- **Run the checks before sending:** `cargo fmt`, `cargo clippy -- -D warnings` and `cargo test`.
- **Never commit a token.** Never paste one in an issue, a screenshot or a log.
- **Bug reports:** Settings > Gravecord > Report a Bug shows which log files to attach. HTTP debug lines leave query
  strings out, but read a log before posting it.

## License

gravecord is free software under the GNU General Public License, version 3 or (at your option) any later version. See
[LICENSE](LICENSE). Anyone may use, change and share it; a changed version that is shared must stay open under the
same terms.

## Credits and attributions

- **[GPUI](https://github.com/zed-industries/zed)** (Apache-2.0), the UI framework of the Zed editor, built from a
  pinned commit with the patch in `patches/gpui.patch`.
- **[Serein](https://github.com/ViceVerse-cz/Serein)** (MIT OR Apache-2.0). Parts are adapted from it, and every
  adapted file names its Serein source in its header:
  - the voice engine in `crates/dc-voice`
  - the credential store
  - echo cancellation
  - DM call plumbing
  - camera capture and H.264 encoding and decoding in `crates/dc-media`
- **[Yapfast](https://github.com/yapfast/yapfast)** (GPL-3.0) was read as a protocol reference. No code was copied.
- **[Vencord](https://github.com/Vendicated/Vencord)** and **[Equicord](https://github.com/Equicord/Equicord)**
  (GPL-3.0-or-later). The built-in plugins reimplement their plugins' behaviour, read from their sources. Each port
  names the file it follows.
- **[ClearURLs](https://github.com/ClearURLs/Rules)** rules (LGPL-3.0), bundled in
  `app/assets/plugins/clearurls-rules.json`.
- **[DeepFilterNet](https://github.com/Rikorose/DeepFilterNet)** (MIT OR Apache-2.0) for noise suppression.
- Fonts and some artwork load at run time from Discord's own servers, as they do in the official client. No extracted
  Discord assets are stored in this repository.

## AI Disclosure

AI coding tools were used in developing and debugging this client. My focus has been on matching the official client:
comparing builds with captures of it, identifying discrepancies, testing real calls and prioritizing fixes.

Noise suppression uses the DeepFilterNet machine-learning model when enabled.

## Support

gravecord is built in spare time by one person. If you want to see it reach a first release and beyond, supporting it
on Patreon is the way to help:

<a href="https://www.patreon.com/loreanxavier"><img src="https://img.shields.io/badge/Support%20gravecord%20on%20Patreon-F96854?style=for-the-badge&logo=patreon&logoColor=white" alt="Support gravecord on Patreon"></a>
