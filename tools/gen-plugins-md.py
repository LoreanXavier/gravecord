"""Writes PLUGINS.md: the status of every plugin an Equicord backup enables, and for each ported plugin its settings
compared with the original's, setting by setting.

Usage: python tools/gen-plugins-md.py <equicord-backup.json> <Equicord checkout> PLUGINS.md

Reads the backup only for plugin names and enabled flags (never setting values or the data store), the Equicord
checkout for each plugin's source, description and settings (`definePluginSettings`), and `app/src/plugins/registry.rs`
for the ported settings. It fails (exit 1) when a ported plugin misses an original setting without a reason below,
has a setting the original does not, or when a plugin that is not ported has no reason of its own.
"""
import json, os, re, sys

BACKUP, CHECKOUT, OUT = sys.argv[1:4]
ROOT = os.path.join(CHECKOUT, 'src')
REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

backup = json.load(open(BACKUP, encoding='utf-8'))
enabled = sorted((k for k, v in backup['settings']['plugins'].items() if v.get('enabled')), key=str.lower)

# Plugins renamed upstream since the backup (Equicord's `migratePluginSettings`): the backup's name, the new name.
RENAMED = {
    'BetterQuickReact': 'MoreQuickReactions',
    'ExpressionCloner': 'ExpressionManager',
    'IloveSpam': 'ILoveSpam',
    'LimitMiddleClickPaste': 'MiddleClickTweaks',
    'MessageTags': 'CustomCommands',
    'TenorGifSearch': 'GifProviderSwitcher',
}
# Plugins removed upstream since the backup, with the commit that says why.
REMOVED = {
    'BetterNotesBox': 'removed from Equicord (last seen in dbca8ef2); the private note box is the client\'s own here.',
    'FavoriteGifSearch': 'removed from Equicord in 0001bab8: "now a default Discord feature".',
    'FriendsSince': 'removed from Equicord in 36c7a837: "now a stock Discord feature"; gravecord shows Friends Since '
                    'in profiles.',
    'MoreKaomoji': 'removed from Equicord in d7e6fcd3.',
    'NSFWGateBypass': 'removed from Equicord in 9a3c66ab.',
    'StreamerModeOnStream': 'no longer in the Equicord checkout of 2026-11-14 (added in 9e0aa4b2, since dropped).',
    'Summaries': 'removed from Equicord in 452241cf: "Discord killed this feature".',
    'TimeBarAllActivities': 'removed from Equicord in bcfef05a: "now a stock feature".',
}


def balanced(text, start):
    """The text of the bracketed block that opens at `start` (`{` or `(`), strings and comments skipped."""
    pairs = {'{': '}', '(': ')', '[': ']'}
    stack, i, quote = [], start, None
    while i < len(text):
        c = text[i]
        if quote:
            if c == '\\':
                i += 2
                continue
            if c == quote:
                quote = None
        elif c in '"\'`':
            quote = c
        elif text.startswith('//', i):
            i = text.find('\n', i)
            continue
        elif c in pairs:
            stack.append(pairs[c])
        elif stack and c == stack[-1]:
            stack.pop()
            if not stack:
                return text[start:i + 1]
        i += 1
    return text[start:]


def top_level_entries(block):
    """`key: { ... }` entries at the top of an object literal."""
    inner, depth, i, out = block[1:-1], 0, 0, []
    while i < len(inner):
        c = inner[i]
        if c in '{([':
            if depth == 0 and c == '{':
                m = re.search(r'(\w+)\s*:\s*$', inner[:i])
                if m:
                    body = balanced(inner, i)
                    out.append((m.group(1), body))
                    i += len(body)
                    continue
            depth += 1
        elif c in '})]':
            depth -= 1
        elif c in '"\'`':
            end = inner.find(c, i + 1)
            i = end if end > 0 else i
        i += 1
    return out


sources = {}
for dp, dn, fn in os.walk(ROOT):
    for f in fn:
        if not f.endswith(('.ts', '.tsx')):
            continue
        path = os.path.join(dp, f)
        text = open(path, encoding='utf-8', errors='replace').read()
        for m in re.finditer(r'definePlugin\(\{', text):
            block = balanced(text, m.end() - 1)
            name = re.search(r'\n\s{4}name:\s*"([^"]+)"', block)
            if not name:
                continue
            rel = os.path.relpath(path, ROOT).replace(os.sep, '/')
            desc = re.search(r'\n\s{4}description:\s*"([^"]*)"', block)
            settings = []
            s = re.search(r'definePluginSettings\(\{', text)
            if s:
                for key, body in top_level_entries(balanced(text, s.end() - 1)):
                    kind = re.search(r'type:\s*OptionType\.(\w+)', body)
                    # A select marks its default option with `default: true`; take that option's value.
                    picked = re.search(r'value:\s*([^,}\n]+?),\s*default:\s*true', body)
                    default = re.search(r'\bdefault:\s*([^,\n]+)', body)
                    default = picked.group(1).strip() if picked else (default.group(1).strip() if default else '')
                    settings.append((key, kind.group(1) if kind else '?', default))
            sources.setdefault(name.group(1), (rel, desc.group(1) if desc else '', settings))

# The ported plugins and their settings, from the registry.
registry = open(os.path.join(REPO, 'app/src/plugins/registry.rs'), encoding='utf-8').read()
ours = {}
for m in re.finditer(r'\n    Def \{\n        id: "([^"]+)"', registry):
    block = balanced(registry, m.start() + len('\n    Def '))
    ours[m.group(1)] = re.findall(r'key: "([^"]+)"', block)
idle_src = registry.split('IDLE_SETTINGS')[1].split('];')[0]
# A reason may go on over lines with Rust's backslash-newline, which also drops the next line's indent.
idle = {(a, b): re.sub(r'\\\n\s*', '', why).replace('\\"', '"') for a, b, why in re.findall(
    r'\(\s*"([^"]+)",\s*"([^"]+)",\s*"((?:[^"\\]|\\.)*)",\s*\)', idle_src, re.S)}
# Every entry must be read, or its reason would drop out of PLUGINS.md without a word.
idle_entries = idle_src.count('\n    (')
assert len(idle) == idle_entries, f'IDLE_SETTINGS: read {len(idle)} of {idle_entries} entries'

# How each ported plugin is built here, and its proof.
PORTED = {
    'AlwaysAnimate': "plugin_hooks/always_animate.rs: icons plays animated avatars (message rows, member list, panel) and server icons on the rail without the pointer and, for server icons, without focus, as canAnimate:!0 does; nameplates play in the member list all the time; roleGradients slides every gradient name, and with the plugin alone (its two patches without a setting) gradient names in chat and the member list slide. Reduced motion still stops gradients.",
    'GameActivityToggle': "plugin_hooks/game_activity_toggle.rs: a panel button before the microphone that turns Share my activity on and off (the account setting, so the presence changes at once), red with the struck-through gamepad while off; oldIcon draws the old line. Not 1:1: the right-click Share Spotify Activity menu is missing, because the app has no Spotify connection show_activity request yet.",
    'CancelFriendRequest': "profile_menu.rs, profile_other.rs, dm.rs: for an outgoing request the profile's friend button turns red and cancels it, the DM's text button reads 'Cancel Outgoing Friend Request' (critical), and the profile More menu ends with that row; each removes the relationship as the friends list's Cancel does.",
    'ServerListIndicators': "plugin_hooks/server_list_indicators.rs: under the Home button an item with the friends online and/or the servers (mode), and the same as its tooltip. Pending join requests of servers not joined are not counted yet (the app keeps no UserGuildJoinRequestStore id list).",
    'ShowConnections': "plugin_hooks/show_connections.rs: the connected accounts as platform icons (size, spacing) with their names as tooltips in the profile popout and the DM sidebar, and with showOnModal in place of the full profile's connection cards; a click opens the page or copies the name.",
    'MessagePeek': "plugin_hooks/message_peek.rs: under each DM and group the last message the app holds (author: first line, GIF, voice message, attachments, sticker), its age at the right (hidden under the pointer); an activity instead when the message is over an hour old; hideMuted. Not ported: the startup fetch of the 25 newest DMs' last message (plugin code sends nothing), the inline markdown of the preview (plain first line) and BetterActivities' icons.",
    'PlatformIndicators': "plugin_hooks/platform_indicators.rs: an icon per client and its status colour (with tooltips) after names in the member list, in message headers, and before the pronouns in the DM sidebar profile; the phone for anyone on mobile with colorMobileIndicator. Your own clients are the presence the server sent, else this desktop (the app keeps no session client list). Not yet in the popout and full profile (only the DM sidebar has the pronoun line the original patches the same way).",
    'TypingIndicator': "plugin_hooks/typing_indicator.rs: by a channel's name, up to three 16px avatars and the dots of who is typing, the chat's typing line as the tooltip. Not ported: thread rows (the original also patches the thread spine), and TypingTweaks' several-users line (TypingTweaks is not built yet).",
    'ReadAllNotificationsButton': "plugin_hooks/read_all.rs: a Read All tile (or compact text) under the Home button; one BULK_ACK of every server's unread channels and joined threads (the app's own bulk ack).",
    'FixImagesQuality': "media_image.rs, lightbox.rs: attachment pictures without the proxy's size in chat (scaled only past 2000 x 1200), the original file in the viewer (and in chat with originalImagesInChat).",
    'OnePingPerDM': "notifications.rs: in the DMs it affects only the oldest unread message (the first after the channel's ack) plays the sound; ignored users, allowed mentions and @everyone still ping; the desktop notification is left alone; alwaysPlaySound plays the message sound for any message that notifies.",
    'VoiceDownload': "voice_message.rs: a download arrow after a voice message's volume that opens the file in the browser (the original's desktop behaviour).",
    'CallTimer': "plugin_hooks/call_timer.rs: your time under the connection panel's channel line (seconds always, code font); with `allCallTimers` everyone's time by their voice row, as the clock icon with a tooltip or, with `showWithoutHover`, as text under the name. Join times start when the app first sees someone in a channel.",
    'MemberCount': "plugin_hooks/member_count.rs: online, total and in-voice counts with their tooltips at the top of the member list, online and total in the server tooltip. Not ported: the original's gateway requests (`preload` of a server's online count, member list subscriptions while the list is hidden); plugin code sends nothing, so an unopened server shows '?' online until its count arrives.",
    'AnonymiseFileNames': "uploads.rs: files are renamed as they are sent, by the original's four methods; a switch first in each upload card's bar turns it off for that file. A spoilered file stays one. Its /Spoiler command is not ported: the app runs no client-side slash commands of plugins.",
    'ReverseImageSearch': 'menu.rs: "Search Image" at the end of the link group when a picture is right-clicked; a '
                          'submenu of its seven engines and All, opened in the browser. The engine favicons (fetched '
                          'from DuckDuckGo by the original) are not drawn: plugin code fetches nothing.',
    'BetterUploadButton': 'ui.rs: the composer plus opens the file picker on a click and its menu on a right click.',
    'ColorSighted': 'member_avatars.rs, ui.rs status dots, status_menu.rs: idle, Do Not Disturb and offline keep their '
                    'colour but draw the plain disk; the mobile phone stays (the original leaves it too).',
    'CopyUserURLs': 'context_menu.rs: "Copy User URL" at the end of user and DM menus; copies '
                    '`<https://discord.com/users/ID>` (angle brackets, as the original).',
    'QuickMention': 'hover_bar.rs: an @ button after the quick reactions and before Add Reaction, where you may write; '
                    'inserts `<@ID> ` in the composer (shown as text: the composer draws no mention pill for typed '
                    'syntax).',
    'AlwaysTrust': 'links.rs: any link opens without "Leaving Discord" (`domain`); server_settings.rs: Delete Server '
                   'without the typed name, behind the plugin\'s own "Delete server?" (`noDeleteSafety`, `confirmModal`).',
    'ClearURLs': 'plugins/clear_urls.rs: the ClearURLs rules, bundled; messages sent and edited.',
    'DisableCallIdle': 'dm_call.rs: no leave after 3 minutes alone in a DM call. The pinned client\'s other idle rule '
                       '(`handleIdleUpdate`: leave voice after 5 h idle, move to the AFK channel) is not in the app '
                       'yet, so there is nothing more to disable.',
    'GifPaste': 'gifs.rs: picking a GIF inserts its link and a space in the composer and closes the picker.',
    'MessageClickActions': 'plugin_hooks/message_clicks.rs: the original\'s click state machine (single, double, '
                           'triple click; keys held incl. Backspace and Delete; the three timeouts) and all 13 '
                           'actions with the app\'s own Edit, Reply, Delete and Pin (without their dialogs, as the '
                           'original), reactions, thread, quote. On by default (the owner\'s double-click to edit). '
                           'System DMs are the Discord system account\'s DM (the app keeps no `system` flag).',
    'NoPendingCount': 'unread_badges.rs and home.rs: friend request count 0, no Message Requests badge (the row stays).',
    'NoTypingAnimation': 'typing_avatar.rs: every typing indicator draws still dots and asks for no animation frames.',
    'RevealAllSpoilers': 'ui.rs: Ctrl-click a hidden spoiler opens every text spoiler of its message, Ctrl+Shift-click '
                         'those of every message the chat holds now (later ones stay hidden).',
    'UnlimitedAccounts': 'account_switcher.rs: `maxAccounts` (0 = no limit) for the list and the Add button.',
    'VoiceStats': 'plugin_hooks/voice_stats.rs: seconds with each person in your voice channel, saved every 30 s; '
                  '"Voice Time" after Member Since in the DM sidebar and the full profile. An Equicord backup brings '
                  'its totals (`VoiceStats_totals`, the one data store key read).',
    'VolumeBooster': 'User Volume (context menu, call tile menu, call tray) and Stream Volume sliders to their top x '
                     '`multiplier`; the voice engine takes the higher gain. The app syncs no volumes to the account, '
                     'so the original\'s 200% cap on the sync has nothing to cap.',
}
TESTS = {
    'AlwaysAnimate': '`each_switch_plays_its_part_and_two_patches_need_none`; frames aa-on/aa-off (avatar report); plugins-off chat, member-list',
    'GameActivityToggle': '`the_button_reads_its_settings_and_names_the_next_state`; frames gat-on/gat-off/gat-old; plugins-off chat (user panel)',
    'CancelFriendRequest': 'frames cfr-on/cfr-off (an outgoing request set locally); plugins-off voice-profile, profile-popout',
    'ServerListIndicators': '`the_item_says_what_its_mode_counts`; frame sli-on; plugins-off chat (rail)',
    'ShowConnections': '`spacing_and_settings_read_as_the_original`; frame sc-on; plugins-off profile-popout, voice-profile',
    'MessagePeek': '`plugin_hooks::message_peek::tests` (2); frame mp-on; plugins-off dm-list',
    'PlatformIndicators': '`plugin_hooks::platform_indicators::tests` (2); frames pi-on/pi-off; plugins-off member-list, chat, dm-list',
    'TypingIndicator': '`the_indicator_leaves_out_who_its_settings_leave_out`; frames ti-on/ti-off; plugins-off channel-typing',
    'ReadAllNotificationsButton': '`the_button_is_a_tile_or_compact_text`; frame ranb-on; plugins-off chat (rail)',
    'FixImagesQuality': '`fix_images_quality_loads_full_resolution`; plugins-off image-menu (an attachment picture)',
    'OnePingPerDM': '`one_ping_per_dm_lets_the_oldest_unread_ping`; no frame (a sound, not a picture)',
    'VoiceDownload': 'frames vd-on/vd-off/vd-pair; plugins-off chat (the voice message is in view)',
    'CallTimer': '`plugin_hooks::call_timer::tests` (3); frames ct-on/ct-all; plugins-off voice-profile',
    'MemberCount': '`plugin_hooks::member_count::tests` (2); frames mc-on/mc-off; plugins-off member-list',
    'AnonymiseFileNames': '`anonymised_names_follow_each_method`; frames anon-on/anon-off; plugins-off upload-card',
    'ReverseImageSearch': '`image_search_opens_each_engine_with_the_encoded_address`; frame ris-on; plugins-off image-menu',
    'BetterUploadButton': '`copy_user_urls_and_quick_mention_write_what_the_originals_write` (off); frame bub-on; '
                          'plugins-off plus-click',
    'ColorSighted': 'frames cs-on/cs-off; plugins-off dm-list, chat',
    'CopyUserURLs': '`copy_user_urls_and_quick_mention_write_what_the_originals_write`; frame url-on; plugins-off user-menu',
    'QuickMention': '`quick_mention_sits_before_add_reaction`; frames qm-on, qm-click; plugins-off hover-bar',
    'AlwaysTrust': '`always_trust_needs_its_domain_setting`, `always_trust_deletes_servers_by_its_two_settings`; '
                   'frames trust-off/on, trust-settings',
    'ClearURLs': '`plugins::clear_urls::tests` (6); frames clearurls-off/on; plugins-off send-link',
    'DisableCallIdle': '`switch_only_plugins_follow_their_switch_and_the_master`; frames idle-off/on (195 s alone)',
    'GifPaste': '`switch_only_plugins_follow_their_switch_and_the_master`; frames gifpaste-off/on; plugins-off gif-picker',
    'MessageClickActions': '`plugin_hooks::message_clicks::tests` (11); frames mca-backspace (Delete sent), '
                           'mca-plain-click (nothing), mca-on/off, mca-settings; plugins-off double-click and its control',
    'NoPendingCount': '`no_pending_count_hides_each_badge_by_its_setting`; frames pending-off/on; plugins-off requests',
    'NoTypingAnimation': '`switch_only_plugins_follow_their_switch_and_the_master`; plugins-off typing',
    'RevealAllSpoilers': '`ctrl_reveals_the_message_and_ctrl_shift_every_loaded_message`; frames spoilers-off/on',
    'UnlimitedAccounts': '`unlimited_accounts_lifts_the_five_account_limit`; frames ua-off/ua-on; plugins-off accounts',
    'VoiceStats': '`plugin_hooks::voice_stats::tests` (5), backup import test; frames vs-dm-on/off, vs-full-on',
    'VolumeBooster': '`volume_booster_raises_the_slider_top_by_its_multiplier`; frames vb-off/vb-on; plugins-off user-volume',
}
# Original settings a port does not have, with why (none today).
SKIPPED_SETTINGS = {}

# Already what gravecord does without a plugin.
NATIVE = {
    'NoTrack': 'gravecord sends no analytics, metrics or Sentry reports; its only `/science` post is the overlay problem '
               'report the user submits.',
    'ForceOwnerCrown': 'the member list shows the owner crown at any server size.',
    'NormalizeMessageLinks': 'Copy Message Link already writes discord.com links.',
    'NoPushToTalk': "the app does not enforce Use Voice Activity yet (the pinned client's PermissionVADStore and its "
                    "warning), so voice activity already works where a server requires push to talk; when the app adds "
                    "the rule, this plugin needs its hook there.",
    'NoUnblockToJump': "the app has no 'unblock to jump' dialog yet, so jumping to a blocked or ignored user's message "
                       "already works; when the app adds the dialog, this plugin needs its hook there.",
}

# Plumbing of the client mod itself: nothing to port.
API = {n for n in enabled if n.endswith('API')} | {
    'ConcatenatedComponentExtractor', 'ConcatenatedModules', 'CrashHandler', 'EquicordHelper', 'NoDevtoolsWarning',
    'ReactErrorDecoder', 'SupportHelper', 'ThemeAttributes',
}
NOT_APPLICABLE = {
    'WebContextMenus': 'web build only (the desktop app has these menus).',
    'WebKeybinds': 'web build only.',
    'WebScreenShareFixes': 'web build only (Chromium bitrate cap).',
    'DisableDeepLinks': 'web build only.',
    'BetterSettings': 'settings are native here: no crossfade or lazy loading to remove.',
    'Settings': 'its settings UI is the Gravecord settings group itself; `settingsLocation` not ported (the group sits '
                'before Developer).',
    'YoutubeAdblock': 'needs an ad filter inside the YouTube web view of embeds; deferred with the web host.',
    'VCSupport': 'Equicord support-server helper.',
    'NoDevtoolsWarning': 'no web console in a native client.',
    'CrashHandler': 'recovers React render crashes; there is no React here.',
    'ReactErrorDecoder': 'decodes minified React errors; there is no React here.',
    'ThemeAttributes': 'adds DOM attributes for CSS themes; there is no DOM or CSS here.',
    'EquicordHelper': 'workarounds for Discord web crashes in the mod.',
    'SupportHelper': 'support-channel helper of the mod.',
}
# Not built yet: what each needs in this app.
PLANNED = {
    'AccountPanelServerProfile': 'a right-click menu on the account panel that opens the server profile popout.',
    'AltKrispSwitch': 'Krisp, which is blocked here (its signature check); without it there is nothing to switch to.',
    'BetterActivities': 'activity icons in member list rows and every activity in the profile popout.',
    'BetterAudioPlayer': 'a spectrograph and oscilloscope in the audio attachment player (needs the decoded samples).',
    'BetterBlockedUsers': 'search in the blocked list, selectable names, no blocked-profile prompt, grouped blocked messages.',
    'BetterCommands': 'changes to the slash command UI.',
    'BetterFolders': 'a second server sidebar for folders.',
    'BetterForwards': 'forward dialog tweaks.',
    'BetterGifAltText': "a place where alt text shows: the original changes the `alt` that screen readers read, and the app draws images without an accessibility tree.",
    'BetterPlusReacts': '"+:emoji:" reacting to the Nth message above, in the composer\'s send path.',
    'BetterRoleContext': 'role context menu rows (copy colour, edit role, view icon).',
    'BetterRoleDot': 'copy a role colour on its dot; dots and coloured names together.',
    'BiggerStreamPreview': 'a larger stream preview popout.',
    'BypassStatus': 'notification and sound rules that pass Do Not Disturb for chosen users, channels and servers.',
    'ClipsEnhancements': 'more clip FPS and duration options in the clips settings.',
    'ClipUpload': 'a button to upload a local video as a clip.',
    'CustomRPC': 'a custom rich presence editor and its activity.',
    'Decor': 'its third-party avatar decorations service (decor-auth in the backup\'s data store is not copied).',
    'DownloadAllAttachments': 'a hover-bar button that saves every attachment of a message.',
    'Experiments': 'the experiments and developer pages.',
    'FavoriteEmojiFirst': 'favourite emoji first in the emoji autocomplete.',
    'FixYoutubeEmbeds': 'the YouTube embed workaround inside the web host.',
    'FriendInvites': 'slash commands for friend invite links.',
    'FullSearchContext': 'the full message menu on search results.',
    'FullVCPFP': 'avatars filling voice call tiles.',
    'GifMaker': 'a GIF encoder and its editor.',
    'GlobalBadges': 'its third-party badge service.',
    'GreetStickerPicker': 'a sticker choice for the "Wave to say hi!" button.',
    'IgnoreActivities': 'a list of activities kept off your status.',
    'ImageZoom': 'a zoom lens in the image viewer.',
    'LimitlessScreenshare': 'resolution and FPS sliders past the tier limits (owner: later, FakeNitro-style).',
    'MessageLogger': 'a store of deleted and edited messages and their red/edited display.',
    'MoreCommands': 'its slash commands.',
    'MuteChannelMentions': 'mention badges hidden for chosen channels.',
    'NewGuildSettings': 'notification defaults applied when joining a server.',
    'NoNitroUpsell': 'every Nitro upsell hidden (the app draws them from the pinned client).',
    'OpenInApp': 'Spotify, Steam and other links opened in their apps.',
    'PermissionFreeWill': 'no client-side limits in channel permission editing.',
    'PermissionsViewer': 'a permission breakdown in profiles and menus.',
    'PinDMs': 'pinned DM categories in the DM list.',
    'PreviewMessage': 'a preview of the message before sending.',
    'Quoter': 'a quote image made from a message.',
    'RelationshipNotifier': 'notices when a friend, group or server removes you (its data store lists are not copied).',
    'RoleColorEverywhere': 'role colours in mentions, lists, voice and reactions.',
    'SearchFix': 'its search request fix.',
    'SedEnhanced': '`s/a/b/` editing the last message, in the send path.',
    'ServerInfo': 'a server information dialog.',
    'ShowHiddenThings': 'hidden and moderator-only things shown.',
    'SilentMessageToggle': "the pinned client's `@silent` prefix first: its send path strips it and sets SUPPRESS_NOTIFICATIONS (the `silentPrefix` rule and the sends of modules 857069 and 92961); the app sends the text as typed, so the plugin's prefix would reach people as words.",
    'SpotifyCrack': 'listen along without Premium and no Spotify pause on voice.',
    'SpotifyShareCommands': 'its /track, /album, /artist commands.',
    'SteamStatusSync': 'status sync with the Steam client.',
    'StopAutoUnread': 'no automatic unread marking.',
    'Streaks': 'DM streak counts (its service login in the data store is not copied).',
    'StreamingCodecDisabler': 'a codec choice for streams and cameras in dc-media.',
    'SuperReactionTweaks': 'super reaction limits and default.',
    'Translate+': 'message translation (needs a translation service).',
    'TypingTweaks': 'avatars and role colours in the typing line.',
    'UnlockedAvatarZoom': "the avatar crop tool (Edit Image of a new avatar), which the app does not have yet; the plugin only raises its zoom slider's maximum.",
    'UserMessagesPronouns': 'pronouns by names in chat.',
    'UserPFP': 'its third-party avatar service.',
    'USRBG': 'its third-party banner service.',
    'ValidReply': 'replied messages fetched when missing.',
    'ValidUser': 'unknown mentioned users fetched.',
    'ViewIcons': 'avatars, banners and icons opened full size, with menu rows.',
    'VoiceMessages': 'recording voice messages on desktop.',
    'VoiceMessageTranscriber': 'an on-device Whisper model runner.',
    'WhoReacted': 'reactor avatars on reactions.',
    'ZipPreview': 'a ZIP listing in file attachments.',
    'BetterQuickReact': 'more quick reaction buttons in the message context menu.',
    'ExpressionCloner': 'menu rows that clone, edit and delete emoji and stickers into your servers.',
    'IloveSpam': 'messages of users flagged as likely spammers shown as any other.',
    'LimitMiddleClickPaste': 'middle-click paste only into a focused text box, and middle-click link opening.',
    'MessageTags': 'saved tags sent as custom slash commands.',
    'TenorGifSearch': 'Tenor or Giphy as the GIF search instead of the default provider.',
}

errors = []
rows, setting_rows = [], []
counts = {'ported': 0, 'native': 0, 'not applicable': 0, 'planned': 0}
for name in enabled:
    upstream = RENAMED.get(name, name)
    src, desc, original = sources.get(upstream, ('', '', []))
    if name in ours:
        if name not in PORTED:
            errors.append(f'{name} is in the registry but has no PORTED note')
            continue
        status, note, test = 'ported', PORTED[name], TESTS[name]
        here = set(ours[name])
        for key, kind, default in original:
            if key in here:
                why = idle.get((name, key))
                state = f'nothing to act on yet: {why}' if why else 'ported'
            elif key in SKIPPED_SETTINGS.get(name, {}):
                state = 'not ported: ' + SKIPPED_SETTINGS[name][key]
            else:
                errors.append(f'{name}.{key} of the original is missing and has no reason')
                state = 'MISSING'
            setting_rows.append(f'| {name} | `{key}` | {kind} | `{default}` | {state} |')
        for key in sorted(here - {key for key, _, _ in original}):
            errors.append(f'{name}.{key} is not a setting of the original')
    elif name in NATIVE:
        status, note, test = 'native', NATIVE[name], ''
    elif name in API or name in NOT_APPLICABLE:
        status, test = 'not applicable', ''
        note = NOT_APPLICABLE.get(name, 'client-mod plumbing (an API for other plugins), nothing to port.')
    elif name in REMOVED:
        status, note, test = 'not applicable', REMOVED[name], ''
    else:
        status, test = 'planned', ''
        note = PLANNED.get(name, '')
        if not note:
            errors.append(f'{name} is planned without a reason')
        if upstream != name:
            note = f'now {upstream} upstream. ' + note
    counts[status] += 1
    if not src:
        src = '(not in the 2026-11-14 checkout)'
    desc = desc.replace('|', '/')
    rows.append(f'| {name} | {status} | {src} | {desc} | {note} | {test} |')

with open(OUT, 'w', encoding='utf-8', newline='\n') as out:
    out.write('# PLUGINS.md\n\n')
    out.write('Built-in equivalents of the Vencord/Equicord plugins the owner keeps on. Not part of the pinned client and not\n')
    out.write('`FEATURES.md` rows. Framework: `app/src/plugins.rs` (registry, state, Backup & Restore) and the Gravecord settings\n')
    out.write('group (`app/src/ui/settings/mods.rs`). Decisions: `DECISIONS.md`, "Built-in plugins". Written by\n')
    out.write('`tools/gen-plugins-md.py`, which fails on a missing setting or a plugin without a reason.\n\n')
    out.write('Plugins off is the pinned client: `cargo xtask plugins-off` captures every scenario of\n')
    out.write('`tests/plugins-off/scenarios.txt` with the plugin code skipped, with the master switch off (every plugin on and\n')
    out.write('every setting changed), and with every plugin off, and fails on one differing pixel or on any plugin effect.\n\n')
    out.write('The list is the plugins enabled in the owner\'s Equicord backup of 2026-09-21 (156). Sources were read in the local\n')
    out.write('Equicord checkout of 2026-11-14 (`src/plugins` is Vencord\'s, `src/equicordplugins` Equicord\'s).\n\n')
    out.write(f'Totals: {counts["ported"]} ported, {counts["native"]} native, {counts["planned"]} planned, '
              f'{counts["not applicable"]} not applicable.\n\n')
    out.write('Status: **ported** works and has its proof; **native** is what gravecord already does without a plugin;\n')
    out.write('**planned** is not built yet (an import keeps its settings for when it is); **not applicable** has nothing to\n')
    out.write('port in a native client, or was removed upstream.\n\n')
    out.write('| plugin | status | source | what it does (original) | here / what it needs | proof |\n')
    out.write('| - | - | - | - | - | - |\n')
    out.write('\n'.join(rows) + '\n\n')
    out.write('## Settings of the ported plugins\n\n')
    out.write('Every setting of each original (`definePluginSettings`), its type and default there, and what it does here.\n\n')
    out.write('| plugin | setting | type | default (original) | here |\n| - | - | - | - | - |\n')
    out.write('\n'.join(setting_rows) + '\n')
print(counts)
if errors:
    print('\n'.join('error: ' + e for e in errors))
    sys.exit(1)
