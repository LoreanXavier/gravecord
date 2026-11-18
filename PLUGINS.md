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

Totals: 41 ported, 6 native, 64 planned, 45 not applicable.

Status: **ported** works and has its proof; **native** is what gravecord already does without a plugin;
**planned** is not built yet (an import keeps its settings for when it is); **not applicable** has nothing to
port in a native client, or was removed upstream.

| plugin | status | source | what it does (original) | here / what it needs | proof |
| - | - | - | - | - | - |
| AccountPanelServerProfile | ported | plugins/accountPanelServerProfile/index.tsx | Right click your account panel in the bottom left to view your profile in the current server | plugin_hooks/account_panel.rs: a right click on the account panel opens View Server Profile (your profile popout in the open server, disabled outside one) and the Prioritize Server Profile check; checked, a left click shows the server profile and the row reads View Account Profile. | `the_menu_names_the_other_popout_and_shows_the_check`; frames apsp-menu/apsp-profile; plugins-off account-panel |
| AltKrispSwitch | planned | equicordplugins/altKrispSwitch/index.tsx | Makes the Noise Suppression Popout switch between None and Krisp instead of Krisp and Standard | Krisp, which is blocked here (its signature check); without it there is nothing to switch to. |  |
| AlwaysAnimate | ported | plugins/alwaysAnimate/index.ts | Animates anything that can be animated | plugin_hooks/always_animate.rs: icons plays animated avatars (message rows, member list, panel) and server icons on the rail without the pointer and, for server icons, without focus, as canAnimate:!0 does; nameplates play in the member list all the time; roleGradients slides every gradient name, and with the plugin alone (its two patches without a setting) gradient names in chat and the member list slide. Reduced motion still stops gradients. | `each_switch_plays_its_part_and_two_patches_need_none`; frames aa-on/aa-off (avatar report); plugins-off chat, member-list |
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
| BetterGifAltText | planned | plugins/betterGifAltText/index.ts | Change GIF alt text from simply being 'GIF' to containing the gif tags / filename | a place where alt text shows: the original changes the `alt` that screen readers read, and the app draws images without an accessibility tree. |  |
| BetterNotesBox | not applicable | (not in the 2026-11-14 checkout) |  | removed from Equicord (last seen in dbca8ef2); the private note box is the client's own here. |  |
| BetterPlusReacts | planned | equicordplugins/betterPlusReacts/index.ts | The amount of plus before :emoji: is the message to add it to | "+:emoji:" reacting to the Nth message above, in the composer's send path. |  |
| BetterQuickReact | planned | plugins/moreQuickReactions/index.ts | Improves the quick react buttons in the message context menu. | now MoreQuickReactions upstream. more quick reaction buttons in the message context menu. |  |
| BetterRoleContext | planned | plugins/betterRoleContext/index.tsx | Adds options to copy role color / edit role / view role icon when right clicking roles in the user profile or in the member list | role context menu rows (copy colour, edit role, view icon). |  |
| BetterRoleDot | ported | plugins/betterRoleDot/index.ts | Copy role colour on RoleDot (accessibility setting) click. Also allows using both RoleDot and coloured names simultaneously | plugin_hooks/better_role_dot.rs: a click on the role dot (chat, member list, and the popout's dots) copies #rrggbb with the success toast; bothStyles makes every In names / Next to names test true (names, role mentions and dots together); copyRoleColorInProfilePopout draws the popout's roles with dots. | `colours_copy_as_color_strings_and_switches_read`; frame brd-both; plugins-off chat, member-list, profile-popout |
| BetterSettings | not applicable | plugins/betterSettings/index.tsx | Enhances your settings-menu-opening experience | settings are native here: no crossfade or lazy loading to remove. |  |
| BetterUploadButton | ported | plugins/betterUploadButton/index.ts | Upload with a single click, open menu with right click | ui.rs: the composer plus opens the file picker on a click and its menu on a right click. | `copy_user_urls_and_quick_mention_write_what_the_originals_write` (off); frame bub-on; plugins-off plus-click |
| BiggerStreamPreview | planned | plugins/biggerStreamPreview/index.tsx | This plugin allows you to enlarge stream previews | a larger stream preview popout. |  |
| BypassStatus | planned | equicordplugins/bypassStatus/index.tsx | Still get notifications from specific sources when in do not disturb mode. Right-click on users/channels/guilds to set them to bypass do not disturb mode. | notification and sound rules that pass Do Not Disturb for chosen users, channels and servers. |  |
| CallTimer | ported | plugins/callTimer/index.tsx | Add call timers for all users in voice channels and in the connection status. | plugin_hooks/call_timer.rs: your time under the connection panel's channel line (seconds always, code font); with `allCallTimers` everyone's time by their voice row, as the clock icon with a tooltip or, with `showWithoutHover`, as text under the name. Join times start when the app first sees someone in a channel. | `plugin_hooks::call_timer::tests` (3); frames ct-on/ct-all; plugins-off voice-profile |
| CancelFriendRequest | ported | equicordplugins/pendingFriendRequest/index.tsx | Adds a way to cancel outgoing friend requests from profiles. | profile_menu.rs, profile_other.rs, dm.rs: for an outgoing request the profile's friend button turns red and cancels it, the DM's text button reads 'Cancel Outgoing Friend Request' (critical), and the profile More menu ends with that row; each removes the relationship as the friends list's Cancel does. | frames cfr-on/cfr-off (an outgoing request set locally); plugins-off voice-profile, profile-popout |
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
| DownloadAllAttachments | ported | equicordplugins/downloadAllAttachments/index.tsx | Adds a popover button to download all attachments in a message at once. | plugin_hooks/download_all.rs: a hover bar button on messages with attachments (before QuickMention's, in plugin order); over 1 GiB a critical toast, else every attachment from its proxy URL (and with downloadAllFileTypes its URL after it), four at a time, saved into Downloads as the one file or a stored ZIP attachments-<id>.zip with unique names, then the original's success or failure toast. The app has no save dialog, so the file goes where its other downloads go. | `names_are_made_unique_as_the_original_does`, `the_archive_holds_each_file_stored_with_its_crc`, `sources_size_check_and_toasts_follow_the_original`, `download_all_saves_what_answered_as_a_zip_or_one_file` (local server); frame daa-on; plugins-off attachment-hover, hover-bar |
| DynamicImageModalAPI | not applicable | plugins/_api/dynamicImageModalApi.ts | Allows you to omit either width or height when opening an image modal | client-mod plumbing (an API for other plugins), nothing to port. |  |
| EquicordHelper | not applicable | equicordplugins/equicordHelper/index.tsx | Used to provide support and fix discord caused crashes. | workarounds for Discord web crashes in the mod. |  |
| Experiments | planned | plugins/experiments/index.tsx | Enable Access to Experiments & other dev-only features in Discord! | the experiments and developer pages. |  |
| ExpressionCloner | planned | plugins/expressionManager/index.tsx | Clone Emotes & Stickers. Adds context menu options to quickly clone, edit, or delete them. Also adds Copy & Save options for stickers | now ExpressionManager upstream. menu rows that clone, edit and delete emoji and stickers into your servers. |  |
| ExtraContextMenusAPI | not applicable | (not in the 2026-11-14 checkout) |  | client-mod plumbing (an API for other plugins), nothing to port. |  |
| FavoriteEmojiFirst | ported | plugins/favEmojiFirst/index.tsx | Puts your favorite emoji first in the emoji autocomplete and also has emoji alias. | autocomplete.rs + plugin_hooks.rs favorite_emoji_first: the emoji autocomplete sorts every match, puts your favourite emoji (the picker's Favorites) first and cuts the list where it was. Not 1:1 yet: the alias half (see its settings). | frames fef-on/fef-off/fef-top (:smi with smiling_imp a favourite, demo-favorite); plugins-off emoji-autocomplete |
| FavoriteGifSearch | not applicable | (not in the 2026-11-14 checkout) |  | removed from Equicord in 0001bab8: "now a default Discord feature". |  |
| FixImagesQuality | ported | plugins/fixImagesQuality/index.tsx | Improves quality of images by loading them at their original resolution | media_image.rs, lightbox.rs: attachment pictures without the proxy's size in chat (scaled only past 2000 x 1200), the original file in the viewer (and in chat with originalImagesInChat). | `fix_images_quality_loads_full_resolution`; plugins-off image-menu (an attachment picture) |
| FixYoutubeEmbeds | planned | plugins/fixYoutubeEmbeds.desktop/index.tsx | Bypasses youtube videos being blocked from display on Discord (for example by UMG) | the YouTube embed workaround inside the web host. |  |
| ForceOwnerCrown | native | plugins/forceOwnerCrown/index.ts | Force the owner crown next to usernames even if the server is large. | the member list shows the owner crown at any server size. |  |
| FriendInvites | planned | plugins/friendInvites/index.ts | Create and manage friend invite links via slash commands (/create friend invite, /view friend invites, /revoke friend invites). | slash commands for friend invite links. |  |
| FriendsSince | not applicable | (not in the 2026-11-14 checkout) |  | removed from Equicord in 36c7a837: "now a stock Discord feature"; gravecord shows Friends Since in profiles. |  |
| FullSearchContext | planned | plugins/fullSearchContext/index.tsx | Makes the message context menu in message search results have all options you'd expect | the full message menu on search results. |  |
| FullVCPFP | ported | equicordplugins/fullVcPfp/index.tsx | Makes avatars take up the entire vc tile | plugin_hooks/full_vc_pfp.rs: a call tile without video draws the avatar over the whole tile (contain, unrounded, the 80px circle gone) at 1024px, the server avatar first. Not 1:1: an animated avatar stays still while its owner speaks (the original asks for the animated file then). | `the_server_avatar_comes_first_at_1024`; frame fvp-on (callsim); plugins-off voice-profile, user-volume |
| GameActivityToggle | ported | plugins/gameActivityToggle/index.tsx | Adds a button next to the mic and deafen button to toggle game activity. Right click it to toggle Spotify activity. | plugin_hooks/game_activity_toggle.rs: a panel button before the microphone that turns Share my activity on and off (the account setting, so the presence changes at once), red with the struck-through gamepad while off; oldIcon draws the old line. Not 1:1: the right-click Share Spotify Activity menu is missing, because the app has no Spotify connection show_activity request yet. | `the_button_reads_its_settings_and_names_the_next_state`; frames gat-on/gat-off/gat-old; plugins-off chat (user panel) |
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
| MessagePeek | ported | equicordplugins/messagePeek/index.tsx | Shows the last message preview and timestamp in the Direct Messages list. | plugin_hooks/message_peek.rs: under each DM and group the last message the app holds (author: first line, GIF, voice message, attachments, sticker), its age at the right (hidden under the pointer); an activity instead when the message is over an hour old; hideMuted. Not ported: the startup fetch of the 25 newest DMs' last message (plugin code sends nothing), the inline markdown of the preview (plain first line) and BetterActivities' icons. | `plugin_hooks::message_peek::tests` (2); frame mp-on; plugins-off dm-list |
| MessagePopoverAPI | not applicable | plugins/_api/messagePopover.ts | API to add buttons to message popovers. | client-mod plumbing (an API for other plugins), nothing to port. |  |
| MessageTags | planned | plugins/customCommands/index.ts | Allows you to create custom slash commands / tags | now CustomCommands upstream. saved tags sent as custom slash commands. |  |
| MessageUpdaterAPI | not applicable | plugins/_api/messageUpdater.ts | API for updating and re-rendering messages. | client-mod plumbing (an API for other plugins), nothing to port. |  |
| MoreCommands | planned | equicordplugins/moreCommands/index.ts | Adds various fun and useful commands | its slash commands. |  |
| MoreKaomoji | not applicable | (not in the 2026-11-14 checkout) |  | removed from Equicord in d7e6fcd3. |  |
| MuteChannelMentions | ported | userplugins/muteChannelMentions/index.tsx | Hides mention badges from the channels you pick, both on the channel and on the server icon. Nothing is sent to Discord. | plugin_hooks/mute_channel_mentions.rs + dc-state ReadStates::hidden: the picked channels' mention badges are filtered wherever the app reads a channel's mentions (channel rows, server icons, overlay, keyboard), none at all or with onlyEveryone only your direct mentions since the last read; channel and thread menus get the Hide mention badge check after Mark As Read. Nothing is sent. | `ids_parse_as_the_original_splits_them`, dc-state `hidden_channels_show_nothing_or_only_direct_mentions_since_the_ack`; frame mcm-menu; plugins-off channel-menu, chat |
| NewGuildSettings | planned | plugins/newGuildSettings/index.tsx | Automatically mute new servers and change various other settings upon joining | notification defaults applied when joining a server. |  |
| NicknameIconsAPI | not applicable | plugins/_api/nicknameIcons.ts | API to add icons to the nickname, in profiles | client-mod plumbing (an API for other plugins), nothing to port. |  |
| NoDevtoolsWarning | not applicable | plugins/noDevtoolsWarning/index.ts | Disables the 'HOLD UP' banner in the console. As a side effect, also prevents Discord from hiding your token, which prevents random logouts. | no web console in a native client. |  |
| NoNitroUpsell | ported | equicordplugins/noNitroUpsell/index.ts | Removes all of Discord's nitro upsells by tricking the client into thinking you have nitro. | plugin_hooks.rs premium_type_override, nitro.rs premium_wire: while on, the account's tier reads as Nitro (premiumTypeOverride 2) everywhere the app asks for it, so the upsells that depend on it are left out, as the original tricks the client; the server still applies the real tier. | `no_nitro_upsell_reads_as_nitro_only_while_on`; frames nnu-on/nnu-off (Appearance: Color Themes as Nitro); plugins-off gif-picker, settings-appearance |
| NoPendingCount | ported | plugins/noPendingCount/index.ts | Removes the ping count of incoming friend requests, message requests, and nitro offers. | unread_badges.rs and home.rs: friend request count 0, no Message Requests badge (the row stays). | `no_pending_count_hides_each_badge_by_its_setting`; frames pending-off/on; plugins-off requests |
| NoPushToTalk | native | equicordplugins/noPushToTalk/index.ts | Bypasses the push-to-talk requirement for voice activity in channels that enforce it. | the app does not enforce Use Voice Activity yet (the pinned client's PermissionVADStore and its warning), so voice activity already works where a server requires push to talk; when the app adds the rule, this plugin needs its hook there. |  |
| NormalizeMessageLinks | native | equicordplugins/normalizeMessageLinks/index.ts | Strip canary/ptb from message links | Copy Message Link already writes discord.com links. |  |
| NoticesAPI | not applicable | plugins/_api/notices.ts | Fixes notices being automatically dismissed | client-mod plumbing (an API for other plugins), nothing to port. |  |
| NoTrack | native | plugins/_core/noTrack.ts | Disable Discord's tracking (analytics/'science'), metrics and Sentry crash reporting | gravecord sends no analytics, metrics or Sentry reports; its only `/science` post is the overlay problem report the user submits. |  |
| NoTypingAnimation | ported | plugins/noTypingAnimation/index.ts | Disables the CPU-intensive typing dots animation | typing_avatar.rs: every typing indicator draws still dots and asks for no animation frames. | `switch_only_plugins_follow_their_switch_and_the_master`; plugins-off typing |
| NoUnblockToJump | native | plugins/noUnblockToJump/index.ts | Allows you to jump to messages of blocked or ignored users and likely spammers without unblocking them | the app has no 'unblock to jump' dialog yet, so jumping to a blocked or ignored user's message already works; when the app adds the dialog, this plugin needs its hook there. |  |
| NSFWGateBypass | not applicable | (not in the 2026-11-14 checkout) |  | removed from Equicord in 9a3c66ab. |  |
| OnePingPerDM | ported | plugins/onePingPerDM/index.ts | If unread messages are sent by a user in DMs multiple times, you'll only receive one audio ping. Read the messages to reset the limit | notifications.rs: in the DMs it affects only the oldest unread message (the first after the channel's ack) plays the sound; ignored users, allowed mentions and @everyone still ping; the desktop notification is left alone; alwaysPlaySound plays the message sound for any message that notifies. | `one_ping_per_dm_lets_the_oldest_unread_ping`; no frame (a sound, not a picture) |
| OpenInApp | planned | plugins/openInApp/index.ts | Open links in their respective apps instead of your browser | Spotify, Steam and other links opened in their apps. |  |
| PermissionFreeWill | planned | plugins/permissionFreeWill/index.ts | Disables the client-side restrictions for channel permission management. | no client-side limits in channel permission editing. |  |
| PermissionsViewer | planned | plugins/permissionsViewer/index.tsx | View the permissions a user or channel has, and the roles of a server | a permission breakdown in profiles and menus. |  |
| PinDMs | planned | plugins/pinDms/index.tsx | Allows you to pin private channels to the top of your DM list. To pin/unpin or re-order pins, right click DMs | pinned DM categories in the DM list. |  |
| PlatformIndicators | ported | plugins/platformIndicators/index.tsx | Adds platform indicators (Desktop, Mobile, Web...) to users | plugin_hooks/platform_indicators.rs: an icon per client and its status colour (with tooltips) after names in the member list, in message headers, and before the pronouns in the DM sidebar profile; the phone for anyone on mobile with colorMobileIndicator. Your own clients are the presence the server sent, else this desktop (the app keeps no session client list). Not yet in the popout and full profile (only the DM sidebar has the pronoun line the original patches the same way). | `plugin_hooks::platform_indicators::tests` (2); frames pi-on/pi-off; plugins-off member-list, chat, dm-list |
| PreviewMessage | planned | plugins/previewMessage/index.tsx | Lets you preview your message before sending it. | a preview of the message before sending. |  |
| ProfileSectionsAPI | not applicable | equicordplugins/_api/profileSections.ts | API to add sections near the 'Member Since' area of user profile panels. | client-mod plumbing (an API for other plugins), nothing to port. |  |
| QuickMention | ported | plugins/quickMention/index.tsx | Adds a quick mention button to the message actions bar | hover_bar.rs: an @ button after the quick reactions and before Add Reaction, where you may write; inserts `<@ID> ` in the composer (shown as text: the composer draws no mention pill for typed syntax). | `quick_mention_sits_before_add_reaction`; frames qm-on, qm-click; plugins-off hover-bar |
| Quoter | planned | equicordplugins/quoter/index.tsx | Adds the ability to create an inspirational quote image from a message. | a quote image made from a message. |  |
| ReactErrorDecoder | not applicable | plugins/reactErrorDecoder/index.ts |  | decodes minified React errors; there is no React here. |  |
| ReadAllNotificationsButton | ported | plugins/readAllNotificationsButton/index.tsx | Read all server notifications with a single button click! | plugin_hooks/read_all.rs: a Read All tile (or compact text) under the Home button; one BULK_ACK of every server's unread channels and joined threads (the app's own bulk ack). | `the_button_is_a_tile_or_compact_text`; frame ranb-on; plugins-off chat (rail) |
| RelationshipNotifier | planned | plugins/relationshipNotifier/index.ts | Notifies you when a friend, group chat, or server removes you. | notices when a friend, group or server removes you (its data store lists are not copied). |  |
| RevealAllSpoilers | ported | plugins/revealAllSpoilers/index.ts | Reveal all spoilers in a message by Ctrl-clicking a spoiler, or in the chat with Ctrl+Shift-click | ui.rs: Ctrl-click a hidden spoiler opens every text spoiler of its message, Ctrl+Shift-click those of every message the chat holds now (later ones stay hidden). | `ctrl_reveals_the_message_and_ctrl_shift_every_loaded_message`; frames spoilers-off/on |
| ReverseImageSearch | ported | plugins/reverseImageSearch/index.tsx | Adds ImageSearch to image context menus | menu.rs: "Search Image" at the end of the link group when a picture is right-clicked; a submenu of its seven engines and All, opened in the browser. The engine favicons (fetched from DuckDuckGo by the original) are not drawn: plugin code fetches nothing. | `image_search_opens_each_engine_with_the_encoded_address`; frame ris-on; plugins-off image-menu |
| RoleColorEverywhere | planned | plugins/roleColorEverywhere/index.tsx | Adds the top role color anywhere possible | role colours in mentions, lists, voice and reactions. |  |
| SearchFix | planned | equicordplugins/searchFix/index.tsx |  | its search request fix. |  |
| SedEnhanced | planned | equicordplugins/sedEnhanced/index.ts | Expands on Discord's rudimentary `sed` support. | `s/a/b/` editing the last message, in the send path. |  |
| ServerInfo | planned | plugins/serverInfo/index.tsx | Allows you to view info about a server | a server information dialog. |  |
| ServerListAPI | not applicable | plugins/_api/serverList.ts | API required for plugins that modify the server list. | client-mod plumbing (an API for other plugins), nothing to port. |  |
| ServerListIndicators | ported | plugins/serverListIndicators/index.tsx | Add online friend count or server count in the server list | plugin_hooks/server_list_indicators.rs: under the Home button an item with the friends online and/or the servers (mode), and the same as its tooltip. Pending join requests of servers not joined are not counted yet (the app keeps no UserGuildJoinRequestStore id list). | `the_item_says_what_its_mode_counts`; frame sli-on; plugins-off chat (rail) |
| Settings | not applicable | plugins/_core/settings.tsx | Adds Settings UI and debug info | its settings UI is the Gravecord settings group itself; `settingsLocation` not ported (the group sits before Developer). |  |
| ShowConnections | ported | plugins/showConnections/index.tsx | Show connected accounts in user popouts and the DM sidebar | plugin_hooks/show_connections.rs: the connected accounts as platform icons (size, spacing) with their names as tooltips in the profile popout and the DM sidebar, and with showOnModal in place of the full profile's connection cards; a click opens the page or copies the name. | `spacing_and_settings_read_as_the_original`; frame sc-on; plugins-off profile-popout, voice-profile |
| ShowHiddenThings | planned | plugins/showHiddenThings/index.ts | Displays various hidden & moderator-only things regardless of permissions. | hidden and moderator-only things shown. |  |
| SilentMessageToggle | planned | plugins/silentMessageToggle/index.tsx | Adds a button to the chat bar to toggle sending a silent message. | the pinned client's `@silent` prefix first: its send path strips it and sets SUPPRESS_NOTIFICATIONS (the `silentPrefix` rule and the sends of modules 857069 and 92961); the app sends the text as typed, so the plugin's prefix would reach people as words. |  |
| SpotifyCrack | planned | plugins/spotifyCrack/index.ts | Free listen along, no auto-pausing in voice chat, and allows activity to continue playing when idling | listen along without Premium and no Spotify pause on voice. |  |
| SpotifyShareCommands | planned | plugins/spotifyShareCommands/index.ts | Share your current Spotify track, album or artist via slash command (/track, /album, /artist) | its /track, /album, /artist commands. |  |
| SteamStatusSync | planned | equicordplugins/steamStatusSync/index.tsx | Sync your status to Steam! (Online, Away, Invisible, or Offline.) | status sync with the Steam client. |  |
| StopAutoUnread | native | equicordplugins/stopAutoUnread/index.ts |  | the app never raises a channel to All Messages by itself: the pinned client's `maybeAutoUpgradeChannel` (called by the unreads notice hook in module 626977) is not ported (GAP: main app), so there is nothing to stop. |  |
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
| TypingIndicator | ported | plugins/typingIndicator/index.tsx | Adds an indicator if someone is typing on a channel. | plugin_hooks/typing_indicator.rs: by a channel's name, up to three 16px avatars and the dots of who is typing, the chat's typing line as the tooltip. Not ported: thread rows (the original also patches the thread spine), and TypingTweaks' several-users line (TypingTweaks is not built yet). | `the_indicator_leaves_out_who_its_settings_leave_out`; frames ti-on/ti-off; plugins-off channel-typing |
| TypingTweaks | ported | plugins/typingTweaks/index.tsx | Show avatars and role colours in the typing indicator | plugin_hooks/typing_tweaks.rs: in the typing line every bold name is the person, with a 16px avatar (showAvatars), the role colour (showRoleColors) and a click to the full profile; over three people the alternative line (alternativeFormatting, with the original's count); amITyping keeps you in the list; blocked and ignored people are left out as useTypingUsers does. | `bold_runs_become_people_and_many_use_the_alternative_line`; frames tt-on/tt-off; plugins-off typing |
| UnlimitedAccounts | ported | equicordplugins/unlimitedAccounts/index.ts | Increases the amount of accounts you can add. | account_switcher.rs: `maxAccounts` (0 = no limit) for the list and the Add button. | `unlimited_accounts_lifts_the_five_account_limit`; frames ua-off/ua-on; plugins-off accounts |
| UnlockedAvatarZoom | planned | plugins/unlockedAvatarZoom/index.ts | Allows you to zoom in further in the image crop tool when changing your avatar | the avatar crop tool (Edit Image of a new avatar), which the app does not have yet; the plugin only raises its zoom slider's maximum. |  |
| UserAreaAPI | not applicable | equicordplugins/_api/userArea.ts | API to add buttons to the user area panel. | client-mod plumbing (an API for other plugins), nothing to port. |  |
| UserMessagesPronouns | ported | plugins/userMessagesPronouns/index.ts | Adds pronouns to chat user messages | plugin_hooks/message_pronouns.rs: after a message's time, '• pronouns' in the time's style with a Pronouns tooltip, server profile pronouns first, from profiles already loaded (nothing fetched), lowercased unless Capitalized; not for bots, system or AutoMod messages, nor yourself without showSelf. Not 1:1: its compact-mode place is not drawn, the app has no compact message display. | `server_pronouns_first_then_the_accounts_formatted`; frame ump-on; plugins-off chat |
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
| WhoReacted | ported | plugins/whoReacted/index.tsx | Renders the avatars of users who reacted to a message | plugin_hooks/who_reacted.rs: after each reaction's count up to five avatars at 90% (four and +N past five), from GET reactions?limit=100&type=0 queued one per 250 ms newest first and merged into the users; nothing on a message with over ten reactions; clickableAvatars opens the profile popout instead of reacting. Super reactions (type 1) are not fetched: the app draws no burst pills. | `five_at_most_with_the_rest_counted`; frame wr-on (demo-reactors); plugins-off reactions, chat |
| YoutubeAdblock | not applicable | plugins/youtubeAdblock.desktop/index.ts | Block ads in YouTube embeds and the WatchTogether activity via AdGuard | needs an ad filter inside the YouTube web view of embeds; deferred with the web host. |  |
| ZipPreview | planned | equicordplugins/zipPreview/index.tsx | Previews ZIP contents inside file attachments. | a ZIP listing in file attachments. |  |

## Settings of the ported plugins

Every setting of each original (`definePluginSettings`), its type and default there, and what it does here.

| plugin | setting | type | default (original) | here |
| - | - | - | - | - |
| AccountPanelServerProfile | `prioritizeServerProfile` | BOOLEAN | `false` | ported |
| AlwaysAnimate | `icons` | BOOLEAN | `true` | ported |
| AlwaysAnimate | `statusEmojis` | BOOLEAN | `true` | nothing to act on yet: the app draws every custom status emoji as a still picture (the pinned client's member list plays an animated one), so there is nothing playing to force |
| AlwaysAnimate | `serverBanners` | BOOLEAN | `true` | nothing to act on yet: the channel list header does not draw the server's banner yet, so there is no banner to play |
| AlwaysAnimate | `nameplates` | BOOLEAN | `true` | ported |
| AlwaysAnimate | `roleGradients` | BOOLEAN | `true` | ported |
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
| BetterRoleDot | `bothStyles` | BOOLEAN | `false` | ported |
| BetterRoleDot | `copyRoleColorInProfilePopout` | BOOLEAN | `false` | ported |
| CallTimer | `format` | SELECT | `"stopwatch"` | ported |
| CallTimer | `allCallTimers` | BOOLEAN | `false` | ported |
| CallTimer | `showWithoutHover` | BOOLEAN | `false` | ported |
| CallTimer | `showRoleColor` | BOOLEAN | `false` | ported |
| CallTimer | `trackSelf` | BOOLEAN | `false` | ported |
| CallTimer | `showSeconds` | BOOLEAN | `false` | ported |
| CallTimer | `watchLargeGuilds` | BOOLEAN | `false` | nothing to act on yet: the original subscribes to every server's events for it (`GUILD_SUBSCRIPTIONS_FLUSH`); plugin code sends nothing, so times come from the voice states the app already receives |
| DownloadAllAttachments | `downloadAllFileTypes` | BOOLEAN | `false` | ported |
| FavoriteEmojiFirst | `aliases` | COMPONENT | `` | not ported: not ported yet: the emoji alias editor (Set Alias in the emoji menus, this list, `:alias:` written out on send and offered in the autocomplete) is the next step of this port. |
| FavoriteEmojiFirst | `clearAll` | COMPONENT | `` | not ported: not ported yet: comes with the alias editor (see aliases). |
| FixImagesQuality | `originalImagesInChat` | BOOLEAN | `false` | ported |
| GameActivityToggle | `oldIcon` | BOOLEAN | `false` | ported |
| GameActivityToggle | `location` | SELECT | `"PANEL"` | nothing to act on yet: the original only reads it while EquicordToolbox is on, which is not ported; the button stays next to Mute/Deafen as with the default |
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
| MessagePeek | `hideMuted` | BOOLEAN | `false` | ported |
| MuteChannelMentions | `channelIds` | STRING | `""` | ported |
| MuteChannelMentions | `onlyEveryone` | BOOLEAN | `true` | ported |
| NoPendingCount | `hideFriendRequestsCount` | BOOLEAN | `true` | ported |
| NoPendingCount | `hideMessageRequestsCount` | BOOLEAN | `true` | ported |
| NoPendingCount | `hidePremiumOffersCount` | BOOLEAN | `true` | nothing to act on yet: the app's Home badge does not count unseen Nitro offers yet (`getUnacknowledgedOffers`), so there is no count to hide |
| OnePingPerDM | `channelToAffect` | SELECT | `"both_dms"` | ported |
| OnePingPerDM | `allowMentions` | BOOLEAN | `false` | ported |
| OnePingPerDM | `allowEveryone` | BOOLEAN | `false` | ported |
| OnePingPerDM | `ignoreUsers` | STRING | `""` | ported |
| OnePingPerDM | `alwaysPlaySound` | BOOLEAN | `false` | ported |
| PlatformIndicators | `list` | BOOLEAN | `true` | ported |
| PlatformIndicators | `profiles` | BOOLEAN | `true` | ported |
| PlatformIndicators | `messages` | BOOLEAN | `true` | ported |
| PlatformIndicators | `colorMobileIndicator` | BOOLEAN | `true` | ported |
| PlatformIndicators | `showBots` | BOOLEAN | `false` | ported |
| PlatformIndicators | `ConsoleIcon` | SELECT | `"equicord"` | ported |
| ReadAllNotificationsButton | `compact` | BOOLEAN | `false` | ported |
| ServerListIndicators | `mode` | SELECT | `IndicatorType.FRIEND` | ported |
| ShowConnections | `iconSize` | NUMBER | `32` | ported |
| ShowConnections | `iconSpacing` | SELECT | `Spacing.COZY` | ported |
| ShowConnections | `showGithubOrgs` | BOOLEAN | `true` | nothing to act on yet: the original asks api.github.com for the organisations of a GitHub account; plugin code sends nothing, so only the accounts Discord lists show |
| ShowConnections | `showOnModal` | BOOLEAN | `true` | ported |
| TypingIndicator | `includeCurrentChannel` | BOOLEAN | `true` | ported |
| TypingIndicator | `includeMutedChannels` | BOOLEAN | `false` | ported |
| TypingIndicator | `includeIgnoredUsers` | BOOLEAN | `false` | ported |
| TypingIndicator | `includeBlockedUsers` | BOOLEAN | `false` | ported |
| TypingIndicator | `indicatorMode` | SELECT | `IndicatorMode.Dots | IndicatorMode.Avatars` | ported |
| TypingTweaks | `showAvatars` | BOOLEAN | `true` | ported |
| TypingTweaks | `showRoleColors` | BOOLEAN | `true` | ported |
| TypingTweaks | `alternativeFormatting` | BOOLEAN | `true` | ported |
| TypingTweaks | `amITyping` | BOOLEAN | `false` | ported |
| UnlimitedAccounts | `maxAccounts` | NUMBER | `0` | ported |
| UserMessagesPronouns | `pronounsFormat` | SELECT | `PronounsFormat.Lowercase` | ported |
| UserMessagesPronouns | `showSelf` | BOOLEAN | `true` | ported |
| VolumeBooster | `multiplier` | SLIDER | `2` | ported |
| WhoReacted | `clickableAvatars` | BOOLEAN | `true` | ported |
