# Privacy

What gravecord sends, where it sends it, and what it keeps on your PC. Everything here can be checked in the source
code of this repository. If you find something this page does not mention, please open an issue.

## The short version

- **No servers of ours.** gravecord has no server, no account system and no analytics service of its own. Nothing is
  sent to the project or to anyone working on it, so there is nothing for us to collect, keep or sell.
- **Discord gets what the official app sends it, minus the background analytics.** Your messages, actions and
  presence go to Discord, because that is how Discord works.
- **Your token stays between your PC and Discord.** It is kept in Windows Credential Manager and sent only to
  Discord.
- **Plugins are opt-in.** Every built-in plugin but one is off by default, and that one talks only to Discord.
  The few plugins that use other services are listed below with what they send.

## What gravecord connects to

### Always: Discord

- **What:** Discord's API, its real-time gateway, its CDN and media proxy, and its voice and video servers. These are
  the same services the official app uses, with the same requests.
- **What travels:** your token goes in the requests that need it (see [Your token](#your-token)).
- **Discord's own files:** fonts, interface text, sounds and some artwork come from Discord's servers too, as they do
  in the official app.
- **Pictures in embeds:** they load through Discord's media proxy. A picture comes straight from its own site only
  when Discord's data has no proxy address for it, as in the official app.

### With default settings: a few other hosts

| Host | When | What it receives |
| - | - | - |
| GitHub (`api.github.com`, `github.com`, GitHub's download hosts) | Installed copies, at start and then hourly, to check for updates | A request for the latest release, without signing in. No account information. |
| Spotify's image server (`i.scdn.co`) | When a Spotify activity or cover is shown | A picture request |
| Twitch and YouTube image servers | When someone's activity has a Twitch or YouTube picture | A picture request |
| Spotify's web player (`open.spotify.com/embed`) | When a Spotify link's player is visible in chat, as in the official app | The player page loads in a web view |
| Spotify's API (`api.spotify.com`, `dealer.spotify.com`) | Only if your Discord account has Spotify connected, for playback state and Listen Along | A Spotify access token that Discord hands out for this purpose. Your Discord token is not sent. |
| YouTube, Twitch or TikTok players | Only after you press Play on such a video | The player page loads in a web view |
| The GIF provider's picture host | When the GIF picker shows previews. The search itself goes through Discord. | Picture requests |
| hCaptcha (`js.hcaptcha.com`) | Only when Discord asks for a captcha | The captcha widget loads in a web view |
| Activities (`*.discordsays.com`) | Only when you launch an Activity | The Activity loads in a web view |

**How these requests reach other sites:**

- They carry no Discord token.
- Like any download, they show your IP address to the site.
- They also send the same browser-style headers as requests to Discord: the client's user agent, your system language
  and your time zone setting.

**Links you click** open in your browser, and gravecord sends nothing itself. That includes the Report a Bug page,
which opens GitHub's issue form.

**What gravecord's own code does not include:**

- a crash reporter
- an analytics or telemetry library
- a font service
- an advertising or tracking service

This page covers the connections gravecord's own code makes. Pages shown inside a web view (the Spotify and
video players, the captcha, Activities) run in Microsoft's WebView2 and behave as those pages do in a browser,
including whatever they load themselves.

Noise suppression (DeepFilterNet) runs on your PC with its model built in, and downloads nothing.

### Only when you turn a plugin on

Every plugin below is off by default and contacts nothing until you switch it on.

| Plugin | Service | What it sends | When |
| - | - | - | - |
| GlobalBadges | `badges.equicord.org` (address editable), plus the hosts of the badge images it lists | A request for the badge list | While on: at start, then every 30 minutes |
| UserPFP | UserPFP's list on GitHub Pages and its images on GitHub (address editable), plus image hosts in the list | A request for the list, then the pictures | While on, when a listed person's avatar is shown |
| USRBG | `usrbg.is-hardly.online` and the image host it names | A request for the list, then banner requests. A banner request contains the Discord user id of the person whose profile you are viewing. | While on, when a listed person's profile or voice tile is shown |
| Translate+ | Google's translation service (`translate.googleapis.com`); for Toki Pona text, `aiapi.serversmp.xyz`; dictionaries from GitHub | **The text of the message you translate** | Only when you click Translate on a message |
| GifProviderSwitcher | Tenor or Giphy | Your GIF searches, and which GIF you sent | While on, when you use the GIF picker |
| MoreCommands | The Dog API, The Cat API, nekos.best, Reddit | A plain request for a random picture, sent as your message | Only when you run those commands |
| OpenInApp | `spotify.link`, `s.team` and the address they redirect to | A request that follows the short link to find the real one | When you click such a link |
| BetterActivities | Spotify's image server | Picture requests | While on, when member and DM lists show Spotify activities |
| ZipPreview | Wherever the attachment is stored (normally Discord) | A download of the ZIP file, size-capped | When you open the preview |
| ReverseImageSearch | Google Lens, Yandex, SauceNAO, IQDB, Bing, TinEye, ImgOps | Nothing from the app. It opens your browser with the picture's address. | When you choose it from the menu |

**Other plugins:**

- Every other plugin either works inside the app or talks only to Discord, with ordinary Discord requests.
- No plugin talks to a Vencord or Equicord account or cloud service.
- With the master switch on the Gravecord settings page off, every plugin is off.

## What Discord receives

**What gravecord sends to Discord:**

- **Your own activity:** messages, edits, reactions, calls, settings changes, your status, and the game you are
  playing if activity sharing is on.
  - Game detection reads the list of running programs on your PC and compares it with Discord's list of games.
  - Only a match is shared, and only as your activity.
- **A client-information header**, the same one the official app sends:
  - Windows version and build, processor architecture and system language
  - the client version
  - random ids that are new on every launch
- **Experiment assignments:** gravecord reads them, as the official app does, so its screens match the official
  ones.

**What gravecord does not send:** the background analytics the official app sends: its tracking events, performance
metrics and crash reports.

**Requests you make yourself that send diagnostic information to Discord:**

- **Game Overlay > Report a Problem:** sends the form you fill in (category, your details, the game) as Discord's
  overlay bug report.
- **Upload Logs** (Voice & Video > Advanced, or after a call survey): sends your debug log files to Discord, as the
  official button does.
- **Sending a GIF from the picker:** tells Discord which GIF you picked and what you searched for, as the official
  app does.

## Your token

- **Where it is kept:**
  - In Windows Credential Manager, under the name `gravecord`.
  - Windows encrypts it for your Windows account, so it is tied to that account.
  - gravecord never writes it to a settings file, cache, log or backup.
  - With Switch Accounts, each account's token gets its own Credential Manager entry.
- **Where it is sent:**
  - only to Discord, in the requests that need it and when connecting to the gateway
  - never with picture or file downloads, never to plugin services, and never to GitHub
- **Who can read it:** like any saved Windows credential, programs running under your own Windows account can read
  it. That is also true of the official app's saved login. Don't run programs you don't trust.
- **Logging out** removes the saved login from Credential Manager. An account kept in Switch Accounts keeps its own
  entry until you remove that account from the list.

## What stays on your PC

Everything below is in `%LOCALAPPDATA%\gravecord` and is never uploaded unless the table says so.

| What | Contents |
| - | - |
| `storage.json` | Your client settings, plugin settings and the Switch Accounts list (names and avatars, no tokens) |
| Start cache | What the first screen needs, so it draws before Discord answers: your servers, channels, friends, settings, the recent messages of the last channel you had open, and the top of its member list |
| Picture, font, sound and language caches | Files downloaded from Discord |
| Web view data | Cookies of the captcha, Activities and video players, kept as a browser would keep them |
| Debug logs | Only while Debug Logging is on (Voice & Video > Advanced), or always in a nightly tester build. The newest five files are kept. They leave your PC only if you click Upload Logs, which sends them to Discord, or attach them to a report yourself. |
| `panic.log` | The error message and code location of a crash. It is never sent anywhere. |
| Clips | Clips you save |

**Backup & Restore:**

- **Export:** writes your plugin settings and nothing else.
- **Import:** reads only plugin settings and VoiceStats' totals from an Equicord or Vencord backup. The rest of such a
  backup is never read, since it can hold other services' login tokens.

**Before you post a log:**

- A log file never contains your token.
- It can contain user ids, and the addresses of requests that failed, such as a search you ran.
- Read a log before posting it publicly.

## Updates and verifying what you run

**How the updater checks a download.** Only installed copies update themselves. Before an update runs, the app checks
all of these:

1. it came over HTTPS from GitHub (or from the host of an update feed you set yourself)
2. it matches the size and checksum GitHub reports
3. it carries an update manifest signed with the project's release key, whose public half is built into the app

A file that fails a check is deleted and never run. A swapped or tampered release therefore cannot install itself
through the updater.

**What the signature does not prove.** It shows who published an update. It does not show which source code the
update was built from.

**How releases are made:**

- Releases are built by the maintainer with `cargo xtask package`, not on a public build server.
- The setup file is not code-signed with a Windows certificate.
- There is no published release yet.

**How to be certain:** build gravecord yourself from this repository. The steps are in the README under
[Building from source](README.md#building-from-source).

## Unofficial client

gravecord is not made or endorsed by Discord. Third-party clients are against Discord's terms; using one is your own
decision, on your own account.
