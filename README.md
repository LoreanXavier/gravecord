<div align="center">

# gravecord

**A native Discord client written in Rust. No Electron, no browser, the same look.**

<a href="https://www.patreon.com/loreanxavier"><img src="https://img.shields.io/badge/Support%20gravecord%20on%20Patreon-F96854?style=for-the-badge&logo=patreon&logoColor=white" alt="Support gravecord on Patreon"></a>

![Status](https://img.shields.io/badge/status-early%20development-e0a030?style=for-the-badge)
![Platform](https://img.shields.io/badge/platform-Windows-5865F2?style=for-the-badge&logo=windows&logoColor=white)
![Built with](https://img.shields.io/badge/Rust%20%2B%20GPUI-2b2b2b?style=for-the-badge&logo=rust&logoColor=white)

<img src="docs/media/hero.webp" width="860" alt="gravecord moving from the login screen through chat, a voice call, a stream, the soundboard and settings">

**[Why](#why) · [What works](#what-works) · [Screens](#screens) · [Voice and video](#voice-and-video) · [What is missing](#what-is-missing) · [Building](#building) · [AI Disclosure](#ai-disclosure) · [Support](#support)**

</div>

> [!IMPORTANT]
> **gravecord is made by one person in their spare time.** If you want to see it finished, please consider supporting it on [**Patreon**](https://www.patreon.com/loreanxavier).

> [!WARNING]
> **There is no release yet.** Parts of it work well, others are not built. It is an unofficial client, not made or endorsed by Discord, and third-party clients are against Discord's terms: using one is your own decision, on your own account.

## Why

The official desktop client is a whole web browser. gravecord is the same client rebuilt as one native program: Rust from top to bottom, drawn on the GPU with [GPUI](https://github.com/zed-industries/zed), with no web page behind the interface.

The aim is not a Discord-like app with its own ideas. It is the real thing, screen for screen and pixel for pixel, and every button has to do what the button does in the official client. Each screen is compared against captures of the official build, element by element, and the frames are read side by side before it counts as done.

<table>
  <tr>
    <td align="center"><img src="docs/media/chat.webp" width="420" alt="Scrolling through a channel, then typing a mention with its suggestions"></td>
    <td align="center"><img src="docs/media/settings.webp" width="420" alt="Scrolling the Voice and Video settings while the side navigation follows"></td>
  </tr>
  <tr>
    <td align="center"><b>Chat</b><br><sub>messages, embeds, threads, @ # : suggestions</sub></td>
    <td align="center"><b>Settings</b><br><sub>the real pages, with choices that take effect</sub></td>
  </tr>
</table>

## What works

| | Area | State |
|:-:|---|---|
| ✅ | **Login** | Email and password, QR code, two-factor codes, kept in the Windows Credential Manager. Awaiting its first real login |
| ✅ | **Chat** | Messages, replies, reactions, embeds, threads, polls, uploads, paste, slash commands, search with filters, image viewer |
| ✅ | **Social** | DMs, group DMs, friends, profiles, right-click menus |
| ✅ | **Voice** | End-to-end encrypted calls (DAVE), noise suppression with DeepFilterNet, echo cancellation, push to talk, soundboard |
| ✅ | **Video** | Watching Go Live, sending Go Live, receiving camera, hardware H.264 |
| ✅ | **Servers** | Roles, AutoMod, Community, Onboarding, Discovery, member safety and the rest of server settings, forums, Events with their create flow |
| ✅ | **Presence** | Game detection with Registered Games, and Rich Presence: games that talk to Discord's local pipe show up as your activity |
| ✅ | **Desktop** | Installer, automatic updates, start with Windows, tray, taskbar badges, notifications, one instance at a time, spellcheck |
| 🟡 | **Expression** | Stickers with animation, the sticker picker, Nitro display name styles and profile effects drawn everywhere, the profile editor. Saving your own style needs Nitro and has not been tried on a real account |
| 🟡 | **Shop and Quests** | The Shop with its previews and the steps up to payment, for money and for Orbs. Quest Home without quest cards yet |
| 🟡 | **Clips** | The settings page and saving the last seconds of your own stream with a keybind. No gallery yet |
| 🟡 | **Activities** | The shelf and launching work. Opening one past its sign-in has not been tried |
| 🟡 | **Nitro** | The Nitro page and purchase dialogs up to payment. Subscriber screens are still being built |
| ⏳ | **Not yet** | See [what is missing](#what-is-missing) |

## Screens

<table>
  <tr>
    <td><img src="docs/media/login.webp" alt="The login screen with its QR code"></td>
    <td><img src="docs/media/call.webp" alt="A voice call with tiles and the control bar"></td>
  </tr>
  <tr>
    <td align="center"><b>Login</b></td>
    <td align="center"><b>Voice channel</b></td>
  </tr>
  <tr>
    <td><img src="docs/media/stream.webp" alt="A stream tile with quality and LIVE badges next to a member tile"></td>
    <td><img src="docs/media/golive.webp" alt="The Go Live source picker"></td>
  </tr>
  <tr>
    <td align="center"><b>Go Live</b><br><sub>the picture is a generated test pattern</sub></td>
    <td align="center"><b>Source picker</b></td>
  </tr>
  <tr>
    <td><img src="docs/media/soundboard.webp" alt="The soundboard picker"></td>
    <td><img src="docs/media/connection.webp" alt="The connection popout with live ping and packet loss"></td>
  </tr>
  <tr>
    <td align="center"><b>Soundboard</b></td>
    <td align="center"><b>Connection details</b><br><sub>real ping and packet loss</sub></td>
  </tr>
</table>

## Voice and video

The voice engine was tested in live calls against the official client, in both directions:

- **Audio** both ways, with the same voice privacy code on both sides, so the end-to-end encryption really is shared with official clients.
- **Go Live** watched at about 55 frames a second, and a stream from gravecord opened in the official client.
- **Camera** received and decoded in hardware.
- **Noise suppression** runs DeepFilterNet, since Krisp only loads in Discord's own signed app. The switch says what is running.
- **Reduce Room Echo** is an extra that the official client does not have.

<details>
<summary><b>Still unproven in a real call</b></summary>
<br>

- Sending our own camera to someone (capture works, the sending half has only passed a local test)
- Sharing a real screen or window as the source
- Hearing custom soundboard sounds from other servers

</details>

## What is missing

For a first release:

- [ ] A first real login, a real notification and a real update, checked on a real account
- [ ] Hours of everyday use without a crash
- [ ] DM calls: ringing and the incoming call screen
- [ ] Voice messages
- [ ] Nitro subscriber screens and limits
- [ ] The clips gallery

And after that: Stage channels, Game Overlay, quest cards, Rich Presence for browser games, then macOS and Linux.

## Building

Windows, with a current stable Rust. The interface library (GPUI) is built from a pinned Zed commit with one patch on top, checked out beside the code:

```bash
git clone https://github.com/zed-industries/zed vendor/zed
```

```bash
git -C vendor/zed checkout b5cd310b3b54dfc659fcc06453e6358843dfe216
```

```bash
git -C vendor/zed apply ../../patches/gpui.patch
```

```bash
cargo build --release -p gravecord
```

Then run `target\release\gravecord.exe` from the repository, so that it finds `app\assets`.

A few things come from the network when it runs: fonts and member artwork load from Discord's own servers, as they do in the official client. Application branding is bundled: Gravecord artwork is the default, with an Original option under Settings → Appearance. The executable and installer retain the Gravecord icon.

## AI Disclosure

AI coding tools were used in developing and debugging this client. My focus has been on matching the official client:
comparing builds with captures of it, identifying discrepancies, testing real calls and prioritizing fixes.

Noise suppression uses the DeepFilterNet machine-learning model when enabled.

## Support

gravecord is built in spare time. If you want to see it finished, this is the way to help:

<a href="https://www.patreon.com/loreanxavier"><img src="https://img.shields.io/badge/Support%20gravecord%20on%20Patreon-F96854?style=for-the-badge&logo=patreon&logoColor=white" alt="Support gravecord on Patreon"></a>
