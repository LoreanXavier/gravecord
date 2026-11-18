# PLUGINS.md

Built-in equivalents of the Vencord/Equicord plugins the owner keeps on. Not part of the pinned client and not
`FEATURES.md` rows. Framework: `app/src/plugins.rs` (registry, state, Backup & Restore) and the Gravecord settings
group (`app/src/ui/settings/mods.rs`). Decisions: `DECISIONS.md`, "Built-in plugins". Written by
`tools/gen-plugins-md.py`, which fails on a missing setting or a plugin without a reason.

Plugins off is the pinned client: `cargo xtask plugins-off` captures every scenario of
`tests/plugins-off/scenarios.txt` with the plugin code skipped, with the master switch off (every plugin on and
every setting changed), and with every plugin off, and fails on one differing pixel or on any plugin effect.

The list is the plugins enabled in the owner's Equicord backup of 2026-09-21 (156). Sources were read in the local
Equicord checkout of 2026-11-14 (`src/plugins` is Vencord's, `src/equicordplugins` Equicord's).

Totals: 21 ported, 3 native, 87 planned, 45 not applicable.

Status: **ported** works and has its proof; **native** is what gravecord already does without a plugin;
**planned** is not built yet (an import keeps its settings for when it is); **not applicable** has nothing to
port in a native client, or was removed upstream.

| plugin | status | source | what it does (original) | here / what it needs | proof |
| - | - | - | - | - | - |
| AccountPanelServerProfile | planned | plugins/accountPanelServerProfile/index.tsx | Right click your account panel in the bottom left to view your profile in the current server | a right-click menu on the account panel that opens the server profile popout. |  |
| AltKrispSwitch | planned | equicordplugins/altKrispSwitch/index.tsx | Makes the Noise Suppression Popout switch between None and Krisp instead of Krisp and Standard | Krisp, which is blocked here (its signature check); without it there is nothing to switch to. |  |
| AlwaysAnimate | planned | plugins/alwaysAnimate/index.ts | Animates anything that can be animated | a switch in the image pipeline to play animated avatars, banners and emoji without hover. |  |
| AlwaysTrust | ported | plugins/alwaysTrust/index.tsx | Removes the annoying untrusted domain and suspicious file popup | links.rs: any link opens without "Leaving Discord" (`domain`); server_settings.rs: Delete Server without the typed name, behind the plugin's own "Delete server?" (`noDeleteSafety`, `confirmModal`). | `always_trust_needs_its_domain_setting`, `always_trust_deletes_servers_by_its_two_settings`; frames trust-off/on, trust-settings |
| AnonymiseFileNames | ported | plugins/anonymiseFileNames/index.tsx | Anonymise uploaded file names | uploads.rs: files are renamed as they are sent, by the original's four methods; a switch first in each upload card's bar turns it off for that file. A spoilered file stays one. Its /Spoiler command is not ported: the app runs no client-side slash commands of plugins. | `anonymised_names_follow_each_method`; frames anon-on/anon-off; plugins-off upload-card |
| AudioPlayerAPI | not applicable | equicordplugins/_api/audioPlayer.ts | API to play internal Discord audio files or external audio links. | client-mod plumbing (an API for other plugins), nothing to port. |  |
| BadgeAPI | not applicable | plugins/_api/badges/index.tsx | API to add badges to users | client-mod plumbing (an API for other plugins), nothing to port. |  |
| BetterActivities | planned | equicordplugins/betterActivities/index.tsx | Shows activity icons in the member list and allows showing all activities | activity icons in member list rows and every activity in the profile popout. |  |
| BetterAudioPlayer | planned | equicordplugins/betterAudioPlayer.desktop/index.tsx | Adds a spectrograph and oscilloscope visualizer to audio attachment players. | a spectrograph and oscilloscope in the audio attachment player (needs the decoded samples). |  |
| BetterBlockedUsers | planned | equicordplugins/betterBlockedUsers/index.tsx | Allows you to search in blocked users list, make names selectable in settings, skips blocked profile prompt, and groups blocked / ignored messages. | search in the blocked list, selectable names, no blocked-profile prompt, grouped blocked messages. |  |
| BetterCommands | planned | equicordplugins/betterCommands/index.ts | Enhances the command system with miscellaneous improvements. | changes to the slash command UI. |  |
| BetterFolders | planned | plugins/betterFolders/index.tsx | Shows server folders on dedicated sidebar and adds folder related improvements | a second server sidebar for folders. |  |
| BetterForwards | planned | equicordplugins/betterForwards/index.tsx | Message forward utilities including NSFW bypass and UI improvements. | forward dialog tweaks. |  |
| BetterGifAltText | planned | plugins/betterGifAltText/index.ts | Change GIF alt text from simply being 'GIF' to containing the gif tags / filename | GIF alt text from the tags or file name. |  |
| BetterNotesBox | not applicable | (not in the 2026-11-14 checkout) |  | removed from Equicord (last seen in dbca8ef2); the private note box is the client's own here. |  |
| BetterPlusReacts | planned | equicordplugins/betterPlusReacts/index.ts | The amount of plus before :emoji: is the message to add it to | "+:emoji:" reacting to the Nth message above, in the composer's send path. |  |
| BetterQuickReact | planned | plugins/moreQuickReactions/index.ts | Improves the quick react buttons in the message context menu. | now MoreQuickReactions upstream. more quick reaction buttons in the message context menu. |  |
| BetterRoleContext | planned | plugins/betterRoleContext/index.tsx | Adds options to copy role color / edit role / view role icon when right clicking roles in the user profile or in the member list | role context menu rows (copy colour, edit role, view icon). |  |
| BetterRoleDot | planned | plugins/betterRoleDot/index.ts | Copy role colour on RoleDot (accessibility setting) click. Also allows using both RoleDot and coloured names simultaneously | copy a role colour on its dot; dots and coloured names together. |  |
| BetterSettings | not applicable | plugins/betterSettings/index.tsx | Enhances your settings-menu-opening experience | settings are native here: no crossfade or lazy loading to remove. |  |
| BetterUploadButton | ported | plugins/betterUploadButton/index.ts | Upload with a single click, open menu with right click | ui.rs: the composer plus opens the file picker on a click and its menu on a right click. | `copy_user_urls_and_quick_mention_write_what_the_originals_write` (off); frame bub-on; plugins-off plus-click |
| BiggerStreamPreview | planned | plugins/biggerStreamPreview/index.tsx | This plugin allows you to enlarge stream previews | a larger stream preview popout. |  |
| BypassStatus | planned | equicordplugins/bypassStatus/index.tsx | Still get notifications from specific sources when in do not disturb mode. Right-click on users/channels/guilds to set them to bypass do not disturb mode. | notification and sound rules that pass Do Not Disturb for chosen users, channels and servers. |  |
| CallTimer | ported | plugins/callTimer/index.tsx | Add call timers for all users in voice channels and in the connection status. | plugin_hooks/call_timer.rs: your time under the connection panel's channel line (seconds always, code font); with `allCallTimers` everyone's time by their voice row, as the clock icon with a tooltip or, with `showWithoutHover`, as text under the name. Join times start when the app first sees someone in a channel. | `plugin_hooks::call_timer::tests` (3); frames ct-on/ct-all; plugins-off voice-profile |
| CancelFriendRequest | planned | equicordplugins/pendingFriendRequest/index.tsx | Adds a way to cancel outgoing friend requests from profiles. | a cancel button for outgoing friend requests in profiles. |  |
| ChatInputButtonAPI | not applicable | plugins/_api/chatButtons.ts | API to add buttons to the chat input | client-mod plumbing (an API for other plugins), nothing to port. |  |
| ClearURLs | ported | plugins/clearURLs/index.ts | Automatically removes tracking elements from URLs you send | plugins/clear_urls.rs: the ClearURLs rules, bundled; messages sent and edited. | `plugins::clear_urls::tests` (6); frames clearurls-off/on; plugins-off send-link |
| ClipsEnhancements | planned | equicordplugins/clipsEnhancements.discordDesktop/index.tsx | Add more Clip FPS and duration options, custom clip length, RPC tagging and more | more clip FPS and duration options in the clips settings. |  |
| ClipUpload | planned | equicordplugins/clipUpload.desktop/index.tsx | Adds a button to upload a local video file as a Discord clip. | a button to upload a local video as a clip. |  |
| ColorSighted | ported | plugins/colorSighted/index.ts | Removes the colorblind-friendly icons from statuses, just like 2015-2017 Discord | member_avatars.rs, ui.rs status dots, status_menu.rs: idle, Do Not Disturb and offline keep their colour but draw the plain disk; the mobile phone stays (the original leaves it too). | frames cs-on/cs-off; plugins-off dm-list, chat |
| CommandsAPI | not applicable | plugins/_api/commands.ts | Api required by anything that uses commands | client-mod plumbing (an API for other plugins), nothing to port. |  |
| ConcatenatedComponentExtractor | not applicable | plugins/_core/concatenatedComponentExtractor.tsx | Extract components that have been concatenated by the bundler | client-mod plumbing (an API for other plugins), nothing to port. |  |
| ConcatenatedModules | not applicable | equicordplugins/_core/concatenatedModules.tsx | Extract modules that have been concatenated by the bundler | client-mod plumbing (an API for other plugins), nothing to port. |  |
| ContextMenuAPI | not applicable | plugins/_api/contextMenu.ts | API for adding/removing items to/from context menus. | client-mod plumbing (an API for other plugins), nothing to port. |  |
| CopyUserURLs | ported | plugins/copyUserURLs/index.tsx | Adds a 'Copy User URL' option to the user context menu. | context_menu.rs: "Copy User URL" at the end of user and DM menus; copies `<https://discord.com/users/ID>` (angle brackets, as the original). | `copy_user_urls_and_quick_mention_write_what_the_originals_write`; frame url-on; plugins-off user-menu |
| CrashHandler | not applicable | plugins/crashHandler/index.ts | Utility plugin for handling and possibly recovering from crashes without a restart | recovers React render crashes; there is no React here. |  |
| CustomRPC | planned | plugins/customRPC/index.tsx | Add a fully customisable Rich Presence (Game status) to your Discord profile | a custom rich presence editor and its activity. |  |
| Decor | planned | plugins/decor/index.tsx | Create and use your own custom avatar decorations, or pick your favorite from the presets. | its third-party avatar decorations service (decor-auth in the backup's data store is not copied). |  |
| DisableCallIdle | ported | plugins/disableCallIdle/index.ts | Disables automatically getting kicked from a DM voice call after 3 minutes and being moved to an AFK voice channel. | dm_call.rs: no leave after 3 minutes alone in a DM call. The pinned client's other idle rule (`handleIdleUpdate`: leave voice after 5 h idle, move to the AFK channel) is not in the app yet, so there is nothing more to disable. | `switch_only_plugins_follow_their_switch_and_the_master`; frames idle-off/on (195 s alone) |
| DisableDeepLinks | not applicable | plugins/noDeepLinks.web/index.ts | Disables Discord's stupid deep linking feature which tries to force you to use their Desktop App | web build only. |  |
| DownloadAllAttachments | planned | equicordplugins/downloadAllAttachments/index.tsx | Adds a popover button to download all attachments in a message at once. | a hover-bar button that saves every attachment of a message. |  |
| DynamicImageModalAPI | not applicable | plugins/_api/dynamicImageModalApi.ts | Allows you to omit either width or height when opening an image modal | client-mod plumbing (an API for other plugins), nothing to port. |  |
| EquicordHelper | not applicable | equicordplugins/equicordHelper/index.tsx | Used to provide support and fix discord caused crashes. | workarounds for Discord web crashes in the mod. |  |
| Experiments | planned | plugins/experiments/index.tsx | Enable Access to Experiments & other dev-only features in Discord! | the experiments and developer pages. |  |
| ExpressionCloner | planned | plugins/expressionManager/index.tsx | Clone Emotes & Stickers. Adds context menu options to quickly clone, edit, or delete them. Also adds Copy & Save options for stickers | now ExpressionManager upstream. menu rows that clone, edit and delete emoji and stickers into your servers. |  |
| ExtraContextMenusAPI | not applicable | (not in the 2026-11-14 checkout) |  | client-mod plumbing (an API for other plugins), nothing to port. |  |
| FavoriteEmojiFirst | planned | plugins/favEmojiFirst/index.tsx | Puts your favorite emoji first in the emoji autocomplete and also has emoji alias. | favourite emoji first in the emoji autocomplete. |  |
| FavoriteGifSearch | not applicable | (not in the 2026-11-14 checkout) |  | removed from Equicord in 0001bab8: "now a default Discord feature". |  |
| FixImagesQuality | planned | plugins/fixImagesQuality/index.tsx | Improves quality of images by loading them at their original resolution | original-resolution image requests in the image pipeline. |  |
| FixYoutubeEmbeds | planned | plugins/fixYoutubeEmbeds.desktop/index.tsx | Bypasses youtube videos being blocked from display on Discord (for example by UMG) | the YouTube embed workaround inside the web host. |  |
| ForceOwnerCrown | native | plugins/forceOwnerCrown/index.ts | Force the owner crown next to usernames even if the server is large. | the member list shows the owner crown at any server size. |  |
| FriendInvites | planned | plugins/friendInvites/index.ts | Create and manage friend invite links via slash commands (/create friend invite, /view friend invites, /revoke friend invites). | slash commands for friend invite links. |  |
| FriendsSince | not applicable | (not in the 2026-11-14 checkout) |  | removed from Equicord in 36c7a837: "now a stock Discord feature"; gravecord shows Friends Since in profiles. |  |
| FullSearchContext | planned | plugins/fullSearchContext/index.tsx | Makes the message context menu in message search results have all options you'd expect | the full message menu on search results. |  |
| FullVCPFP | planned | equicordplugins/fullVcPfp/index.tsx | Makes avatars take up the entire vc tile | avatars filling voice call tiles. |  |
| GameActivityToggle | planned | plugins/gameActivityToggle/index.tsx | Adds a button next to the mic and deafen button to toggle game activity. Right click it to toggle Spotify activity. | a panel button that turns game activity on and off. |  |
| GifMaker | planned | equicordplugins/gifMaker/index.tsx | Create and caption GIFs from any media in chat or the GIF picker. | a GIF encoder and its editor. |  |
| GifPaste | ported | plugins/gifPaste/index.ts | Makes picking a gif in the gif picker insert a link into the chatbox instead of instantly sending it | gifs.rs: picking a GIF inserts its link and a space in the composer and closes the picker. | `switch_only_plugins_follow_their_switch_and_the_master`; frames gifpaste-off/on; plugins-off gif-picker |
| GlobalBadges | planned | equicordplugins/globalBadges/index.tsx | Adds global badges from other client mods | its third-party badge service. |  |
| GreetStickerPicker | planned | plugins/greetStickerPicker/index.tsx | Allows you to use any greet sticker instead of only the random one by right-clicking the 'Wave to say hi!' button | a sticker choice for the "Wave to say hi!" button. |  |
| HeaderBarAPI | not applicable | equicordplugins/_api/headerBar.ts | API to add buttons to the header bar and channel toolbar. | client-mod plumbing (an API for other plugins), nothing to port. |  |
| IgnoreActivities | planned | plugins/ignoreActivities/index.tsx | Ignore activities from showing up on your status ONLY. You can configure which ones are specifically ignored from the Registered Games and Activities tabs, or use the general settings below | a list of activities kept off your status. |  |
| IloveSpam | planned | plugins/iLoveSpam/index.ts | Do not hide messages from 'likely spammers' | now ILoveSpam upstream. messages of users flagged as likely spammers shown as any other. |  |
| ImageZoom | planned | plugins/imageZoom/index.tsx | Lets you zoom in to images and gifs. Use scroll wheel to zoom in and shift + scroll wheel to increase lens radius / size | a zoom lens in the image viewer. |  |
| LimitlessScreenshare | planned | (not in the 2026-11-14 checkout) |  | resolution and FPS sliders past the tier limits (owner: later, FakeNitro-style). |  |
| LimitMiddleClickPaste | planned | equicordplugins/middleClickTweaks/index.ts | Various middle click tweaks, such as with pasting and link opening. | now MiddleClickTweaks upstream. middle-click paste only into a focused text box, and middle-click link opening. |  |
| MemberCount | ported | plugins/memberCount/index.tsx | Shows the number of online members, total members, and users in voice channels on the server - in the member list and tooltip. | plugin_hooks/member_count.rs: online, total and in-voice counts with their tooltips at the top of the member list, online and total in the server tooltip. Not ported: the original's gateway requests (`preload` of a server's online count, member list subscriptions while the list is hidden); plugin code sends nothing, so an unopened server shows '?' online until its count arrives. | `plugin_hooks::member_count::tests` (2); frames mc-on/mc-off; plugins-off member-list |
| MemberListDecoratorsAPI | not applicable | plugins/_api/memberListDecorators/index.tsx | API to add decorators to member list (both in servers and DMs) | client-mod plumbing (an API for other plugins), nothing to port. |  |
| MenuItemDemanglerAPI | not applicable | (not in the 2026-11-14 checkout) |  | client-mod plumbing (an API for other plugins), nothing to port. |  |
| MessageAccessoriesAPI | not applicable | plugins/_api/messageAccessories.ts | API to add message accessories. | client-mod plumbing (an API for other plugins), nothing to port. |  |
| MessageClickActions | ported | plugins/messageClickActions/index.ts | Customize click actions on messages. | plugin_hooks/message_clicks.rs: the original's click state machine (single, double, triple click; keys held incl. Backspace and Delete; the three timeouts) and all 13 actions with the app's own Edit, Reply, Delete and Pin (without their dialogs, as the original), reactions, thread, quote. On by default (the owner's double-click to edit). System DMs are the Discord system account's DM (the app keeps no `system` flag). | `plugin_hooks::message_clicks::tests` (11); frames mca-backspace (Delete sent), mca-plain-click (nothing), mca-on/off, mca-settings; plugins-off double-click and its control |
| MessageDecorationsAPI | not applicable | plugins/_api/messageDecorations/index.tsx | API to add decorations to messages | client-mod plumbing (an API for other plugins), nothing to port. |  |
| MessageEventsAPI | not applicable | plugins/_api/messageEvents.ts | Api required by anything using message events. | client-mod plumbing (an API for other plugins), nothing to port. |  |
| MessageLogger | planned | plugins/messageLogger/index.tsx | Temporarily logs deleted and edited messages. | a store of deleted and edited messages and their red/edited display. |  |
| MessagePeek | planned | equicordplugins/messagePeek/index.tsx | Shows the last message preview and timestamp in the Direct Messages list. | the last message and its time under each DM in the list. |  |
| MessagePopoverAPI | not applicable | plugins/_api/messagePopover.ts | API to add buttons to message popovers. | client-mod plumbing (an API for other plugins), nothing to port. |  |
| MessageTags | planned | plugins/customCommands/index.ts | Allows you to create custom slash commands / tags | now CustomCommands upstream. saved tags sent as custom slash commands. |  |
| MessageUpdaterAPI | not applicable | plugins/_api/messageUpdater.ts | API for updating and re-rendering messages. | client-mod plumbing (an API for other plugins), nothing to port. |  |
| MoreCommands | planned | equicordplugins/moreCommands/index.ts | Adds various fun and useful commands | its slash commands. |  |
| MoreKaomoji | not applicable | (not in the 2026-11-14 checkout) |  | removed from Equicord in d7e6fcd3. |  |
| MuteChannelMentions | planned | userplugins/muteChannelMentions/index.tsx | Hides mention badges from the channels you pick, both on the channel and on the server icon. Nothing is sent to Discord. | mention badges hidden for chosen channels. |  |
| NewGuildSettings | planned | plugins/newGuildSettings/index.tsx | Automatically mute new servers and change various other settings upon joining | notification defaults applied when joining a server. |  |
| NicknameIconsAPI | not applicable | plugins/_api/nicknameIcons.ts | API to add icons to the nickname, in profiles | client-mod plumbing (an API for other plugins), nothing to port. |  |
| NoDevtoolsWarning | not applicable | plugins/noDevtoolsWarning/index.ts | Disables the 'HOLD UP' banner in the console. As a side effect, also prevents Discord from hiding your token, which prevents random logouts. | no web console in a native client. |  |
| NoNitroUpsell | planned | equicordplugins/noNitroUpsell/index.ts | Removes all of Discord's nitro upsells by tricking the client into thinking you have nitro. | every Nitro upsell hidden (the app draws them from the pinned client). |  |
| NoPendingCount | ported | plugins/noPendingCount/index.ts | Removes the ping count of incoming friend requests, message requests, and nitro offers. | unread_badges.rs and home.rs: friend request count 0, no Message Requests badge (the row stays). | `no_pending_count_hides_each_badge_by_its_setting`; frames pending-off/on; plugins-off requests |
| NoPushToTalk | planned | equicordplugins/noPushToTalk/index.ts | Bypasses the push-to-talk requirement for voice activity in channels that enforce it. | voice activity allowed where push to talk is required. |  |
| NormalizeMessageLinks | native | equicordplugins/normalizeMessageLinks/index.ts | Strip canary/ptb from message links | Copy Message Link already writes discord.com links. |  |
| NoticesAPI | not applicable | plugins/_api/notices.ts | Fixes notices being automatically dismissed | client-mod plumbing (an API for other plugins), nothing to port. |  |
| NoTrack | native | plugins/_core/noTrack.ts | Disable Discord's tracking (analytics/'science'), metrics and Sentry crash reporting | gravecord sends no analytics, metrics or Sentry reports; its only `/science` post is the overlay problem report the user submits. |  |
| NoTypingAnimation | ported | plugins/noTypingAnimation/index.ts | Disables the CPU-intensive typing dots animation | typing_avatar.rs: every typing indicator draws still dots and asks for no animation frames. | `switch_only_plugins_follow_their_switch_and_the_master`; plugins-off typing |
| NoUnblockToJump | planned | plugins/noUnblockToJump/index.ts | Allows you to jump to messages of blocked or ignored users and likely spammers without unblocking them | jumping to blocked users' messages without unblocking. |  |
| NSFWGateBypass | not applicable | (not in the 2026-11-14 checkout) |  | removed from Equicord in 9a3c66ab. |  |
| OnePingPerDM | ported | plugins/onePingPerDM/index.ts | If unread messages are sent by a user in DMs multiple times, you'll only receive one audio ping. Read the messages to reset the limit | notifications.rs: in the DMs it affects only the oldest unread message (the first after the channel's ack) plays the sound; ignored users, allowed mentions and @everyone still ping; the desktop notification is left alone; alwaysPlaySound plays the message sound for any message that notifies. | `one_ping_per_dm_lets_the_oldest_unread_ping`; no frame (a sound, not a picture) |
| OpenInApp | planned | plugins/openInApp/index.ts | Open links in their respective apps instead of your browser | Spotify, Steam and other links opened in their apps. |  |
| PermissionFreeWill | planned | plugins/permissionFreeWill/index.ts | Disables the client-side restrictions for channel permission management. | no client-side limits in channel permission editing. |  |
| PermissionsViewer | planned | plugins/permissionsViewer/index.tsx | View the permissions a user or channel has, and the roles of a server | a permission breakdown in profiles and menus. |  |
| PinDMs | planned | plugins/pinDms/index.tsx | Allows you to pin private channels to the top of your DM list. To pin/unpin or re-order pins, right click DMs | pinned DM categories in the DM list. |  |
| PlatformIndicators | planned | plugins/platformIndicators/index.tsx | Adds platform indicators (Desktop, Mobile, Web...) to users | desktop, mobile and web icons by names. |  |
| PreviewMessage | planned | plugins/previewMessage/index.tsx | Lets you preview your message before sending it. | a preview of the message before sending. |  |
| ProfileSectionsAPI | not applicable | equicordplugins/_api/profileSections.ts | API to add sections near the 'Member Since' area of user profile panels. | client-mod plumbing (an API for other plugins), nothing to port. |  |
| QuickMention | ported | plugins/quickMention/index.tsx | Adds a quick mention button to the message actions bar | hover_bar.rs: an @ button after the quick reactions and before Add Reaction, where you may write; inserts `<@ID> ` in the composer (shown as text: the composer draws no mention pill for typed syntax). | `quick_mention_sits_before_add_reaction`; frames qm-on, qm-click; plugins-off hover-bar |
| Quoter | planned | equicordplugins/quoter/index.tsx | Adds the ability to create an inspirational quote image from a message. | a quote image made from a message. |  |
| ReactErrorDecoder | not applicable | plugins/reactErrorDecoder/index.ts |  | decodes minified React errors; there is no React here. |  |
| ReadAllNotificationsButton | planned | plugins/readAllNotificationsButton/index.tsx | Read all server notifications with a single button click! | a button that marks every server read. |  |
| RelationshipNotifier | planned | plugins/relationshipNotifier/index.ts | Notifies you when a friend, group chat, or server removes you. | notices when a friend, group or server removes you (its data store lists are not copied). |  |
| RevealAllSpoilers | ported | plugins/revealAllSpoilers/index.ts | Reveal all spoilers in a message by Ctrl-clicking a spoiler, or in the chat with Ctrl+Shift-click | ui.rs: Ctrl-click a hidden spoiler opens every text spoiler of its message, Ctrl+Shift-click those of every message the chat holds now (later ones stay hidden). | `ctrl_reveals_the_message_and_ctrl_shift_every_loaded_message`; frames spoilers-off/on |
| ReverseImageSearch | ported | plugins/reverseImageSearch/index.tsx | Adds ImageSearch to image context menus | menu.rs: "Search Image" at the end of the link group when a picture is right-clicked; a submenu of its seven engines and All, opened in the browser. The engine favicons (fetched from DuckDuckGo by the original) are not drawn: plugin code fetches nothing. | `image_search_opens_each_engine_with_the_encoded_address`; frame ris-on; plugins-off image-menu |
| RoleColorEverywhere | planned | plugins/roleColorEverywhere/index.tsx | Adds the top role color anywhere possible | role colours in mentions, lists, voice and reactions. |  |
| SearchFix | planned | equicordplugins/searchFix/index.tsx |  | its search request fix. |  |
| SedEnhanced | planned | equicordplugins/sedEnhanced/index.ts | Expands on Discord's rudimentary `sed` support. | `s/a/b/` editing the last message, in the send path. |  |
| ServerInfo | planned | plugins/serverInfo/index.tsx | Allows you to view info about a server | a server information dialog. |  |
| ServerListAPI | not applicable | plugins/_api/serverList.ts | API required for plugins that modify the server list. | client-mod plumbing (an API for other plugins), nothing to port. |  |
| ServerListIndicators | planned | plugins/serverListIndicators/index.tsx | Add online friend count or server count in the server list | online friend and server counts by the server list. |  |
| Settings | not applicable | plugins/_core/settings.tsx | Adds Settings UI and debug info | its settings UI is the Gravecord settings group itself; `settingsLocation` not ported (the group sits before Developer). |  |
| ShowConnections | planned | plugins/showConnections/index.tsx | Show connected accounts in user popouts and the DM sidebar | connected accounts in user popouts and the DM sidebar. |  |
| ShowHiddenThings | planned | plugins/showHiddenThings/index.ts | Displays various hidden & moderator-only things regardless of permissions. | hidden and moderator-only things shown. |  |
| SilentMessageToggle | planned | plugins/silentMessageToggle/index.tsx | Adds a button to the chat bar to toggle sending a silent message. | the pinned client's `@silent` prefix first: its send path strips it and sets SUPPRESS_NOTIFICATIONS (the `silentPrefix` rule and the sends of modules 857069 and 92961); the app sends the text as typed, so the plugin's prefix would reach people as words. |  |
| SpotifyCrack | planned | plugins/spotifyCrack/index.ts | Free listen along, no auto-pausing in voice chat, and allows activity to continue playing when idling | listen along without Premium and no Spotify pause on voice. |  |
| SpotifyShareCommands | planned | plugins/spotifyShareCommands/index.ts | Share your current Spotify track, album or artist via slash command (/track, /album, /artist) | its /track, /album, /artist commands. |  |
| SteamStatusSync | planned | equicordplugins/steamStatusSync/index.tsx | Sync your status to Steam! (Online, Away, Invisible, or Offline.) | status sync with the Steam client. |  |
| StopAutoUnread | planned | equicordplugins/stopAutoUnread/index.ts |  | no automatic unread marking. |  |
| Streaks | planned | equicordplugins/streaks/index.tsx | Shows a streak next to a user when you exchange DMs with them on consecutive days. | DM streak counts (its service login in the data store is not copied). |  |
| StreamerModeOnStream | not applicable | (not in the 2026-11-14 checkout) |  | no longer in the Equicord checkout of 2026-11-14 (added in 9e0aa4b2, since dropped). |  |
| StreamingCodecDisabler | planned | equicordplugins/streamingCodecDisabler/index.ts | Disable video codecs of your choice for streaming and cameras. | a codec choice for streams and cameras in dc-media. |  |
| Summaries | not applicable | (not in the 2026-11-14 checkout) |  | removed from Equicord in 452241cf: "Discord killed this feature". |  |
| SuperReactionTweaks | planned | plugins/superReactionTweaks/index.ts | Customize the limit of Super Reactions playing at once, and super react by default | super reaction limits and default. |  |
| SupportHelper | not applicable | plugins/_core/supportHelper.tsx | Helps us provide support to you | support-channel helper of the mod. |  |
| TenorGifSearch | planned | plugins/gifProviderSwitcher/index.tsx | Allows you to use Tenor or Giphy GIF search instead of Klipy | now GifProviderSwitcher upstream. Tenor or Giphy as the GIF search instead of the default provider. |  |
| ThemeAttributes | not applicable | plugins/themeAttributes/index.ts | Adds data attributes to various elements for theming purposes | adds DOM attributes for CSS themes; there is no DOM or CSS here. |  |
| TimeBarAllActivities | not applicable | (not in the 2026-11-14 checkout) |  | removed from Equicord in bcfef05a: "now a stock feature". |  |
| Translate+ | planned | equicordplugins/translatePlus/index.tsx | Vencord's translate plugin but with support for artistic languages! | message translation (needs a translation service). |  |
| TypingIndicator | planned | plugins/typingIndicator/index.tsx | Adds an indicator if someone is typing on a channel. | typing dots by channels in the channel list. |  |
| TypingTweaks | planned | plugins/typingTweaks/index.tsx | Show avatars and role colours in the typing indicator | avatars and role colours in the typing line. |  |
| UnlimitedAccounts | ported | equicordplugins/unlimitedAccounts/index.ts | Increases the amount of accounts you can add. | account_switcher.rs: `maxAccounts` (0 = no limit) for the list and the Add button. | `unlimited_accounts_lifts_the_five_account_limit`; frames ua-off/ua-on; plugins-off accounts |
| UnlockedAvatarZoom | planned | plugins/unlockedAvatarZoom/index.ts | Allows you to zoom in further in the image crop tool when changing your avatar | a wider zoom in the avatar crop tool. |  |
| UserAreaAPI | not applicable | equicordplugins/_api/userArea.ts | API to add buttons to the user area panel. | client-mod plumbing (an API for other plugins), nothing to port. |  |
| UserMessagesPronouns | planned | plugins/userMessagesPronouns/index.ts | Adds pronouns to chat user messages | pronouns by names in chat. |  |
| UserPFP | planned | equicordplugins/userpfp/index.tsx | Allows you to set local static or animated avatars for yourself and other users | its third-party avatar service. |  |
| UserSettingsAPI | not applicable | plugins/_api/userSettings.ts | Patches Discord's UserSettings to expose their group and name. | client-mod plumbing (an API for other plugins), nothing to port. |  |
| USRBG | planned | plugins/usrbg/index.tsx | Displays user banners from USRBG, allowing anyone to get a banner without Nitro | its third-party banner service. |  |
| ValidReply | planned | plugins/validReply/index.ts |  | replied messages fetched when missing. |  |
| ValidUser | planned | plugins/validUser/index.tsx | Fix mentions for unknown users showing up as '@unknown-user' (hover over a mention to fix it) | unknown mentioned users fetched. |  |
| VCSupport | not applicable | (not in the 2026-11-14 checkout) |  | Equicord support-server helper. |  |
| ViewIcons | planned | plugins/viewIcons/index.tsx | Makes avatars and banners in user profiles clickable, adds View Icon/Banner/Avatar Decoration entries in the user, server and group channel context menu. | avatars, banners and icons opened full size, with menu rows. |  |
| VoiceDownload | ported | plugins/voiceDownload/index.tsx | Adds a download to voice messages. (Opens a new browser tab) | voice_message.rs: a download arrow after a voice message's volume that opens the file in the browser (the original's desktop behaviour). | frames vd-on/vd-off/vd-pair; plugins-off chat (the voice message is in view) |
| VoiceMessages | planned | plugins/voiceMessages/index.tsx | Allows you to send voice messages like on mobile. To do so, right click the upload button and click Send Voice Message | recording voice messages on desktop. |  |
| VoiceMessageTranscriber | planned | equicordplugins/voiceMessageTranscriber.desktop/index.tsx | On-device transcriptions for voice messages powered by Whisper | an on-device Whisper model runner. |  |
| VoiceStats | ported | equicordplugins/voiceStats/index.tsx | Shows how long you've spent in voice with each user in their profile | plugin_hooks/voice_stats.rs: seconds with each person in your voice channel, saved every 30 s; "Voice Time" after Member Since in the DM sidebar and the full profile. An Equicord backup brings its totals (`VoiceStats_totals`, the one data store key read). | `plugin_hooks::voice_stats::tests` (5), backup import test; frames vs-dm-on/off, vs-full-on |
| VolumeBooster | ported | plugins/volumeBooster/index.ts | Allows you to set the user and stream volume above the default maximum | User Volume (context menu, call tile menu, call tray) and Stream Volume sliders to their top x `multiplier`; the voice engine takes the higher gain. The app syncs no volumes to the account, so the original's 200% cap on the sync has nothing to cap. | `volume_booster_raises_the_slider_top_by_its_multiplier`; frames vb-off/vb-on; plugins-off user-volume |
| WebContextMenus | not applicable | plugins/webContextMenus.web/index.ts | Re-adds context menus missing in the web version of Discord: Links & Images (Copy/Open Link/Image), Text Area (Copy, Cut, Paste, SpellCheck) | web build only (the desktop app has these menus). |  |
| WebKeybinds | not applicable | plugins/webKeybinds.web/index.ts | Re-adds keybinds missing in the web version of Discord. Only works fully on Vesktop/Equibop/Legcord, not inside your browser | web build only. |  |
| WebScreenShareFixes | not applicable | plugins/webScreenShareFixes.web/index.ts | Removes 2500kbps bitrate cap on chromium and vesktop clients and fixes CPU usage growing endlessly while screensharing. | web build only (Chromium bitrate cap). |  |
| WhoReacted | planned | plugins/whoReacted/index.tsx | Renders the avatars of users who reacted to a message | reactor avatars on reactions. |  |
| YoutubeAdblock | not applicable | plugins/youtubeAdblock.desktop/index.ts | Block ads in YouTube embeds and the WatchTogether activity via AdGuard | needs an ad filter inside the YouTube web view of embeds; deferred with the web host. |  |
| ZipPreview | planned | equicordplugins/zipPreview/index.tsx | Previews ZIP contents inside file attachments. | a ZIP listing in file attachments. |  |

## Settings of the ported plugins

Every setting of each original (`definePluginSettings`), its type and default there, and what it does here.

| plugin | setting | type | default (original) | here |
| - | - | - | - | - |
| AlwaysTrust | `domain` | BOOLEAN | `true` | ported |
| AlwaysTrust | `file` | BOOLEAN | `true` | nothing to act on yet: the app does not show the pinned client's "Potentially Dangerous Download" dialog yet (module 432518, opened from module 889558), so there is no dialog to remove |
| AlwaysTrust | `noDeleteSafety` | BOOLEAN | `true` | ported |
| AlwaysTrust | `confirmModal` | BOOLEAN | `true` | ported |
| AnonymiseFileNames | `anonymiseByDefault` | BOOLEAN | `true` | ported |
| AnonymiseFileNames | `spoilerMessages` | BOOLEAN | `false` | ported |
| AnonymiseFileNames | `method` | SELECT | `Methods.Random` | ported |
| AnonymiseFileNames | `randomisedLength` | NUMBER | `7` | ported |
| AnonymiseFileNames | `consistent` | STRING | `"image"` | ported |
| AnonymiseFileNames | `dateFormat` | STRING | `"YYYY-MM-DD_HH-mm-ss-SSS"` | ported |
| CallTimer | `format` | SELECT | `"stopwatch"` | ported |
| CallTimer | `allCallTimers` | BOOLEAN | `false` | ported |
| CallTimer | `showWithoutHover` | BOOLEAN | `false` | ported |
| CallTimer | `showRoleColor` | BOOLEAN | `false` | ported |
| CallTimer | `trackSelf` | BOOLEAN | `false` | ported |
| CallTimer | `showSeconds` | BOOLEAN | `false` | ported |
| CallTimer | `watchLargeGuilds` | BOOLEAN | `false` | ported |
| MemberCount | `toolTip` | BOOLEAN | `true` | ported |
| MemberCount | `memberList` | BOOLEAN | `true` | ported |
| MemberCount | `voiceActivity` | BOOLEAN | `true` | ported |
| MessageClickActions | `singleClickAction` | SELECT | `"DELETE"` | ported |
| MessageClickActions | `singleClickModifier` | SELECT | `"BACKSPACE"` | ported |
| MessageClickActions | `singleClickOthersAction` | SELECT | `"DELETE"` | ported |
| MessageClickActions | `singleClickOthersModifier` | SELECT | `"BACKSPACE"` | ported |
| MessageClickActions | `doubleClickAction` | SELECT | `"EDIT"` | ported |
| MessageClickActions | `doubleClickOthersAction` | SELECT | `"REPLY"` | ported |
| MessageClickActions | `doubleClickModifier` | SELECT | `"NONE"` | ported |
| MessageClickActions | `tripleClickAction` | SELECT | `"REACT"` | ported |
| MessageClickActions | `tripleClickModifier` | SELECT | `"NONE"` | ported |
| MessageClickActions | `reactEmoji` | COMPONENT | `"💀"` | ported |
| MessageClickActions | `addAdditionalReacts` | BOOLEAN | `false` | ported |
| MessageClickActions | `additionalReactEmojis` | COMPONENT | `""` | ported |
| MessageClickActions | `disableInDms` | BOOLEAN | `false` | ported |
| MessageClickActions | `disableInSystemDms` | BOOLEAN | `true` | ported |
| MessageClickActions | `clickTimeout` | NUMBER | `300` | ported |
| MessageClickActions | `doubleClickHoldThreshold` | NUMBER | `150` | ported |
| MessageClickActions | `deferDoubleClickForTriple` | BOOLEAN | `false` | ported |
| MessageClickActions | `selectionHoldTimeout` | NUMBER | `300` | ported |
| MessageClickActions | `quoteWithReply` | BOOLEAN | `true` | ported |
| MessageClickActions | `useSelectionForQuote` | BOOLEAN | `false` | ported |
| NoPendingCount | `hideFriendRequestsCount` | BOOLEAN | `true` | ported |
| NoPendingCount | `hideMessageRequestsCount` | BOOLEAN | `true` | ported |
| NoPendingCount | `hidePremiumOffersCount` | BOOLEAN | `true` | nothing to act on yet: the app's Home badge does not count unseen Nitro offers yet (`getUnacknowledgedOffers`), so there is no count to hide |
| OnePingPerDM | `channelToAffect` | SELECT | `"both_dms"` | ported |
| OnePingPerDM | `allowMentions` | BOOLEAN | `false` | ported |
| OnePingPerDM | `allowEveryone` | BOOLEAN | `false` | ported |
| OnePingPerDM | `ignoreUsers` | STRING | `""` | ported |
| OnePingPerDM | `alwaysPlaySound` | BOOLEAN | `false` | ported |
| UnlimitedAccounts | `maxAccounts` | NUMBER | `0` | ported |
| VolumeBooster | `multiplier` | SLIDER | `2` | ported |
