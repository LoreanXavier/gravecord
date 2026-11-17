# PLUGINS.md

Built-in equivalents of the Vencord/Equicord plugins the owner keeps on. Not part of the pinned client and not
`FEATURES.md` rows. Framework: `app/src/plugins.rs` (registry, state, Backup & Restore) and the Gravecord settings
group (`app/src/ui/settings/mods.rs`). Decisions: `DECISIONS.md`, "Built-in plugins". Written by
`tools/gen-plugins-md.py`.

The list is the plugins enabled in the owner's Equicord backup of 2026-09-21 (156). Sources were read in the local
Equicord checkout of 2026-11-14 (`src/plugins` is Vencord's, `src/equicordplugins` Equicord's).

Totals: 10 ported, 3 native, 106 planned, 37 not applicable.

Status: **ported** works and has its proof; **native** is what gravecord already does without a plugin;
**planned** is not built yet (an import keeps its settings for when it is); **not applicable** has nothing to
port in a native client.

| plugin | status | source | what it does (original) | here / what it needs | proof |
| - | - | - | - | - | - |
| AccountPanelServerProfile | planned | plugins/accountPanelServerProfile/index.tsx | Right click your account panel in the bottom left to view your profile in the current server |  |  |
| AltKrispSwitch | planned | equicordplugins/altKrispSwitch/index.tsx | Makes the Noise Suppression Popout switch between None and Krisp instead of Krisp and Standard |  |  |
| AlwaysAnimate | planned | plugins/alwaysAnimate/index.ts | Animates anything that can be animated |  |  |
| AlwaysTrust | ported | plugins/alwaysTrust/index.tsx | Removes the annoying untrusted domain and suspicious file popup | links.rs: masked links open without "Leaving Discord" (`domain`). | frames trust-off/trust-on |
| AnonymiseFileNames | planned | plugins/anonymiseFileNames/index.tsx | Anonymise uploaded file names |  |  |
| AudioPlayerAPI | not applicable | equicordplugins/_api/audioPlayer.ts | API to play internal Discord audio files or external audio links. | client-mod plumbing (an API for other plugins), nothing to port. |  |
| BadgeAPI | not applicable | plugins/_api/badges/index.tsx | API to add badges to users | client-mod plumbing (an API for other plugins), nothing to port. |  |
| BetterActivities | planned | equicordplugins/betterActivities/index.tsx | Shows activity icons in the member list and allows showing all activities |  |  |
| BetterAudioPlayer | planned | equicordplugins/betterAudioPlayer.desktop/index.tsx | Adds a spectrograph and oscilloscope visualizer to audio attachment players. |  |  |
| BetterBlockedUsers | planned | equicordplugins/betterBlockedUsers/index.tsx | Allows you to search in blocked users list, make names selectable in settings, skips blocked profile prompt, and groups blocked / ignored messages. |  |  |
| BetterCommands | planned | equicordplugins/betterCommands/index.ts | Enhances the command system with miscellaneous improvements. |  |  |
| BetterFolders | planned | plugins/betterFolders/index.tsx | Shows server folders on dedicated sidebar and adds folder related improvements | folder sidebar. |  |
| BetterForwards | planned | equicordplugins/betterForwards/index.tsx | Message forward utilities including NSFW bypass and UI improvements. |  |  |
| BetterGifAltText | planned | plugins/betterGifAltText/index.ts | Change GIF alt text from simply being 'GIF' to containing the gif tags / filename |  |  |
| BetterNotesBox | planned | (not in the 2026-11-14 checkout) |  |  |  |
| BetterPlusReacts | planned | equicordplugins/betterPlusReacts/index.ts | The amount of plus before :emoji: is the message to add it to |  |  |
| BetterQuickReact | planned | (not in the 2026-11-14 checkout) |  |  |  |
| BetterRoleContext | planned | plugins/betterRoleContext/index.tsx | Adds options to copy role color / edit role / view role icon when right clicking roles in the user profile or in the member list |  |  |
| BetterRoleDot | planned | plugins/betterRoleDot/index.ts | Copy role colour on RoleDot (accessibility setting) click. Also allows using both RoleDot and coloured names simultaneously |  |  |
| BetterSettings | not applicable | plugins/betterSettings/index.tsx | Enhances your settings-menu-opening experience | settings are native here: no crossfade or lazy loading to remove. |  |
| BetterUploadButton | planned | plugins/betterUploadButton/index.ts | Upload with a single click, open menu with right click | left click uploads, right click menu. |  |
| BiggerStreamPreview | planned | plugins/biggerStreamPreview/index.tsx | This plugin allows you to enlarge stream previews |  |  |
| BypassStatus | planned | equicordplugins/bypassStatus/index.tsx | Still get notifications from specific sources when in do not disturb mode. Right-click on users/channels/guilds to set them to bypass do not disturb mode. |  |  |
| CallTimer | planned | plugins/callTimer/index.tsx | Add call timers for all users in voice channels and in the connection status. | timer in the voice panel. |  |
| CancelFriendRequest | planned | equicordplugins/pendingFriendRequest/index.tsx | Adds a way to cancel outgoing friend requests from profiles. |  |  |
| ChatInputButtonAPI | not applicable | plugins/_api/chatButtons.ts | API to add buttons to the chat input | client-mod plumbing (an API for other plugins), nothing to port. |  |
| ClearURLs | ported | plugins/clearURLs/index.ts | Automatically removes tracking elements from URLs you send | plugins/clear_urls.rs: ClearURLs rules bundled; send and edit. | `plugins::clear_urls::tests` (6); frames clearurls-off/on |
| ClipsEnhancements | planned | equicordplugins/clipsEnhancements.discordDesktop/index.tsx | Add more Clip FPS and duration options, custom clip length, RPC tagging and more |  |  |
| ClipUpload | planned | equicordplugins/clipUpload.desktop/index.tsx | Adds a button to upload a local video file as a Discord clip. |  |  |
| ColorSighted | planned | plugins/colorSighted/index.ts | Removes the colorblind-friendly icons from statuses, just like 2015-2017 Discord | status masks in member_avatars.rs. |  |
| CommandsAPI | not applicable | plugins/_api/commands.ts | Api required by anything that uses commands | client-mod plumbing (an API for other plugins), nothing to port. |  |
| ConcatenatedComponentExtractor | not applicable | plugins/_core/concatenatedComponentExtractor.tsx | Extract components that have been concatenated by the bundler | client-mod plumbing (an API for other plugins), nothing to port. |  |
| ConcatenatedModules | not applicable | equicordplugins/_core/concatenatedModules.tsx | Extract modules that have been concatenated by the bundler | client-mod plumbing (an API for other plugins), nothing to port. |  |
| ContextMenuAPI | not applicable | plugins/_api/contextMenu.ts | API for adding/removing items to/from context menus. | client-mod plumbing (an API for other plugins), nothing to port. |  |
| CopyUserURLs | planned | plugins/copyUserURLs/index.tsx | Adds a 'Copy User URL' option to the user context menu. |  |  |
| CrashHandler | not applicable | plugins/crashHandler/index.ts | Utility plugin for handling and possibly recovering from crashes without a restart | recovers React render crashes; there is no React here. |  |
| CustomRPC | planned | plugins/customRPC/index.tsx | Add a fully customisable Rich Presence (Game status) to your Discord profile | custom rich presence (needs the RPC/activity editor). |  |
| Decor | planned | plugins/decor/index.tsx | Create and use your own custom avatar decorations, or pick your favorite from the presets. | third-party avatar decorations service. |  |
| DisableCallIdle | ported | plugins/disableCallIdle/index.ts | Disables automatically getting kicked from a DM voice call after 3 minutes and being moved to an AFK voice channel. | dm_call.rs: no leave after 3 minutes alone in a DM call. | frames idle-off/idle-on (195 s alone in a simulated DM call) |
| DisableDeepLinks | not applicable | plugins/noDeepLinks.web/index.ts | Disables Discord's stupid deep linking feature which tries to force you to use their Desktop App | web build only. |  |
| DownloadAllAttachments | planned | equicordplugins/downloadAllAttachments/index.tsx | Adds a popover button to download all attachments in a message at once. |  |  |
| DynamicImageModalAPI | not applicable | plugins/_api/dynamicImageModalApi.ts | Allows you to omit either width or height when opening an image modal | client-mod plumbing (an API for other plugins), nothing to port. |  |
| EquicordHelper | not applicable | equicordplugins/equicordHelper/index.tsx | Used to provide support and fix discord caused crashes. | workarounds for Discord web crashes in the mod. |  |
| Experiments | planned | plugins/experiments/index.tsx | Enable Access to Experiments & other dev-only features in Discord! |  |  |
| ExpressionCloner | planned | (not in the 2026-11-14 checkout) |  |  |  |
| ExtraContextMenusAPI | not applicable | (not in the 2026-11-14 checkout) |  | client-mod plumbing (an API for other plugins), nothing to port. |  |
| FavoriteEmojiFirst | planned | plugins/favEmojiFirst/index.tsx | Puts your favorite emoji first in the emoji autocomplete and also has emoji alias. |  |  |
| FavoriteGifSearch | planned | (not in the 2026-11-14 checkout) |  |  |  |
| FixImagesQuality | planned | plugins/fixImagesQuality/index.tsx | Improves quality of images by loading them at their original resolution |  |  |
| FixYoutubeEmbeds | planned | plugins/fixYoutubeEmbeds.desktop/index.tsx | Bypasses youtube videos being blocked from display on Discord (for example by UMG) |  |  |
| ForceOwnerCrown | native | plugins/forceOwnerCrown/index.ts | Force the owner crown next to usernames even if the server is large. | the member list shows the owner crown at any server size. |  |
| FriendInvites | planned | plugins/friendInvites/index.ts | Create and manage friend invite links via slash commands (/create friend invite, /view friend invites, /revoke friend invites). |  |  |
| FriendsSince | planned | (not in the 2026-11-14 checkout) |  |  |  |
| FullSearchContext | planned | plugins/fullSearchContext/index.tsx | Makes the message context menu in message search results have all options you'd expect |  |  |
| FullVCPFP | planned | equicordplugins/fullVcPfp/index.tsx | Makes avatars take up the entire vc tile |  |  |
| GameActivityToggle | planned | plugins/gameActivityToggle/index.tsx | Adds a button next to the mic and deafen button to toggle game activity. Right click it to toggle Spotify activity. |  |  |
| GifMaker | planned | equicordplugins/gifMaker/index.tsx | Create and caption GIFs from any media in chat or the GIF picker. |  |  |
| GifPaste | ported | plugins/gifPaste/index.ts | Makes picking a gif in the gif picker insert a link into the chatbox instead of instantly sending it | gifs.rs: picking a GIF inserts its link and a space in the composer. | frames gifpaste-off/gifpaste-on |
| GlobalBadges | planned | equicordplugins/globalBadges/index.tsx | Adds global badges from other client mods | third-party badge service. |  |
| GreetStickerPicker | planned | plugins/greetStickerPicker/index.tsx | Allows you to use any greet sticker instead of only the random one by right-clicking the 'Wave to say hi!' button |  |  |
| HeaderBarAPI | not applicable | equicordplugins/_api/headerBar.ts | API to add buttons to the header bar and channel toolbar. | client-mod plumbing (an API for other plugins), nothing to port. |  |
| IgnoreActivities | planned | plugins/ignoreActivities/index.tsx | Ignore activities from showing up on your status ONLY. You can configure which ones are specifically ignored from the Registered Games and Activities tabs, or use the general settings below | hide listed activities. |  |
| IloveSpam | planned | (not in the 2026-11-14 checkout) |  |  |  |
| ImageZoom | planned | plugins/imageZoom/index.tsx | Lets you zoom in to images and gifs. Use scroll wheel to zoom in and shift + scroll wheel to increase lens radius / size | lens on image viewer. |  |
| LimitlessScreenshare | planned | equicordplugins/limitlessScreenshare/index.tsx | Adds a slider for screenshare resolution and fps. | resolution/fps sliders past the tier limits (owner: later, FakeNitro-style). |  |
| LimitMiddleClickPaste | planned | (not in the 2026-11-14 checkout) |  |  |  |
| MemberCount | planned | plugins/memberCount/index.tsx | Shows the number of online members, total members, and users in voice channels on the server - in the member list and tooltip. | member list header counts. |  |
| MemberListDecoratorsAPI | not applicable | plugins/_api/memberListDecorators/index.tsx | API to add decorators to member list (both in servers and DMs) | client-mod plumbing (an API for other plugins), nothing to port. |  |
| MenuItemDemanglerAPI | not applicable | (not in the 2026-11-14 checkout) |  | client-mod plumbing (an API for other plugins), nothing to port. |  |
| MessageAccessoriesAPI | not applicable | plugins/_api/messageAccessories.ts | API to add message accessories. | client-mod plumbing (an API for other plugins), nothing to port. |  |
| MessageClickActions | ported | plugins/messageClickActions/index.ts | Customize click actions on messages. | plugin_hooks.rs: double-click edit/reply/copy (`doubleClickAction`, `doubleClickOthersAction`, `doubleClickModifier`, `disableInDms`). On by default. | `ui::plugin_hooks::tests` (3); frames mca-off/on |
| MessageDecorationsAPI | not applicable | plugins/_api/messageDecorations/index.tsx | API to add decorations to messages | client-mod plumbing (an API for other plugins), nothing to port. |  |
| MessageEventsAPI | not applicable | plugins/_api/messageEvents.ts | Api required by anything using message events. | client-mod plumbing (an API for other plugins), nothing to port. |  |
| MessageLogger | planned | plugins/messageLogger/index.tsx | Temporarily logs deleted and edited messages. | deleted/edited message log (needs a store of deletes). |  |
| MessagePeek | planned | equicordplugins/messagePeek/index.tsx | Shows the last message preview and timestamp in the Direct Messages list. |  |  |
| MessagePopoverAPI | not applicable | plugins/_api/messagePopover.ts | API to add buttons to message popovers. | client-mod plumbing (an API for other plugins), nothing to port. |  |
| MessageTags | planned | (not in the 2026-11-14 checkout) |  |  |  |
| MessageUpdaterAPI | not applicable | plugins/_api/messageUpdater.ts | API for updating and re-rendering messages. | client-mod plumbing (an API for other plugins), nothing to port. |  |
| MoreCommands | planned | equicordplugins/moreCommands/index.ts | Adds various fun and useful commands |  |  |
| MoreKaomoji | planned | (not in the 2026-11-14 checkout) |  |  |  |
| MuteChannelMentions | planned | userplugins/muteChannelMentions/index.tsx | Hides mention badges from the channels you pick, both on the channel and on the server icon. Nothing is sent to Discord. |  |  |
| NewGuildSettings | planned | plugins/newGuildSettings/index.tsx | Automatically mute new servers and change various other settings upon joining | notification defaults on join. |  |
| NicknameIconsAPI | not applicable | plugins/_api/nicknameIcons.ts | API to add icons to the nickname, in profiles | client-mod plumbing (an API for other plugins), nothing to port. |  |
| NoDevtoolsWarning | not applicable | plugins/noDevtoolsWarning/index.ts | Disables the 'HOLD UP' banner in the console. As a side effect, also prevents Discord from hiding your token, which prevents random logouts. | no web console in a native client. |  |
| NoNitroUpsell | planned | equicordplugins/noNitroUpsell/index.ts | Removes all of Discord's nitro upsells by tricking the client into thinking you have nitro. |  |  |
| NoPendingCount | ported | plugins/noPendingCount/index.ts | Removes the ping count of incoming friend requests, message requests, and nitro offers. | friend request count 0 (`hideFriendRequestsCount`), no Message Requests badge (`hideMessageRequestsCount`); no Nitro offer badge exists here. | frames pending-off/pending-on |
| NoPushToTalk | planned | equicordplugins/noPushToTalk/index.ts | Bypasses the push-to-talk requirement for voice activity in channels that enforce it. |  |  |
| NormalizeMessageLinks | native | equicordplugins/normalizeMessageLinks/index.ts | Strip canary/ptb from message links | Copy Message Link already writes discord.com links. |  |
| NoticesAPI | not applicable | plugins/_api/notices.ts | Fixes notices being automatically dismissed | client-mod plumbing (an API for other plugins), nothing to port. |  |
| NoTrack | native | plugins/_core/noTrack.ts | Disable Discord's tracking (analytics/'science'), metrics and Sentry crash reporting | gravecord sends no analytics, metrics or Sentry reports; its only `/science` post is the overlay problem report the user submits. |  |
| NoTypingAnimation | ported | plugins/noTypingAnimation/index.ts | Disables the CPU-intensive typing dots animation | typing_avatar.rs: still dots, no animation frames. | none yet (headless frame not taken) |
| NoUnblockToJump | planned | plugins/noUnblockToJump/index.ts | Allows you to jump to messages of blocked or ignored users and likely spammers without unblocking them | jump to blocked users' messages. |  |
| NSFWGateBypass | planned | (not in the 2026-11-14 checkout) |  |  |  |
| OnePingPerDM | planned | plugins/onePingPerDM/index.ts | If unread messages are sent by a user in DMs multiple times, you'll only receive one audio ping. Read the messages to reset the limit | notification sound per DM. |  |
| OpenInApp | planned | plugins/openInApp/index.ts | Open links in their respective apps instead of your browser |  |  |
| PermissionFreeWill | planned | plugins/permissionFreeWill/index.ts | Disables the client-side restrictions for channel permission management. |  |  |
| PermissionsViewer | planned | plugins/permissionsViewer/index.tsx | View the permissions a user or channel has, and the roles of a server | permission breakdown in profiles. |  |
| PinDMs | planned | plugins/pinDms/index.tsx | Allows you to pin private channels to the top of your DM list. To pin/unpin or re-order pins, right click DMs | pinned DM categories. |  |
| PlatformIndicators | planned | plugins/platformIndicators/index.tsx | Adds platform indicators (Desktop, Mobile, Web...) to users | client status icons. |  |
| PreviewMessage | planned | plugins/previewMessage/index.tsx | Lets you preview your message before sending it. |  |  |
| ProfileSectionsAPI | not applicable | equicordplugins/_api/profileSections.ts | API to add sections near the 'Member Since' area of user profile panels. | client-mod plumbing (an API for other plugins), nothing to port. |  |
| QuickMention | planned | plugins/quickMention/index.tsx | Adds a quick mention button to the message actions bar |  |  |
| Quoter | planned | equicordplugins/quoter/index.tsx | Adds the ability to create an inspirational quote image from a message. |  |  |
| ReactErrorDecoder | not applicable | plugins/reactErrorDecoder/index.ts |  | decodes minified React errors; there is no React here. |  |
| ReadAllNotificationsButton | planned | plugins/readAllNotificationsButton/index.tsx | Read all server notifications with a single button click! |  |  |
| RelationshipNotifier | planned | plugins/relationshipNotifier/index.ts | Notifies you when a friend, group chat, or server removes you. |  |  |
| RevealAllSpoilers | ported | plugins/revealAllSpoilers/index.ts | Reveal all spoilers in a message by Ctrl-clicking a spoiler, or in the chat with Ctrl+Shift-click | Ctrl-click opens every text spoiler of the message, Ctrl+Shift-click of the channel. | `ui::plugin_hooks::tests`; frames spoilers-off/spoilers-on |
| ReverseImageSearch | planned | plugins/reverseImageSearch/index.tsx | Adds ImageSearch to image context menus |  |  |
| RoleColorEverywhere | planned | plugins/roleColorEverywhere/index.tsx | Adds the top role color anywhere possible | role colours in mentions, lists, voice. |  |
| SearchFix | planned | equicordplugins/searchFix/index.tsx |  |  |  |
| SedEnhanced | planned | equicordplugins/sedEnhanced/index.ts | Expands on Discord's rudimentary `sed` support. | s/a/b/ edits your last message. |  |
| ServerInfo | planned | plugins/serverInfo/index.tsx | Allows you to view info about a server |  |  |
| ServerListAPI | not applicable | plugins/_api/serverList.ts | API required for plugins that modify the server list. | client-mod plumbing (an API for other plugins), nothing to port. |  |
| ServerListIndicators | planned | plugins/serverListIndicators/index.tsx | Add online friend count or server count in the server list |  |  |
| Settings | not applicable | plugins/_core/settings.tsx | Adds Settings UI and debug info | its settings UI is the Gravecord settings group itself; `settingsLocation` not ported (the group sits before Developer). |  |
| ShowConnections | planned | plugins/showConnections/index.tsx | Show connected accounts in user popouts and the DM sidebar | connections in profiles. |  |
| ShowHiddenThings | planned | plugins/showHiddenThings/index.ts | Displays various hidden & moderator-only things regardless of permissions. |  |  |
| SilentMessageToggle | planned | plugins/silentMessageToggle/index.tsx | Adds a button to the chat bar to toggle sending a silent message. | composer button, @silent flag. |  |
| SpotifyCrack | planned | plugins/spotifyCrack/index.ts | Free listen along, no auto-pausing in voice chat, and allows activity to continue playing when idling | no auto-pause of Spotify on voice. |  |
| SpotifyShareCommands | planned | plugins/spotifyShareCommands/index.ts | Share your current Spotify track, album or artist via slash command (/track, /album, /artist) |  |  |
| SteamStatusSync | planned | equicordplugins/steamStatusSync/index.tsx | Sync your status to Steam! (Online, Away, Invisible, or Offline.) |  |  |
| StopAutoUnread | planned | equicordplugins/stopAutoUnread/index.ts |  |  |  |
| Streaks | planned | equicordplugins/streaks/index.tsx | Shows a streak next to a user when you exchange DMs with them on consecutive days. |  |  |
| StreamerModeOnStream | planned | (not in the 2026-11-14 checkout) |  |  |  |
| StreamingCodecDisabler | planned | equicordplugins/streamingCodecDisabler/index.ts | Disable video codecs of your choice for streaming and cameras. | codec choice in dc-media. |  |
| Summaries | planned | (not in the 2026-11-14 checkout) |  |  |  |
| SuperReactionTweaks | planned | plugins/superReactionTweaks/index.ts | Customize the limit of Super Reactions playing at once, and super react by default |  |  |
| SupportHelper | not applicable | plugins/_core/supportHelper.tsx | Helps us provide support to you | support-channel helper of the mod. |  |
| TenorGifSearch | planned | (not in the 2026-11-14 checkout) |  |  |  |
| ThemeAttributes | not applicable | plugins/themeAttributes/index.ts | Adds data attributes to various elements for theming purposes | adds DOM attributes for CSS themes; there is no DOM or CSS here. |  |
| TimeBarAllActivities | planned | (not in the 2026-11-14 checkout) |  |  |  |
| Translate+ | planned | equicordplugins/translatePlus/index.tsx | Vencord's translate plugin but with support for artistic languages! | message translation (needs a translation service). |  |
| TypingIndicator | planned | plugins/typingIndicator/index.tsx | Adds an indicator if someone is typing on a channel. | typing dots in the channel list. |  |
| TypingTweaks | planned | plugins/typingTweaks/index.tsx | Show avatars and role colours in the typing indicator | typing line avatars and role colours. |  |
| UnlimitedAccounts | ported | equicordplugins/unlimitedAccounts/index.ts | Increases the amount of accounts you can add. | account_switcher.rs: `maxAccounts` (0 = no limit). | frames ua-off/ua-on (5 vs 7 listed) |
| UnlockedAvatarZoom | planned | plugins/unlockedAvatarZoom/index.ts | Allows you to zoom in further in the image crop tool when changing your avatar |  |  |
| UserAreaAPI | not applicable | equicordplugins/_api/userArea.ts | API to add buttons to the user area panel. | client-mod plumbing (an API for other plugins), nothing to port. |  |
| UserMessagesPronouns | planned | plugins/userMessagesPronouns/index.ts | Adds pronouns to chat user messages |  |  |
| UserPFP | planned | equicordplugins/userpfp/index.tsx | Allows you to set local static or animated avatars for yourself and other users | third-party avatars service. |  |
| UserSettingsAPI | not applicable | plugins/_api/userSettings.ts | Patches Discord's UserSettings to expose their group and name. | client-mod plumbing (an API for other plugins), nothing to port. |  |
| USRBG | planned | plugins/usrbg/index.tsx | Displays user banners from USRBG, allowing anyone to get a banner without Nitro | third-party banners service. |  |
| ValidReply | planned | plugins/validReply/index.ts |  | fetch missing replied messages. |  |
| ValidUser | planned | plugins/validUser/index.tsx | Fix mentions for unknown users showing up as '@unknown-user' (hover over a mention to fix it) | fetch unknown mentioned users. |  |
| VCSupport | not applicable | (not in the 2026-11-14 checkout) |  | Equicord support-server helper. |  |
| ViewIcons | planned | plugins/viewIcons/index.tsx | Makes avatars and banners in user profiles clickable, adds View Icon/Banner/Avatar Decoration entries in the user, server and group channel context menu. | open avatars/icons full size. |  |
| VoiceDownload | planned | plugins/voiceDownload/index.tsx | Adds a download to voice messages. (Opens a new browser tab) | download button on voice messages. |  |
| VoiceMessages | planned | plugins/voiceMessages/index.tsx | Allows you to send voice messages like on mobile. To do so, right click the upload button and click Send Voice Message | record voice messages on desktop. |  |
| VoiceMessageTranscriber | planned | equicordplugins/voiceMessageTranscriber.desktop/index.tsx | On-device transcriptions for voice messages powered by Whisper | on-device Whisper (needs a model runner). |  |
| VoiceStats | planned | equicordplugins/voiceStats/index.tsx | Shows how long you've spent in voice with each user in their profile |  |  |
| VolumeBooster | ported | plugins/volumeBooster/index.ts | Allows you to set the user and stream volume above the default maximum | User Volume slider to 200% x `multiplier` (context menu, call tile menu); dc-voice gain cap raised. | frames vb-off/vb-on (100% at 1/2 vs 1/4 of the track) |
| WebContextMenus | not applicable | plugins/webContextMenus.web/index.ts | Re-adds context menus missing in the web version of Discord: Links & Images (Copy/Open Link/Image), Text Area (Copy, Cut, Paste, SpellCheck) | web build only (the desktop app has these menus). |  |
| WebKeybinds | not applicable | plugins/webKeybinds.web/index.ts | Re-adds keybinds missing in the web version of Discord. Only works fully on Vesktop/Equibop/Legcord, not inside your browser | web build only. |  |
| WebScreenShareFixes | not applicable | plugins/webScreenShareFixes.web/index.ts | Removes 2500kbps bitrate cap on chromium and vesktop clients and fixes CPU usage growing endlessly while screensharing. | web build only (Chromium bitrate cap). |  |
| WhoReacted | planned | plugins/whoReacted/index.tsx | Renders the avatars of users who reacted to a message | reactor avatars on reactions. |  |
| YoutubeAdblock | not applicable | plugins/youtubeAdblock.desktop/index.ts | Block ads in YouTube embeds and the WatchTogether activity via AdGuard | needs an ad filter inside the YouTube web view of embeds; deferred with the web host. |  |
| ZipPreview | planned | equicordplugins/zipPreview/index.tsx | Previews ZIP contents inside file attachments. |  |  |
