<div align="center">

# gravecord

**A native Discord desktop client written in Rust. No Electron, no browser engine for the interface, the same look.**

<a href="https://www.patreon.com/loreanxavier"><img src="https://img.shields.io/badge/Support%20gravecord%20on%20Patreon-F96854?style=for-the-badge&logo=patreon&logoColor=white" alt="Support gravecord on Patreon"></a>

![Status](https://img.shields.io/badge/status-pre--1.0-e0a030?style=for-the-badge)
![Platform](https://img.shields.io/badge/platform-Windows-5865F2?style=for-the-badge&logo=windows&logoColor=white)
![Built with](https://img.shields.io/badge/Rust%20%2B%20GPUI-2b2b2b?style=for-the-badge&logo=rust&logoColor=white)
![License](https://img.shields.io/badge/license-GPL--3.0--or--later-2b2b2b?style=for-the-badge)

**[What it is](#what-it-is) · [Your data](#your-account-and-your-data) · [Plugins](#built-in-plugins) · [Status](#status) · [Features](#features) · [Install](#install) · [Updates](#automatic-updates) · [Building](#building-from-source) · [Layout](#project-layout) · [Contributing](#contributing) · [License](#license) · [Support](#support)**

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

## Your account and your data

gravecord talks to Discord. It does not talk to us: there is no gravecord server, no gravecord account and no
analytics service, so there is nothing of yours that we receive, keep or could sell.

- **What it connects to:**
  - **Discord's own services:** the API, gateway, CDN, media proxy and voice servers, the same ones the official app
    uses.
  - **With default settings, only a few other hosts:**
    - GitHub, for update checks
    - content Discord shows from other sites, loaded as the official app loads it: Spotify players and cover art,
      Twitch and YouTube previews, GIF previews, and hCaptcha when Discord asks for a captcha
  - YouTube, Twitch and TikTok players load only when you press Play.
- **What Discord receives:**
  - What it needs to work: your messages, actions and presence.
  - The same client-information header the official app sends: Windows version, system language, client version.
  - gravecord does not send the official app's background analytics: tracking events, metrics, crash reports.
  - Two buttons send diagnostics to Discord, and only when you use them: the Game Overlay's Report a Problem form and
    Upload Logs.
- **Your token:**
  - **Where it is kept:** in Windows Credential Manager, encrypted by Windows for your user account. It is never
    kept in gravecord's own files, logs or backups.
  - **Where it goes:** only to Discord. Picture and file downloads, plugin services and update checks never carry it.
- **Plugins:**
  - Every plugin is off until you turn it on, except MessageClickActions, which talks only to Discord.
  - The few plugins that use another service say so in the list below and in [PRIVACY.md](PRIVACY.md). For example,
    Translate+ sends a message's text to Google's translation service when you click Translate.
- **On your PC:** settings, a start cache (your server list and the last channel's recent messages) and picture
  caches, in `%LOCALAPPDATA%\gravecord`. Logs are written only while Debug Logging is on (nightly tester builds always
  keep one), plus a crash log if the app crashes. Nothing is uploaded unless you click Upload Logs.
- **Open source:** every statement here can be checked in the code. [PRIVACY.md](PRIVACY.md) lists each kind of
  connection the app makes and what it receives.

| Destination | Why | On by default? |
| - | - | - |
| Discord (API, gateway, CDN, media proxy, voice) | Everything the client does | Yes |
| GitHub API and downloads | Update checks and update downloads (installed copies only) | Yes |
| Spotify (images, embedded player; API only if your Discord account has Spotify connected) | Activities, Spotify link players, Listen Along | Yes, as in the official app |
| Twitch and YouTube image servers, the GIF provider's image host | Activity pictures and GIF previews | Yes, as in the official app |
| hCaptcha, Activities (`*.discordsays.com`) | Captcha when Discord asks for one; an Activity you launch | When needed |
| YouTube, Twitch, TikTok players | A video embed you press Play on | When you press Play |
| Equicord badges, UserPFP, USRBG | GlobalBadges, UserPFP and USRBG plugins | No, plugin only |
| Google Translate, serversmp (Toki Pona) | Translate+ plugin, on click | No, plugin only |
| Tenor or Giphy | GifProviderSwitcher plugin | No, plugin only |
| The Dog API, The Cat API, nekos.best, Reddit | MoreCommands plugin, when you run its commands | No, plugin only |
| `spotify.link`, `s.team` | OpenInApp plugin, to expand a short link you click | No, plugin only |

**Verifying what you run:**

- **Updates:** an installed copy downloads updates from GitHub over HTTPS, and runs one only if it matches GitHub's
  checksum and is signed with the project's Ed25519 release key, which is built into the app (see
  [Automatic updates](#automatic-updates)).
- **What the signature proves:** who published the build, not which source it came from. Releases are built by the
  maintainer, not on a public build server.
- **How to be sure:** [build it from source](#building-from-source) yourself.

## Built-in plugins

gravecord ships native ports of popular [Vencord](https://github.com/Vendicated/Vencord) and
[Equicord](https://github.com/Equicord/Equicord) plugins:

- **How they are built:** they are rewritten in Rust and compiled into the client. There is no injection, no mod
  loader and no patching of Discord's code, so a Discord update cannot break them the way it breaks a client-mod
  patch. They update with the app.
- **Defaults:** every plugin is off until you turn it on in Settings > Gravecord > Plugins. The one exception is
  MessageClickActions, which provides double-click to edit.
- **Turning them all off:** the master switch on the Gravecord page turns every plugin off at once.
- **Plugins off means official:** `cargo xtask plugins-off` checks a set of scenarios with the plugins off against
  the app without plugin code, and fails on one differing pixel.
- **Settings:** plugin ids and setting names are the originals', so an Equicord or Vencord backup imports as it is
  (see [Backup & Restore](#backup--restore)).

**Credit:** the behaviour of each port was read from the Vencord and Equicord sources (GPL-3.0-or-later, the same
licence as gravecord), plus one user plugin. Each port names the original file it follows. Thanks to their authors
and contributors.

**Coverage:** 86 plugins are ported. A few more are things gravecord already does without a plugin. 16 are planned
and not listed here. [PLUGINS.md](PLUGINS.md) has the full status table, with each plugin's source and settings.
Plugin names are the ones the Plugins page shows. Plugins marked † can reach a site outside Discord once turned on; [PRIVACY.md](PRIVACY.md) says what each one sends.

<details>
<summary><b>The 86 ported plugins</b></summary>

| Plugin | From | What it does |
| - | - | - |
| AccountPanelServerProfile | Vencord | Right-click your account panel to view your profile in the current server. |
| AlwaysAnimate | Vencord | Plays animated avatars, server icons, nameplates and gradient role names without hovering. |
| AlwaysTrust | Vencord | Opens links without the "Leaving Discord" prompt; deleting a server asks a simple confirmation instead of its typed name. |
| AnonymiseFileNames | Vencord | Renames uploaded files as they are sent, with a per-file switch. |
| BetterActivities † | Equicord | Shows activity icons in the member and DM lists, and every activity in profiles. |
| BetterBlockedUsers | Equicord | Groups blocked and ignored messages and opens a blocked person's profile without the prompt. |
| BetterCommands | Equicord | Adds /refresh to reload the command list of the open channel or server. |
| BetterForwards | Equicord | Lets you choose which attachments and embeds to forward, and shows where a forward came from. |
| BetterRoleContext | Vencord | Right-click a role header to edit the role, copy its colour, view its icon or members, or copy its id. |
| BetterRoleDot | Vencord | Click a role dot to copy its colour; show role dots and coloured names together. |
| BetterUploadButton | Vencord | Click the upload button to pick a file at once; right-click for its menu. |
| BiggerStreamPreview | Vencord | View a stream's preview picture at full size from the streamer's menu. |
| BypassStatus | Equicord | Still get notifications from chosen people, channels and servers while on Do Not Disturb. |
| CallTimer | Vencord | Shows how long you, and optionally everyone, have been in the call. |
| CancelFriendRequest | Equicord | Cancel an outgoing friend request from the person's profile or DM. |
| ClearURLs | Vencord | Removes tracking parameters from links you send or edit, with the bundled ClearURLs rules. |
| ClipsEnhancements | Equicord | Adds more clip lengths, resolutions and frame rates, and tags clips with your game. |
| ClipUpload | Equicord | Uploads a local MP4 file as a Discord clip, with its metadata. |
| ColorSighted | Vencord | Draws status dots as plain coloured circles, without the colourblind shapes. |
| CopyUserURLs | Vencord | Adds Copy User URL to user menus. |
| CustomRPC | Vencord | Sets a fully custom Rich Presence activity on your profile. |
| DisableCallIdle | Vencord | Stops a DM call from disconnecting you after three minutes alone. |
| DownloadAllAttachments | Equicord | A hover bar button that saves every attachment of a message at once. |
| ExpressionManager | Vencord | Clone stickers into your servers, and edit, delete or copy stickers and emoji from their menus. |
| FavoriteEmojiFirst | Vencord | Puts your favourite emoji first in emoji autocomplete. |
| FixImagesQuality | Vencord | Loads attachment pictures at their original resolution. |
| FixYoutubeEmbeds | Vencord | Shows a YouTube embed's description, folded until clicked. |
| FriendInvites | Vencord | Slash commands to create, view and revoke friend invite links. |
| FullVCPFP | Equicord | Makes avatars fill the whole call tile. |
| GameActivityToggle | Vencord | A panel button that turns sharing your activity on and off. |
| GifPaste | Vencord | Picking a GIF puts its link in the message box instead of sending it. |
| GifProviderSwitcher † | Vencord | Uses Tenor or Giphy for GIF search instead of Discord's provider. |
| GlobalBadges † | Equicord | Shows badges from other client mods on profiles. |
| GreetStickerPicker | Vencord | Choose which greet sticker to send instead of a random one. |
| IgnoreActivities | Vencord | Hides chosen activities from your status. |
| ImageZoom | Vencord | A magnifying lens on pictures in the viewer, sized with the wheel. |
| MemberCount | Vencord | Shows online, total and in-voice member counts in the member list and server tooltip. |
| MessageClickActions | Vencord | Double-click to edit or reply, and other click and key actions on messages. On by default. |
| MessageLogger | Vencord | Keeps deleted messages visible in red and shows the edit history of edited ones. |
| MessagePeek | Equicord | Shows the last message and its time under each DM. |
| MessageTags | Vencord | Saved text snippets you send with your own slash commands. |
| MoreCommands † | Equicord | Fifty extra slash commands: dice, coin flip, text transforms, random pictures and more. |
| MoreQuickReactions | Vencord | Shows more quick reactions in the hover bar and as a scrolling grid in the message menu. |
| MuteChannelMentions | User plugin | Hides mention badges of channels you pick. |
| NewGuildSettings | Vencord | Applies your chosen notification settings to servers you join. |
| NoNitroUpsell | Equicord | Hides Nitro upsells. Discord still applies your real plan. |
| NoPendingCount | Vencord | Removes the friend request and message request counts. |
| NoTypingAnimation | Vencord | Draws still typing dots. |
| OnePingPerDM | Vencord | Plays one sound per DM until you read it, not one per message. |
| OpenInApp † | Vencord | Opens Spotify, Steam, Epic, Tidal, Apple Music, VRChat and Telegram links in their apps. |
| PermissionsViewer | Vencord | Shows the permissions of a member, a channel or a server's roles. |
| PinDMs | Vencord | Pin DMs into categories at the top of the DM list. |
| PlatformIndicators | Equicord | Shows which clients people are on: desktop, mobile, web and others. |
| PreviewMessage | Vencord | Shows your message as it will look before you send it. |
| QuickMention | Vencord | A hover bar button that mentions the message's author. |
| ReadAllNotificationsButton | Vencord | A button that marks every server as read. |
| RelationshipNotifier | Vencord | Tells you when a friend, group DM or server removes you. |
| RevealAllSpoilers | Vencord | Ctrl-click a spoiler to open every spoiler of the message, Ctrl+Shift-click for the whole chat. |
| ReverseImageSearch † | Vencord | Search a picture with an image search engine, opened in your browser. |
| RoleColorEverywhere | Vencord | Uses role colours on mentions, role headers, voice channels and reaction lists. |
| SearchFix | Equicord | Lets search go past Discord's 5,000-result limit by flipping the sort order. |
| ServerInfo | Vencord | A Server Info dialog with friends, mutual, blocked and ignored members. |
| ServerListIndicators | Vencord | Shows how many friends are online or how many servers you are in, under Home. |
| ShowConnections | Vencord | Shows people's connected accounts as icons in profiles. |
| SilentMessageToggle | Vencord | A chat bar button that sends messages as @silent. |
| SpotifyCrack | Vencord | Lets Listen Along work without Spotify Premium and keeps Spotify playing while you talk. |
| SpotifyShareCommands | Vencord | /track, /album and /artist share what you are listening to. |
| SteamStatusSync | Equicord | Mirrors your Discord status to Steam through the Steam client. |
| StreamingCodecDisabler | Equicord | Can turn off H.264 encoding for streaming and camera. |
| SuperReactionTweaks | Vencord | Opens the reaction picker with Super Reactions on. |
| Translate+ † | Equicord | Translates messages, including Toki Pona, sitelen pona and Shavian. |
| TypingIndicator | Vencord | Shows who is typing next to the channel's name. |
| TypingTweaks | Vencord | Shows avatars and role colours in the typing line. |
| UnlimitedAccounts | Equicord | Raises the number of accounts in Switch Accounts. |
| UnlockedAvatarZoom | Vencord | Lets you zoom further when cropping an avatar. |
| UserMessagesPronouns | Vencord | Shows people's pronouns next to their messages. |
| UserPFP † | Equicord | Shows avatars from the UserPFP list, and lets you set local avatars for anyone. |
| USRBG † | Vencord | Shows profile banners from USRBG. |
| ValidUser | Vencord | Fixes @unknown-user mentions when you hover them. |
| ViewIcons | Vencord | View avatars, banners, decorations and server icons at full size. |
| VoiceDownload | Vencord | A download button on voice messages. |
| VoiceMessages | Vencord | Send voice messages from desktop, recorded or from an audio file. |
| VoiceStats | Equicord | Shows how long you have spent in voice with each person. |
| VolumeBooster | Vencord | Lets user and stream volume go past 200%. |
| WhoReacted | Vencord | Shows the avatars of who reacted next to each reaction. |
| ZipPreview † | Equicord | Lists the files inside a ZIP attachment. |

</details>

**Already native, so no plugin is needed:**

- **ForceOwnerCrown:** the owner crown shows at any server size.
- **NormalizeMessageLinks:** Copy Message Link writes plain `discord.com` links.
- **NoTrack:** gravecord sends none of Discord's background analytics, metrics or crash reports. The one analytics
  event it sends is the Game Overlay's Report a Problem form, when you submit it.

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
- [PRIVACY.md](PRIVACY.md): what is sent where, and what stays on your PC.

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
