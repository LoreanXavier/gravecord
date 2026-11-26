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
    'TenorGifSearch': 'GifProviderSwitcher',
    'MessageTags': 'CustomCommands',
    # 15f8ecf0 "merge and improve YTAdblock and FixYtEmbeds".
    'FixYoutubeEmbeds': 'BetterYouTubeEmbeds',
    'IloveSpam': 'ILoveSpam',
    'LimitMiddleClickPaste': 'MiddleClickTweaks',
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
    """`key: { ... }` and `key: helper(...)` entries at the top of an object literal."""
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
            if depth == 0 and c == '(':
                # `key: helper(...)`: the helper's name, for the caller to look up.
                m = re.search(r'(\w+)\s*:\s*(\w+)\s*$', inner[:i])
                if m:
                    out.append((m.group(1), m.group(2) + '('))
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
            # The definition's top level: four spaces in, or two in a file indented by two (LimitlessScreenshare).
            name = re.search(r'\n(?: {4}| {2})name:\s*"([^"]+)"', block)
            if not name:
                continue
            rel = os.path.relpath(path, ROOT).replace(os.sep, '/')
            desc = re.search(r'\n(?: {4}| {2})description:\s*"([^"]*)"', block)
            settings = []
            holder = text
            s = re.search(r'definePluginSettings\(\{', holder)
            # Some plugins keep their settings in a file of their own beside the index (`settings.ts`).
            if not s:
                for other in sorted(fn):
                    if other != f and other.endswith(('.ts', '.tsx')):
                        holder = open(os.path.join(dp, other), encoding='utf-8', errors='replace').read()
                        s = re.search(r'definePluginSettings\(\{', holder)
                        if s:
                            break
            # Settings made from a table of switches (`definePluginSettings(Object.entries(Rules).reduce(...))`,
            # OpenInApp): one BOOLEAN per key of the table, default true.
            computed = re.search(r'definePluginSettings\(\s*Object\.entries\((\w+)\)', text)
            if not s and computed:
                table = re.search(r'const\s+' + computed.group(1) + r'[^=]*=\s*\{', text)
                if table:
                    for key, _ in top_level_entries(balanced(text, table.end() - 1)):
                        settings.append((key, 'BOOLEAN', 'true'))
            if s:
                for key, body in top_level_entries(balanced(holder, s.end() - 1)):
                    # A setting made by a helper of the file (`showTimeouts: opt("...")`, ShowHiddenThings): the
                    # helper's own object gives the type and default.
                    helper = re.match(r'\s*(\w+)\(', body)
                    made = helper and re.search(r'const\s+' + helper.group(1) + r'\s*=[^{]*\(\{', holder)
                    if made and not re.search(r'type:\s*OptionType', body):
                        body = balanced(holder, made.end() - 1)
                    kind = re.search(r'type:\s*OptionType\.(\w+)', body)
                    # A select marks its default option with `default: true`; take that option's value.
                    picked = re.search(r'value:\s*([^,}\n]+?),\s*default:\s*true', body)
                    default = re.search(r'\bdefault:\s*([^,\n]+)', body)
                    default = picked.group(1).strip() if picked else (default.group(1).strip() if default else '')
                    settings.append((key, kind.group(1) if kind else '?', default))
                # Settings the plugin keeps without a settings row (`.withPrivateSettings<RpcConfig>()`, CustomRPC):
                # each field of that interface, kind PRIVATE, no default.
                private = re.search(r'\}\)\.withPrivateSettings<(\w+)>\(\)', holder[s.end():])
                face = private and re.search(r'interface\s+' + private.group(1) + r'\s*\{', holder)
                if face:
                    for key in re.findall(r'^\s*(\w+)\??:', balanced(holder, face.end() - 1), re.M):
                        settings.append((key, 'PRIVATE', ''))
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
    'LimitlessScreenshare': "plugin_hooks/limitless_screenshare.rs, plugins/limitless_screenshare.rs, dc-media `Resolution::Custom` and `Settings::unlocked`: `canStreamWithSettings` answers yes, so every Go Live choice is open whatever the account (no Nitro wheels or upsell on the choices; Gaming and Screenshare resolve to their top pairs, 1440p 60 and Source 15). The picker's Screen Resolution and Frame Rate menus and the running stream's Stream Quality menu get a slider (3p up to Max Resolution, in 10p steps with Round Resolution; 1 up to Max FPS) above the plugin's preset lists in place of the official ones (`OptionsRange`, `SettingsRange`). A choice off the official table is sent as it is: the capture is scaled to that height (16:9 box, never upscaled, at most 4K) and the encoder runs at that rate (up to 240), with the official bitrate rule; while the plugin is on such a stored choice is read back, while it is off it falls back to the pinned defaults. The Stream Quality slider restarts the stream at most once a second (`COOLDOWN_MS`). The presets (`resolutions`, `fpss`) are edited in the card's own section: chips with Delete (Source stays) and a digits-only box with the add button, `Must be >3` / `Must be >1` as the original.",
    'GifMaker': "plugin_hooks/gif_maker.rs, plugins/gif_maker.rs: \"Make GIF\" ends a message's menu (a message with a picture or video attachment, a .gif link, or an embed's GIF, video or picture: `getMediaInfo`) and the Image Actions menu of a picture or video link; \"Edit GIF\" ends the GIF picker's menu. The dialog \"Make GIF\" shows the GIF as it will be sent and Captions (None; Caption: its text and Font size 10 to 100, a white band over the picture with the words wrapped and centred; Bubble: the original's speech bubble, its tip placed by a click on the preview, Tip Base 0 to 80%) and Dimensions (Width and Height, 32 to 1024; the start size is the media's fitted into Max Width x Max Height, else the last one). The GIF is made off the UI thread 300ms after the last change and a newer change stops it: a still picture as one frame, an animated GIF or WebP frame by frame at its own delays, an H.264 video sampled 30 times a second, at most 200 frames, each stretched to the size, the caption drawn with tiny-skia from the app font's outlines, and encoded as it is made (one frame held). Send puts export.gif in the open channel's message box and closes; Export saves it into Downloads. Every change is stored as the original's hidden settings. The media is the full file once loaded, else the picture the chat or picker already shows. GAP: the caption is set in gg sans in place of the original's Google fonts; WebM and QuickTime videos are not decoded (H.264 MP4 only); the source has no crop or frame range, so neither has the port.",
    'BetterFolders': "plugin_hooks/better_folders.rs: while a folder is open its servers leave the rail (which draws the open folder's header and strip alone) and are drawn in a 72px column beside the rail (`FolderSideBar`): the folder's icon and its tinted strip over its servers as Show Folder Icon says (Never, Always, When more than one folder is expanded), several open folders one under the other in the rail's order; the column slides open and shut over 200ms with Sidebar Animation, is not drawn while no folder is open, and the user panel and the channel list's resize handle move with it. Keep Icons draws the open folder closed in the rail (its small icons). Clicking the column's folder icon closes the folder, its right click opens the folder's menu. Rules (`CHANNEL_SELECT`, `TOGGLE_GUILD_FOLDER_EXPAND`, the Home button, `LOGOUT`): Force Open, Close Server Folder, Close All Folders (a server outside folders, or Home), Close Others, Close All Home Button, every folder closed at log out. GAP: servers in the column cannot be dragged (drags start and drop in the rail only).",
    'Experiments': "plugin_hooks/experiments.rs, plugins/experiments.rs, plugins/experiments_table.rs (written by tools/gen-experiments.py from the pinned modules: 27 legacy and 250 apex experiments): the developers' \"Staff Only\" section after Games & Apps (module 158935 `DEVELOPER_SECTION`) with the Experiments page (`SS`): the original's warning card, the search (module 96919 `oC`: the rows matching the most words first), every registered experiment, overridden ones first, then the newest `YYYY-M`, then by title (`R3`); a row (title, name, User or Guild) opens to Bucket Override or Variant Override (module 271478) with its options (legacy: Not Eligible, Control Bucket, \"Treatment N: label\"; apex: \"Variant N\"), Clear Override while one is set, the current assignment without the override (\"Currently assigned to bucket/variant N\", \"Currently unassigned\", or the guilds' \"N guilds in bucket B\"), the no-config warning, and More Details (the server's and the override's descriptor). Overrides are stored (`plugin-experiments-overrides`) and given to the account's experiment stores (the override layer ported in d9417013) while the plugin is on; turned off, they are taken away. GAP: the Developer Options page (logging, overlays, tracing) and the developer flag's other effects (experiment and playground link embeds, the dev menu); apex guild experiments (the app keeps no apex guild assignments, so their override is stored but acts on nothing); the server's experiment metadata (titles of apex experiments are their names, as the official page shows unregistered metadata); Recent Exposures (no exposures are tracked); the Installation ID line; the override select is a list rather than a dropdown.",
    'Quoter': "plugin_hooks/quoter.rs, plugins/quoter.rs: \"Quote\" ends a message's menu (a message with text, a group of its own as `children.push` places it) and opens \"Catch Them In 4K.\": the quote picture drawn as the original's `createQuoteImage` (1200x600 on black, the author's avatar over the left 600x600 at 512 when loaded, else the chat's file, the grayscale pass as `saturation` with white, the black fade from x 200 to 600, the quote centred in the right half at the largest size from 42 down to 18 that fits 480 high, user mentions as @username, custom emojis as their pictures or a square, \"- name\" and \"@username\" under it, the watermark in the corner), drawn off the UI thread with tiny-skia from the glyph outlines of the app's own font; switches Grayscale, Save as GIF and Show Watermark with its text (32 characters), each stored when changed (the original's hidden settings); Export saves `<preview> - <username>.png|gif` into Downloads under a free name, Send puts it in the open channel's message box as an upload and closes. GAP: the quote is set in gg sans (regular, italic for the author) instead of the original's Google fonts at weight 300; glyphs gg sans lacks (emoji, CJK) draw its missing-glyph shape; no kerning. The watermark's default reads Made with Gravecord (the original names its own mod).",
    'BetterGifAltText': "plugin_hooks/better_gif_alt_text.rs: `altify` where the app shows or exposes alt text: a GIF attachment (or one without a content type) with no description of its own gets \"GIF - <words of its file name>\" in the viewer's View Details \"Description (Alt Text)\" row (module 2323, the zoomable image's props the original patches) and as the accessible name of its tile; a GIFV embed's accessible name (module 265705 `E`, `aria-label`) reads \"GIF - <words of its page address>\". Names lose their digits and `.gif`, split at `,-_ ` and keep 20 words and 300 UTF-16 units, as the original's. Change from the original: the address's query and fragment are left out (attachment addresses now end in a signed `?ex=&is=&hm=` that would end up in the words). The accessible names were ported first (0228bfa0). GAP: the ALT button and its popout keep the sender's description only (read in modules 548146 and 383055, not run: `tp` decides on the mosaic item's own `alt`, while the original changes the props of the zoomable image inside it); image embeds other than GIFV have no accessible name here, so the original's patch of every zoomable image without a content type has no place.",
    'BetterAudioPlayer': "plugin_hooks/better_audio_player.rs, plugins/better_audio_player.rs: a canvas over the audio player (module 153440 type AUDIO: the attachment card and the audio embed), drawn while it plays from the player's own decoded samples through an AnalyserNode of the original's (fftSize 2048, Blackman window, smoothing 0.8, -100 to -30 dB, byte scaling as the Web Audio specification gives it), the window ending at the heard position; drawOscilloscope (three-fold swing, one colour for the whole line, the last point's, as the original strokes once) and drawSpectrograph (bars of width/1024 every width/1024 + 0.5 px, the low bins that fit, scaled to the loudest, a vertical gradient unless solid); when it stops the last frame stays, and a player that never played draws nothing; no work while not playing. Colours read as `R, G, B` or `#RRGGBB`, anything else as the default (the original's toast and rewrite of the text are not ported). The original's 12 MB cap is not ported: it guards its second download of the file into memory, and here the samples are the player's own. The analyser reads the samples before the player's volume.",
    'BetterPlusReacts': "plugin_hooks/better_plus_reacts.rs: the send path's `+:emoji:` rule (ui/send_rules.rs, module 751258) takes more pluses, one per message back (`+:x:` the newest, `++:x:` the one before; no plus is no message, so `:x:` is sent as written), and the composer's Add reaction list opens for a first word of several pluses; choosing an emoji there keeps the typed pluses, so it reacts to the message they pick.",
    'FullSearchContext': "plugin_hooks/full_search_context.rs: a right press on a search result opens the chat's full message menu (761304) for the result's own message and channel instead of the search's short one (847342), with Copy Author ID after Copy Message ID in developer mode; Reply and Edit first open the result's channel at the message. GAP: a result from a voice channel's chat opens the channel's call view, where the chat panel (and so the reply bar) is not open.",
    'SedEnhanced': "plugin_hooks/sed_enhanced.rs, plugins/sed_enhanced.rs: the send path's `s/find/replace` rule (ui/send_rules.rs, module 751258) takes the original's wider pattern (separators `/|$#@!`, the closing one optional, modes `rgmisudyv`), edits the message replied to when it is yours (a reply to someone else's sends nothing) or else your last editable message; `r` (inverted by regexByDefault) makes the find a regular expression with `gmisudyv` as flags, `g` changes every match, JavaScript `$` forms expand, a pattern that does not compile sends nothing, an empty result without attachments deletes. GAP: expressions run on Rust's linear-time `regex`, without look-around or back references (such a pattern sends nothing).",
    'ShowHiddenThings': "plugin_hooks/show_hidden_things.rs: showTimeouts drops the moderator test of the chat timeout clock (showCommunicationDisabledStyles, 549587), showInvitesPaused the Manage Server test of the rail tooltip's Invites paused line (347951 `Fi`, read only while paused), showModView makes the Mod View gate (985925) answer yes in every server, so the user menu offers Open in Mod View everywhere. The pinned pieces were ported in 1ee62c8d. GAP: the user popout's Mod View button (348434, `PRESS_MOD_VIEW`, which the original also shows on yourself) is not ported, so that part has nothing to act on; the highest-role fix is not needed here (Mod View reads the member's roles itself).",
    'ValidReply': 'plugin_hooks/valid_reply.rs: the pointer entering a reply\'s "Message could not be loaded" bar (448368, ported in 1ee62c8d) fetches GET /channels/<c>/messages?limit=1&around=<id> once at a time; the first message of the answer is the reply\'s message and the bar shows it, another one means it is gone and the bar reads "Original message was deleted"; a failed fetch changes nothing. The fetched message is kept for the bar only.',
    'AltKrispSwitch': "plugin_hooks/alt_krisp_switch.rs: the voice panel's Noise Suppression popout switch turns the suppressor off to None instead of Standard (shouldCancelSuppression drops the popout's Standard dispatch); the suppressor here is DeepFilterNet in Krisp's place, so the switch goes between None and DeepFilterNet. The settings page's select is unchanged, as the original only acts on the popout's section.",
    'ClipUpload': "plugin_hooks/clip_upload.rs + plugins/clip_upload.rs: the composer's + menu ends with Upload Clip File (CloudUploadIcon), which opens the original's dialog: Select File (MP4/M4V up to 500 MB) and Parse clip metadata from file (the clip box's game name, users and game id fill Title, Participants and Game/Application when empty), File name, Title, Participants (friends who are not bots, by label, with the consent line), Created at (datetime-local, Created at must be a valid date.), Message, Game/Application (games running, seen and candidates, or a custom id) and Spoiler/Remix/Thumbnail; Upload stamps a copy with the empty clip box (dc_clips::mp4::replace_metadata, the box updateClipMetadata writes), reserves it with is_clip and the clip fields, puts it and sends the message with the pending reply's reference (Checking clip file. / Clip uploaded. / the error toast). GAP: the FFmpeg conversion retry on not a valid clip (no FFmpeg here: a refused file ends with the original's not compatible text), the clip button on a clip attached in the composer (clips are shared from the gallery here), selects drawn as lists under their fields.",
    'CustomRPC': "plugin_hooks/custom_rpc.rs + plugins/custom_rpc.rs: a local activity of its own (LOCAL_ACTIVITY_UPDATE from the CustomRPC socket, after the games') while enablePresence is on, built as createActivity builds it (type, ids, lines and links, stream link, timestamps None/Since open/Your time/Custom with the repeating span, pictures turned into asset ids by fetchAssetIds, buttons with metadata, party); the card shows the original's notices with their buttons, help lines with links, the activity as the profile draws it, presets (saved, loaded, deleted in the store) and the whole form with each field's check and message. GAP: the toolbox toggle (no toolbox).",
    'ExpressionCloner': "plugin_hooks/expression_manager.rs (now ExpressionManager upstream): sticker menus in the picker and in messages get Clone Sticker (name field with the original's 2..30 check, server tiles with picture or acronym and tooltip, the sticker fetched from the media proxy at the largest size under 512 KiB and posted to the server, success or failure toast), Edit Sticker and Delete Sticker where you manage expressions (editAndDelete; delete after the original's confirmation), Copy/Open Sticker Link in place of Copy Sticker Image Link (stickerCopyAndOpen); custom emoji in the picker get Delete Emoji, a default emoji's Copy Emoji its CopyIcon; recents adds Remove From Recents after the favourite row of a frecent emoji or sticker (the frecency proto sent back without the entry; no confirmation in the reaction picker or with Shift). GAP: Clone Emoji, Edit Emoji and Upload as Emoji need the Emoji Studio the app has not got yet.",
    'BetterForwards': "plugin_hooks/better_forwards.rs + plugins/better_forwards.rs: the forward dialog of a message with attachments or embeds gets Forward everything (InfoIcon tooltip) and, in place of the file line, TagGroup pickers of its attachments (kind icons) and embeds (title over a tag per embed and extra image), sent as message_reference.forward_only (empty choice as attachment 0, embed indices sorted); resendOnFail sends failed forwards again as forwardPreface-quoted text with file links five to a message and the note last instead of Forward Failed; dontFollowForwards stays put unless Shift is held on Send; selfForward lists the current channel; a forward's footer becomes the server (other server: picture or acronym, name, opens the profile card), the channel (picture, name or italic unknown, goes to the message) and the time.",
    'MessageLogger': "plugin_hooks/message_logger.rs: a deleted message stays (Stores::keep_deleted) in red text or the red overlay (deleteStyle), pictures grey until hovered, its hover bar down to Clear history; edits keep the replaced text with the edit's time (inlineEdits: dimmed earlier versions with their (edited) marks above the content; the message's own (edited) opens Message Edit History with its tabs and Show Diff); logDeletedAttachments keeps dropped attachments grey; showEditDiffs/separatedDiffs colour additions and removals (createWordDiff's LCS); the message menu drops the quick reactions and buttonsToRemove on a deleted message and ends with Toggle Deleted Highlight, Enable/Disable Diff View and Clear Message History; channel, thread, DM and person menus get Clear Message Log; collapseDeleted folds N deleted messages; ignore rules of shouldIgnore (bots, self, self edits, users, channels or their category, servers, the support bots); the up arrow skips deleted messages.",
    'PermissionsViewer': "plugin_hooks/permissions_viewer.rs: View Permissions in a member's menu (their roles group, or a group of its own before the last), a server channel's (after the mute rows), the server menu's and the server header dropdown's privacy group (SafetyIcon); the xl dialog <name> Permissions lists roles / overwrites / the owner (missing overwrite members asked for with GUILD_MEMBERS_REQUEST) and, for the chosen one, each PermissionsBits permission allowed or denied with its title and an info tooltip of its description (getGuildPermissionSpecMap, the four rows the server page hides included); a right click gives BetterRoleContext's rows with Copy Role ID, or Copy User ID; a member's profile role list ends with the shield button whose 500px popout shows the granted permissions as role pills with Granted By tooltips, the sort switch (permissionsSortOrder) and Role Details.",
    'Translate+': "plugin_hooks/translate_plus.rs + plugins/translate_plus.rs: Translate after Copy Text in a message's menu (icon before and after the label) and a hover bar button, for a message with text; Toki Pona (isTokiPona's word share), sitelen pona glyphs (read back with the pinned sitelen-pona dictionary when sitelen is on) and Shavian (the pinned shavian dictionary, one punctuation mark off each end) as the original detects them, the Toki Pona translator for the first, Google Translate (gtx) to target for the rest and for a non-English target; the translation is drawn last under the message as icon, markdown text and (translated from <language> - Dismiss) in muted italic; a failed Google call shows the original's notice, a failed Toki Pona call nothing.",
    'SteamStatusSync': "plugin_hooks/steam_status_sync.rs: a change of your status (or of game activity sharing) opens steam://friends/status/ with the Steam status chosen for it (Disabled leaves Steam alone; Invisible while game activity is hidden with goInvisibleIfActivityIsHidden); the first status seen only starts the watch, a capture run opens nothing.",
    'FixYoutubeEmbeds': "plugin_hooks/fix_youtube_embeds.rs: youtubeDescription shows a video embed's description, folded to 20 characters and ... (a click opens and folds it). The Referer fix for YouTube's embedded player page has no request to change here: this client opens no player page.",
    'TenorGifSearch': "plugin_hooks/gif_provider_switcher.rs + plugins/gif_provider.rs (now GifProviderSwitcher upstream): the GIF picker's category tiles, trending results, searches and shares go to Tenor or Giphy (provider; Klipy keeps Discord's), their answers turned into Discord's shapes off the UI thread (Request::GifProvider, paged to the picker's 50); the search field reads Search Tenor and each result tile has its source's badge (in the pinned client's primary badge colours: the client mod's --badge-background-brand is not a token of this Discord build). GAP: the provider choice beside the search field and several providers at once (provider is set in the plugin settings), search suggestions and the /tenor command (the app has neither).",
    'SpotifyCrack': "plugin_hooks/spotify_crack.rs: every Spotify account reads as Premium in SPOTIFY_PROFILE_UPDATE (listen along without Premium); noSpotifyAutoPause keeps speaking in voice from pausing Spotify after 30 seconds. keepSpotifyActivityOnIdle is idle: this client does not send your Spotify activity yet.",
    'SearchFix': "plugin_hooks/search_fix.rs: a search page past offset 5000 is asked for in the other sort order from offset 0, as the plugin rewrites the query; the card carries its note.",
    'GlobalBadges': "plugin_hooks/global_badges.rs: the badges of other client mods (apiUrl/users, fetched while on and every 30 minutes; Vencord's and Equicord's left out, each mod behind its Show switch) lead a profile's badge row, round at 90% with their text as tooltip and Copy Badge Name / Copy Badge Image Link on a right click; the card has Join GlobalBadges Server. GAP: the badge directory entry a click opens in the original, the toolbox's Refetch, the Allow Host notice; the Show switches are listed instead of the multi-select.",
    'UserPFP': "plugin_hooks/userpfp.rs: the UserPFP list (databaseSource, fetched while on, hidden as in a release build) and the avatars you set with Set Avatar in a person's menu (stored locally, the Custom Avatar dialog with Original/Local previews, the PNG/GIF URL field, a PNG/JPEG dropped or chosen as a data URL, Delete and Save) replace avatars wherever they are drawn, unless preferNitro and the avatar is animated; UserPFP-hosted pictures are asked for still and GIFs play where avatars animate; overrideServerAvatars puts the custom (else the person's own) avatar over server avatars; the card has the local-only note and the Join UserPFP Server and Support UserPFP here buttons. GAP: the Allow Host notice (no content security policy here).",
    'USRBG': "plugin_hooks/usrbg.rs: the USRBG list is fetched once while the plugin is on; a listed person's profiles show that banner (in place of their own unless nitroFirst) and, with voiceBackground, their voice tile without video shows it as a cover background; the card has the Get your own USRBG banner button.",
    'StreamingCodecDisabler': "plugin_hooks/streaming_codec_disabler.rs + dc-voice signal.rs: disableH264Codec puts H264 into a voice connection's codec list (ops 1 and 14) with encode false, as filterCodecs does; the other four codecs are not offered by this client.",
    'VoiceMessages': "plugin_hooks/voice_messages.rs: the + menu ends with Send Voice Message (Send Audio File as Voice Message beside the client's own Record Voice Message row; in a server with Send Messages and Send Voice Messages); Record Voice Message records from the microphone with the plugin's echoCancellation and noiseSuppression (Ogg Opus) or takes a chosen or dropped audio file, previews it in the voice message player (the plugin's RECORDING / ---- pill otherwise), decodes it off the UI thread for the plugin's own waveform and length, warns when it is not Ogg Opus, and sends it as a voice message with the pending reply and SilentMessageToggle's flag. GAP: the sending toast's clock icon; the warning's link drawn as text.",
    'UnlockedAvatarZoom': "plugin_hooks/unlocked_avatar_zoom.rs: the image crop tool's zoom slider (every picture it edits, as the patched maxValue of module 142630) reaches zoomMultiplier instead of 2; the slider, the arrow keys and the clamp follow it.",
    'BetterQuickReact': "plugin_hooks/more_quick_reactions.rs (now MoreQuickReactions upstream): the hover bar shows reactionCount quick reactions; the message menu's row is uncut and becomes a grid of columns 44px cells (33px compact, button at 75% and emoji at 125%) showing rows rows, padded 4/1/6 and centred, the menu widening to fit; the wheel moves it a row at a time with scroll; frequentEmojis keeps the favourites out (off: they come first, as the pinned list has them).",
    'BypassStatus': "plugin_hooks/bypass_status.rs: while your status is statusToUse, a message that mentions you in a listed server or channel, or a listed person's DM (anywhere with allowOutsideOfDms when it mentions you), still notifies with the client mod's notification (the author's display name where the original reads an undefined globalName off the raw message, then (#channel, parent); mentions resolved; a click opens the message) and the message sound; own, empty, open-and-focused and (respectSilentPings) @silent messages excepted; the server, channel and user menus (a 1:1 DM row's menu is the user menu) get Add/Remove Status Bypass with the ring icons as a group before their last one. GAP: no DM channel is created for a listed person without one (the original's getOrEnsurePrivateChannel, which changes no outcome); the lists are matched with spaces removed rather than rewritten on change.",
    'RelationshipNotifier': "plugin_hooks/relationship_notifier.rs + plugin_notifications.rs: servers, group DMs, friends and incoming requests kept per account (written only on a change, at most once a second); a removal you did not make yourself (GUILD_DELETE not an outage, CHANNEL_DELETE of a group, RELATIONSHIP_REMOVE of a friend or incoming request) gives the original's notification, and at sign-in (and 5 s after turning it on) offlineRemovals tells what went while away; each kind has its switch, notices adds the notice bar's GENERIC notice with OK. The notification is the client mod's: the in-app card while focused (5 s, paused on hover, bottom right), else a desktop notification whose click opens the person's profile. GAP: the Notification Log, the desktop notification's icon.",
    'ZipPreview': "plugin_hooks/zip_preview.rs + plugins/zip_preview.rs: a .zip file attachment gets the chevron row under its name; opening it asks the native question for a Discord attachment (Cancel / Preview ZIP), fetches at most 50 MB off the UI thread, reads it as fflate's unzipSync (stored and deflated entries, folders dropped, sorted, 1000 at most) and lists it with the folder path as links; a text or picture file of 10 MB at most opens in the 680 wide dialog (code block with Copy and Download, or the picture with Download); the states and errors are the original's. GAP: the 180 ms open and close animation.",
    'SilentMessageToggle': "plugin_hooks/silent_message_toggle.rs: the chat bar's bell after PreviewMessage's eye (1.2 times larger, crossed out in the danger colour while off, Enable/Disable Silent Message); while on, a message that goes out gets the @silent prefix (the base's silentPrefix send turns it into SUPPRESS_NOTIFICATIONS) and a voice message the flag itself; autoDisable turns it off after one; persistState none resets it on a channel change, restarts keeps it in storage plugin-silent-message-toggle.",
    'MoreCommands': "plugin_hooks/more_commands.rs: its 50 commands in the Built-In section while it is on: system info, uptime, time, choose, dice, coin, ask, random cat/dog/neko/anime-boys pictures (their public APIs), random number, transform (case, repeat, reverse, NFC/NFD/NFKC/NFKD), word count, countdown, ping, echo, lenny, mock, slap, freaky (addFreakyEnding), morse, uwuify, gifroulette (a favourite GIF, refused in the Equicord and Vencord servers), friendcloud (the affinities' avatars drawn into affinities-cloud.png off the UI thread, waiting in the message box) and 22 kaomoji; Clyde answers where the plugin's sendBotMessage does, the Commands API's error text from Equicord otherwise; uwuEveryMessage uwuifies what you send and edit. GAP: /imgtogif always answers No Image specified! (slash commands take no file option here yet), uwuEverything is idle, /systeminfo's network reads 4g and /gettime writes the en-US form.",
    'MuteChannelMentions': "plugin_hooks/mute_channel_mentions.rs + dc-state ReadStates::hidden: the picked channels' mention badges are filtered wherever the app reads a channel's mentions (channel rows, server icons, overlay, keyboard), none at all or with onlyEveryone only your direct mentions since the last read; channel and thread menus get the Hide mention badge check after Mark As Read. Nothing is sent.",
    'AccountPanelServerProfile': "plugin_hooks/account_panel.rs: a right click on the account panel opens View Server Profile (your profile popout in the open server, disabled outside one) and the Prioritize Server Profile check; checked, a left click shows the server profile and the row reads View Account Profile.",
    'FavoriteEmojiFirst': "autocomplete.rs + plugin_hooks.rs favorite_emoji_first: the emoji autocomplete sorts every match, puts your favourite emoji (the picker's Favorites) first and cuts the list where it was. Not 1:1 yet: the alias half (see its settings).",
    'FullVCPFP': "plugin_hooks/full_vc_pfp.rs: a call tile without video draws the avatar over the whole tile (contain, unrounded, the 80px circle gone) at 1024px, the server avatar first. Not 1:1: an animated avatar stays still while its owner speaks (the original asks for the animated file then).",
    'BetterRoleDot': "plugin_hooks/better_role_dot.rs: a click on the role dot (chat, member list, and the popout's dots) copies #rrggbb with the success toast; bothStyles makes every In names / Next to names test true (names, role mentions and dots together); copyRoleColorInProfilePopout draws the popout's roles with dots.",
    'NoNitroUpsell': "plugin_hooks.rs premium_type_override, nitro.rs premium_wire: while on, the account's tier reads as Nitro (premiumTypeOverride 2) everywhere the app asks for it, so the upsells that depend on it are left out, as the original tricks the client; the server still applies the real tier.",
    'WhoReacted': "plugin_hooks/who_reacted.rs: after each reaction's count up to five avatars at 90% (four and +N past five), from GET reactions?limit=100&type=0 queued one per 250 ms newest first and merged into the users; nothing on a message with over ten reactions; clickableAvatars opens the profile popout instead of reacting. Super reactions (type 1) are not fetched: the app draws no burst pills.",
    'BetterActivities': "plugin_hooks/better_activities.rs: the member list and DM list lines (where the pinned client draws ActivityStatus) end in a row of activity icons (large else small image, mp:/spotify:/app-assets, the application icon or the Xbox mark without assets, one per picture, Twitch first with specialFirst, GIFs only with renderGifs, the default glyph when nothing else) whose tooltip is the activity card without buttons; the profile popout shows every activity as a carousel (arrows wrap, a dot per activity) or a list; removeGameActivityStatus drops the activity part and its dot. On the pinned client the plugin's regexes for removeGameActivityStatus remove the custom status instead (the first lands in a branch that never renders) and its popout patch swallows the profile widgets: the settings' meaning is ported, not those side effects. GAP: the cards are the app's visible activities; the tooltip has no spring; the arrows' tooltip colour.",
    'BetterBlockedUsers': "plugin_hooks.rs (merge_hidden_messages, skip_blocked_prompt): every blocked and every ignored message of a hidden stretch folds into its kind's first row (no split by an ignored run), and a blocked person's profile opens without Show Profile? (an ignored person's question stays). GAP: the search field and Show User Profile buttons of the settings' blocked users list, which the app does not have.",
    'BetterCommands': "plugin_hooks/plugin_commands.rs: /refresh among the built-in commands fetches the open channel's or server's command index again, with Clyde's before and after lines; autoFillArguments is idle (the app's command flow already asks every option). GAP: the user option's per-application index.",
    'BetterRoleContext': "plugin_hooks/better_role_context.rs: a right click on a member list role header opens the role menu: Edit Role (Manage Roles: Server Settings on Roles with the role open), Copy Role Color (#rrggbb), View Role Icon (role-icons/<id>/<icon>.<roleIconFileFormat> at 128 in the viewer, without sender or Forward), View Role Members (the role's member popout) and Copy Role ID, with the original's icons. GAP: the dev-context rows of a profile's role chip (the chips open no menu here).",
    'BiggerStreamPreview': "plugin_hooks/bigger_stream_preview.rs: a streaming person's user menu ends with a separator and View Stream Preview (screen icon), opening the stream preview picture at 1280x720 in the viewer (fetched first when not cached, opened on arrival). GAP: the row on a stream tile's own menu.",
    'ClipsEnhancements': "plugin_hooks/clips_enhancements.rs: Clips settings list 3-30 minute lengths, 240p/360p/2160p/4320p and 45-240 FPS beside the pinned client's choices, sorted (Source last), and the recorder keeps those rates; a clip's game is tagged with your own playing activity's application by richPresenceTagging (always, when the name's first or last word matches, never). GAP: the recorder's 384 MB video cap still bounds a long clip.",
    'FriendInvites': "plugin_hooks/plugin_commands.rs: /create friend invite, /view friend invites and /revoke friend invites among the built-in commands (POST/GET/DELETE /users/@me/invites, the pinned client's FRIEND_INVITES route), answered privately by Clyde with discord.gg links, <t:…:R> expiry and uses; a failed request says nothing, as the original's rejected promise.",
    'SpotifyShareCommands': "plugin_hooks/plugin_commands.rs: /track, /album and /artist among the built-in commands send open.spotify.com links of what your Spotify plays (album and first artist kept from the player state), after the optional message; without music Clyde says \"You're not listening to any music.\". GAP: a local file reads as no music.",
    'MessageTags': "plugin_hooks/message_tags.rs: each saved tag is a built-in slash command with an option per {{argument}} (required without a default) and ephemeral (Clyde says it to you); /tags create opens the Create New Tag dialog (name, response, detected arguments, the reserved/overwrite notice, the arguments guide), /tags list and /tags delete answer through Clyde; the plugin card shows Registered Tags with Edit, Delete and Create Tag. Tags kept under plugin-message-tags.",
    'NewGuildSettings': "plugin_hooks/new_guild_settings.rs: a server you join (its GUILD_CREATE carries your joined_at from the last two minutes: invite, Discovery or an approved join request; a lurked server has none) gets the defaults in one settings patch (muted, mobile push, @everyone/roles suppressed, highlights, events, the level unless Server default), Show All Channels in a Community server that shows picked channels, and voice names hidden; Apply NewGuildSettings in the server menu's privacy group and Apply NewGuildSettings to Folder (250 ms apart). The server header dropdown's privacy group ends with the row too (CogWheel before and after the label).",
    'OpenInApp': "plugin_hooks/open_in_app.rs: links of Spotify, Steam, Epic, Tidal, Apple Music, VRChat and Telegram open in their apps (each behind its switch) before the trust checks, with the success toast; spotify.link and s.team are resolved by HEAD requests off the UI thread (Request::ResolveRedirect, five redirects) and open in the app or the browser; a profile's Spotify or Steam connection opens the account in its app.",
    'PinDMs': "plugin_hooks/pin_dms.rs: categories at the top of the DM list (header in the category colour with a chevron, a click folds it, the open conversation stays), pinned conversations leave the plain list; pinOrder by recent message or custom; canCollapseDmSection folds the plain list from its title (dmSectionCollapsed hidden). Pin DMs submenu in the DM/user menus (Add Category, categories; Unpin DM, Move Up/Down), category menu (Edit, Move Up/Down, Delete), New/Edit Category dialog in the folder dialog's layout without the default swatch. Categories per account in storage. GAP: the friends sidebar, the quick switcher order, submenu row tints.",
    'SuperReactionTweaks': "plugin_hooks/super_reaction_tweaks.rs: superReactByDefault opens the reaction picker with its Super Reactions toggle on, for an account with real Nitro (not the plugins' override). The two playing-limit settings are idle (no super reaction effects play in the app yet).",
    'PreviewMessage': "plugin_hooks/preview_message.rs: while the message box holds text or files, an eye leads the chat bar; it shows the message as one only you see, authored by you: the text and the files with their own pictures (sendBotMessage). Nothing is sent.",
    'ServerInfo': "plugin_hooks/server_info.rs: Server Info after Edit Per-server Profile in the server menu opens the dialog: icon (or acronym), name and description, the banner on the first tab, tabs Server Info / Friends / Mutual Users / Blocked Users / Ignored Users with counts once shown; the info grid (owner, created, joined, vanity, locale, verification, boosts, channels, roles); relationship lists of members (missing ones asked for once by GUILD_MEMBERS_REQUEST) sorted by sorting; mutual users with up to three server icons and +n; rows open the profile, icon and banner the viewer. The server header dropdown's privacy group ends with the row too (InfoIcon).",
    'GreetStickerPicker': "plugin_hooks/greet_sticker_picker.rs: a right click on Wave to say hi! opens Greet Mode (radios Greet / Message) and Greet Stickers (Wumpus, Nelly, Clyde, Choco, Doggo, Peach); a sticker goes out as a mentioning reply by /greet in Greet mode or as an ordinary sticker message in Message mode. The app's menus gained group labels and radio rows for it. GAP: Unholy Multi-Greet (private settings without UI).",
    'ImageZoom': "plugin_hooks/image_zoom.rs: holding the left button on a picture in the viewer shows the lens (size, round or square, grey border and glow) magnifying zoom times under the pointer, in place of the viewer's own zoom; the wheel changes zoom and Shift+wheel the size (zoomSpeed, invertScroll, kept with saveZoomValues). GAP: nearestNeighbour (idle), the lens of an animated picture, the image menu's rows.",
    'IgnoreActivities': "plugin_hooks/ignore_activities.rs: your own activity list filtered before it goes out (op 3) and before your own status shows it: ignore* by kind (custom status untouched), idsList blacklisting or whitelisting application ids by listMode, and an Enable Activity check in a registered game's overflow menu that keeps that game off by id or exe path (re-checking turns detection back on). GAP: the eye toggle in the app launcher; start's pruning of games never seen.",
    'ViewIcons': "plugin_hooks/view_icons.rs: View Avatar / View Server Avatar / View Avatar Decoration as a group before the user menu's last one, View Icon / View Banner in the server menu, View Icon in a group DM's menu, each opening the picture viewer (no sender or Forward) at imgSize in format (animated stays webp, client assets png), Open in Browser and Save at size=4096. GAP: the clickable profile avatars, banners and DM header icons.",
    'ValidUser': "plugin_hooks/valid_user.rs: the pointer on an @unknown-user mention queues GET /users/<id> (newest first, 300 ms apart, never twice) and merges the user, so the mention shows the name. GAP: the badges the original deduces for a profile it cannot fetch.",
    'RoleColorEverywhere': "plugin_hooks/role_color_everywhere.rs: the top role colour (gradient in ENHANCED_ROLE_COLORS servers) on user mentions (with the role-mention tint behind), member list role headers, voice channel names, the reactions dialog and the poll voters dialog, each behind its setting; colorChatMessages mixes it into the text in OKLab at messageSaturation, not on a failed send. GAP: the message box has no mention pills; --text-strong/--text-muted inside a message; CustomUserColors.",
    'TypingTweaks': "plugin_hooks/typing_tweaks.rs: in the typing line every bold name is the person, with a 16px avatar (showAvatars), the role colour (showRoleColors) and a click to the full profile; over three people the alternative line (alternativeFormatting, with the original's count); amITyping keeps you in the list; blocked and ignored people are left out as useTypingUsers does.",
    'UserMessagesPronouns': "plugin_hooks/message_pronouns.rs: after a message's time, '• pronouns' in the time's style with a Pronouns tooltip, server profile pronouns first, from profiles already loaded (nothing fetched), lowercased unless Capitalized; not for bots, system or AutoMod messages, nor yourself without showSelf. Not 1:1: its compact-mode place is not drawn, the app has no compact message display.",
    'DownloadAllAttachments': "plugin_hooks/download_all.rs: a hover bar button on messages with attachments (before QuickMention's, in plugin order); over 1 GiB a critical toast, else every attachment from its proxy URL (and with downloadAllFileTypes its URL after it), four at a time, saved into Downloads as the one file or a stored ZIP attachments-<id>.zip with unique names, then the original's success or failure toast. The app has no save dialog, so the file goes where its other downloads go.",
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
    'LimitlessScreenshare': "`presets_follow_the_original_defaults_and_rules`, `sliders_and_the_throttle`, dc-media `unlocked_settings_take_any_height_and_rate_the_encoder_does` (900p 90 from a 1080p display is 1600x900 at 90 with the high bitrate and validates; 720p 24; no upscale; no rate past 240); replay by use (C:/Projects/gravecord-plugins-verify/ls1..ls12, data lsdata): the pinned picker with the plugin off (ls1/a_picker), the presets unlocked to 1440p 60 on (ls2/b_options), Frame Rate's slider and presets (ls3/c_custom), presets added in the card (ls5/b_added: 90fps; ls10/a_typed, b_added: 900p, sorted before 1080p), the add box's error (ls5/a_error), Round Resolution and the presets back after restarts (ls6/a_fps_menu, ls8/a_res_menu, ls10), the slider dragged to 97 FPS (ls7/a_fps_drag, prefs Custom/P720/97), the 900p preset chosen (ls11 prefs Custom(900)) and the resolution slider in 10p steps at 1020p (ls11/b_res_slid), the running stream's Stream Quality menu with both sliders and the presets (ls12/c_quality). Not by use: a stream actually restarted at an unlocked choice (a capture run cannot share a real screen), the Stream Quality slider's cooldown, a single click on the picker's slider (the harness's click lands without a frame between hover and press; a drag moves it), Enter in the add box (the plus button adds). Whether Discord's servers accept a resolution or rate off the official table is not known.",
    'GifMaker': "`sizes_fit_the_maximum_and_are_clamped`, `links_and_bytes_are_told_apart`, `a_video_is_sampled_thirty_times_a_second_up_to_two_hundred`, `a_still_becomes_a_one_frame_gif_with_its_caption_band`, `the_bubble_covers_the_top_and_frames_keep_their_delays`; replay by use (C:/Projects/gravecord-plugins-verify/gm1..gm9, data gmdata) with an injected message carrying the recorded picture: no row off (gm1/a_menu_off), Make GIF at the menu's end on (gm2/a_menu_end), the dialog at the picture's 480x300 (gm2/b_dialog), Caption's controls (gm3/c_caption), the bubble with its tip where the preview was clicked (gm3/e_tip), the typed caption in the GIF (gm5/b_text), Send puts export.gif in the message box (gm5/c_sent), the mode and text stored and back after restarts (gm4, gm6 `plugin state`, gm6/c_video_caption), the recorded GIFV MP4 through Image Actions (gm6/a_image_menu) made into a GIF (gm6/b_video) with its caption (gm6/c_video_caption), the MP4's own 498x270 read for the start size (gm7/a_video_fitted), Edit GIF in a picker favourite's menu (gm8/b_menu) opens the dialog on that GIF (gm8/c_edit; the picker now closes for it, gm9). Export's file is not seen by use (a capture run writes nothing into Downloads).",
    'BetterFolders': "`the_folder_icon_follows_its_setting`, `selecting_a_server_follows_the_flux_rules`; replay by use (C:/Projects/gravecord-plugins-verify/bf1, bf2, data bfdata) with a folder of three servers made by `rail folder`: the pinned open folder with the plugin off (bf1/a_off), the column with the folder's icon and strip and the rail's bare header on (bf1/b_on), Keep Icons (bf1/c_keep), Show Folder Icon Never (bf1/d_noicon); after a restart the settings back (bf2 `plugin state`, bf2/a_restored), Close Others: opening the second folder in the rail closed the first (bf2/b_close_others, expanded [778]), Close All Folders: a server outside folders closed them and the column left (bf2/c_close_all), Close All Home Button (bf2/d_home). Force Open and Close Server Folder by their tests only (a server of a closed folder cannot be reached by a click there).",
    'Experiments': "plugins: `the_table_holds_both_systems_and_their_options`, `overridden_and_newer_come_first`, `search_ranks_by_matched_words`, `overrides_round_trip_and_summaries_count`, hooks: `an_override_is_kept_and_cleared`, dc-state: `overrides_win_and_outlive_a_new_ready`; replay by use (C:/Projects/gravecord-plugins-verify/exp1..exp4): off there is no page (exp1/b_no_page_off), on the Staff Only page with the warning (exp1/c_page), the search keeps 2026-02-friends-list and its row opens (exp2/b_row_open), Variant 1 puts the Friends button in the title bar (exp3/b_titlebar_on), stored and kept after a restart (exp4/a_after_restart), gone with the plugin off (exp4/b_plugin_off), the overridden row first and open with Clear Override (exp4/c_overridden_first); a real assignment reads \"Currently assigned to variant 1\" (exp5) and a READY guild experiment \"Current Assignments: 2 guilds in bucket -1\" (exp6); the guild override 2026-04_voice_user_duration Treatment 1 opens the voice user's hover popout (exp8/a_popout_override, kept after a restart), gone with the plugin off (exp8/b_popout_plugin_off); Clear Override empties the stored overrides and the popout stays shut (exp9/b_cleared, c_no_popout)",
    'Quoter': "`custom_emojis_become_marks`, `file_names_follow_the_preview`, `grayscale_keeps_the_luminosity`, `the_quote_fits_and_draws_with_the_cached_font`, `emoji_sources_are_the_cdn_files`; replay by use (C:/Projects/gravecord-plugins-verify/quo1..quo6) with an injected message: no Quote row off (quo1/a_menu_off), Quote at the menu's end on (quo2/a_menu_end), the dialog's picture with grayscale, fade, fitted text, the mention as @username and the author rows (quo1/c_dialog), switches turned and the watermark drawn (quo2/b_switched), all three stored and restored after a restart (quo3/a_restored, b_scrolled), the watermark typed and cut at 32 characters into the picture (quo6/a_watermark), Send puts `Never trus - ada.gif` in the message box (quo4/b_sent), Export logs the 131 KB GIF (a capture run writes nothing into Downloads, so the saved file is not seen by use)",
    'BetterGifAltText': "`a_gif_reads_the_words_of_its_name`, `other_pictures_and_written_alt_text_stay`, `at_most_twenty_words_and_three_hundred_units`, `plugin_off_keeps_the_alt_as_it_is`; replay by use (C:/Projects/gravecord-plugins-verify/bgat1, bgat2): an injected GIF attachment's viewer View Details shows no Description row off (a_details_off) and \"GIF - happy cat dance\" on (b_details_on), still on after a restart (bgat2/a_details_after_restart); the accessible names are not seen by use (the harness reads no accessibility tree)",
    'BetterAudioPlayer': "`the_analyser_reads_silence_as_the_node_does`, `a_tone_lights_its_bin_and_the_wave_swings`, `the_scope_spans_the_box_and_takes_the_last_points_colour`, `bars_stop_at_the_right_edge_and_scale_to_the_loudest`, `colors_read_as_rgb_or_hex_with_the_default_otherwise`; replay by use (C:/Projects/gravecord-plugins-verify/bap1, bap2) with an injected sweep.wav attachment: off playing nothing (b_off_playing), on playing scope and bars over the card (c_on_playing, d_on_later), paused keeps its last frame (e_paused), spectrographSolidColor and oscilloscopeColor applied (f_colors), both stored and kept after a restart (bap2/a_after_restart)",
    'BetterPlusReacts': '`pluses_count_the_messages_back`, `the_list_opens_for_several_pluses`; replay by use (C:/Projects/gravecord-plugins-verify/sed1, sed2): `++:thu` opens Add reaction (a_plus_list) and Enter reacts (React refused by inert), plugin off `++:thu` opens nothing (sed2/a_plus_off); enabled state survives a restart',
    'FullSearchContext': "`copy_author_id_follows_copy_message_id`; replay by use (C:/Projects/gravecord-plugins-verify/fsc2, fsc3): a result from the voice channel's chat opens the short menu off (a_short_menu) and the full menu on (b_full_menu); Copy Author ID copies 18 bytes, Copy Message Link the result's channel (88 bytes), Reply opens that channel (c_after_reply); enabled state survives a restart",
    'SedEnhanced': '`the_wider_pattern_matches_as_the_original`, `text_and_expressions_follow_the_original`, base `the_sed_rule_follows_the_pinned_pattern`, `text_replacement_is_javascripts`, `groups_expand_by_number_and_name`; replay by use (C:/Projects/gravecord-plugins-verify/sr1, sed1, sed2): off `s|world|there|g` is sent as text (sr1/b_pipe_sent_off), on it and `s/w(or)ld/[$1]/` (regexByDefault true) edit with nothing sent, `s/(/x/` sends nothing; regexByDefault stored and kept after a restart',
    'ShowHiddenThings': '`every_switch_is_on_by_default_and_off_with_the_plugin`; replay by use (C:/Projects/gravecord-plugins-verify/sht1..sht4) with the account made a non-moderator by GUILD_UPDATE: timeout clock off/on (a_chat_off, c_chat_on), rail Invites paused line off/on (b_rail_off, d_rail_on), user menu Open in Mod View with showModView off/on (sht4); showModView and showInvitesPaused stored false and kept after a restart',
    'ValidReply': '`the_first_message_of_the_answer_decides`; replay by use (C:/Projects/gravecord-plugins-verify/vr1..vr3): two not-loaded bars (vr1/a_bars); after a restart with the plugin kept on, hovering loads the real message and the unknown id reads deleted (vr2/b_both_hovered); plugin off nothing changes (vr3/c_off_hovered)',
    'AltKrispSwitch': '`the_switch_turns_to_none_only_while_on`; replay by use: the popout switch off stores None (2) and keeps it after a restart, plugin off stores Standard (1)',
    'ClipUpload': '`names_read_as_the_original_reads_them`, `metadata_is_read_after_the_footer`, `dates_round_trip_through_the_field`, `bodies_carry_the_clip_fields`, `a_picked_file_is_stamped_in_a_copy`; frames pending (live runs held)',
    'CustomRPC': '`activities_read_as_create_activity_builds_them`, `checks_read_as_the_form_writes_them`, `fields_check_as_the_form_checks`; frames pending (live runs held)',
    'ExpressionCloner': '`stickers_follow_the_originals_tables`, dc-proto `an_entry_leaves_its_map_and_the_rest_stays`; frames pending (live runs held)',
    'BetterForwards': '`resends_read_as_the_original_writes_them`, `embeds_split_as_the_original_splits_them`; frames pending (live runs held)',
    'MessageLogger': '`diffs_follow_create_word_diff`, `ignore_rules_follow_should_ignore`, `dropped_attachments_stay_in_place`; frames pending (live runs held)',
    'PermissionsViewer': '`every_bit_has_a_title_and_the_order_is_the_originals`, `entries_and_grants_follow_the_original`; frames pending (live runs held)',
    'Translate+': '`languages_are_found_as_the_original_finds_them`, `dictionaries_apply_as_in_the_original`, `nothing_is_offered_while_off`; frames pending (live runs held)',
    'SteamStatusSync': '`statuses_map_as_the_original`',
    'FixYoutubeEmbeds': '`long_descriptions_fold_to_twenty_characters`; frames pending (live runs held)',
    'TenorGifSearch': '`the_provider_and_badges_follow_the_original`, `answers_take_discord_shapes`; frames pending (live runs held)',
    'SpotifyCrack': '`the_defaults_follow_the_original`',
    'SearchFix': '`past_5000_the_order_flips_from_the_start`',
    'GlobalBadges': '`badges_follow_the_switches_and_blocked_mods`; frames pending (live runs held)',
    'UserPFP': '`userpfp_pictures_are_asked_for_still`; frames pending (live runs held)',
    'USRBG': '`banners_follow_the_list_and_nitro_first`; frames pending (live runs held)',
    'StreamingCodecDisabler': '`only_the_switch_takes_h264_out`',
    'VoiceMessages': '`the_waveform_follows_generate_waveform`; frames pending (live runs held)',
    'UnlockedAvatarZoom': '`the_slider_reaches_the_multiplier`; frames pending (live runs held)',
    'BetterQuickReact': '`the_grid_scrolls_a_row_at_a_time`; frames pending (live runs held)',
    'BypassStatus': '`ids_and_mentions_follow_the_original`; frames pending (live runs held)',
    'RelationshipNotifier': '`texts_and_names_follow_the_original`, `the_timeout_waits_while_hovered`; frames pending (live runs held)',
    'ZipPreview': '`an_archive_reads_as_unzip_sync_does`, `helpers_follow_the_original`; frames pending (live runs held)',
    'SilentMessageToggle': '`the_prefix_and_settings_follow_the_original`, `auto_disable_takes_the_toggle_once`; frames pending (live runs held)',
    'MoreCommands': '`the_text_commands_follow_the_original`, `uwuify_replaces_words_and_keeps_links`, `transform_applies_case_form_reversal_and_repeats`, `the_cloud_follows_the_original_numbers`, `every_command_has_a_distinct_name`; frames pending (live runs held)',
    'MuteChannelMentions': '`ids_parse_as_the_original_splits_them`, dc-state `hidden_channels_show_nothing_or_only_direct_mentions_since_the_ack`; frame mcm-menu; plugins-off channel-menu, chat',
    'AccountPanelServerProfile': '`the_menu_names_the_other_popout_and_shows_the_check`; frames apsp-menu/apsp-profile; plugins-off account-panel',
    'FavoriteEmojiFirst': 'frames fef-on/fef-off/fef-top (:smi with smiling_imp a favourite, demo-favorite); plugins-off emoji-autocomplete',
    'FullVCPFP': '`the_server_avatar_comes_first_at_1024`; frame fvp-on (callsim); plugins-off voice-profile, user-volume',
    'BetterRoleDot': '`colours_copy_as_color_strings_and_switches_read`; frame brd-both; plugins-off chat, member-list, profile-popout',
    'NoNitroUpsell': '`no_nitro_upsell_reads_as_nitro_only_while_on`; frames nnu-on/nnu-off (Appearance: Color Themes as Nitro); plugins-off gif-picker, settings-appearance',
    'WhoReacted': '`five_at_most_with_the_rest_counted`; frame wr-on (demo-reactors); plugins-off reactions, chat',
    'BetterActivities': '`the_settings_follow_the_original_defaults`, `pictures_follow_get_application_icons`, `without_assets_the_application_icon_or_xbox_stands_in`, `twitch_goes_first_or_at_its_place`; frames pending (fixture token answers 401)',
    'BetterBlockedUsers': 'plugins-off chat, dm-list (the hooks return false off); frames pending (fixture token answers 401)',
    'BetterCommands': 'command table test (`every_plugin_with_commands_is_defined`); frames pending (fixture token answers 401)',
    'BetterRoleContext': '`rows_follow_what_the_role_offers_in_the_original_order`, `colors_copy_as_css_hex_and_the_format_defaults_to_png`; frames brc-menu, brc-members, brc-icon, brc-edit (synthetic hoisted role); plugins-off role-header',
    'BiggerStreamPreview': 'frame bsp-menu (callsim peer live, row at the end of the voice row menu); plugins-off user-volume, voice-profile',
    'ClipsEnhancements': '`tagging_matches_by_the_name_s_ends`, `labels_follow_the_messages`; frame ce-length (with NoNitroUpsell: the Clips page needs Nitro in a capture run, so plugins-off has no Clips scenario; the plugin-off lists are the clips label tests)',
    'FriendInvites': '`answers_read_as_the_original_writes_them`, `every_plugin_with_commands_is_defined`; frame ssc-commands (the three commands among Built-In); plugins-off slash-list',
    'SpotifyShareCommands': '`answers_read_as_the_original_writes_them`; frames ssc-commands, ssc-none (Clyde: not listening); plugins-off slash-list',
    'MessageTags': '`arguments_and_responses_follow_the_original`; frames mt-dialog (/tags create, arguments detected), mt-run (/greet with user:World sends Hello World! I am feeling great.)',
    'NewGuildSettings': '`defaults_follow_the_switches`, `only_a_recent_join_counts`; frame ngs-menu; plugins-off guild-menu',
    'OpenInApp': '`links_become_the_apps_addresses`, `every_switch_defaults_on`; plugins-off send-link, masked-link',
    'PinDMs': '`categories_round_trip_and_order_by_recency_or_as_kept`, `the_pin_menu_offers_categories_or_unpin_and_moves`, `swaps_stay_inside_the_list_and_settings_default_off`; frames pd-menu, pd-sub, pd-dialog, pd-list, pd-folded, pd-catmenu; plugins-off dm-list, user-menu',
    'SuperReactionTweaks': '`super_reactions_start_on_only_with_the_switch`; plugins-off reactions',
    'PreviewMessage': '`the_preview_is_yours_ephemeral_and_carries_the_files`; frames pm (the eye leads the bar), pm-shown (Only you can see this)',
    'ServerInfo': '`acronyms_and_levels_follow_the_original`, `sorting_defaults_to_display_names`; frames si-menu, si-info, si-friends, si-mutual; plugins-off guild-menu',
    'GreetStickerPicker': '`the_menu_has_the_mode_and_the_six_stickers`; menu unit test; frame gsp-menu (synthetic join message, right click on Wave to say hi!)',
    'ImageZoom': '`the_wheel_follows_the_original`; frames pending (fixture token answers 401)',
    'IgnoreActivities': '`kinds_are_ignored_by_their_switch_and_custom_status_never`, `the_ids_list_blacklists_or_whitelists`, `an_ignored_game_goes_by_its_id_or_its_exe`; no frame (the capture profile has no registered game); plugins-off: the filter sits on the presence path, which a capture never sends',
    'ViewIcons': '`addresses_follow_open_image`, `settings_default_to_webp_at_1024`, `the_group_goes_before_the_last_one`; frames vi-menu (View Avatar group before Open in Mod View), vi-guild (no icon, no rows); plugins-off user-menu, channel-menu',
    'ValidUser': '`keys_carry_the_user`; frame vu-after (hover logs the GET, `plugin demo-user` answer shows @Discord); plugins-off mentions',
    'RoleColorEverywhere': '`each_place_has_its_switch_and_messages_need_theirs`, `oklab_mix_matches_the_css_ends_and_middle`; frames msg-on/msg-off (mention and message colour), brc-menu (role header); plugins-off mentions, role-header',
    'TypingTweaks': '`bold_runs_become_people_and_many_use_the_alternative_line`; frames tt-on/tt-off; plugins-off typing',
    'UserMessagesPronouns': '`server_pronouns_first_then_the_accounts_formatted`; frame ump-on; plugins-off chat',
    'DownloadAllAttachments': '`names_are_made_unique_as_the_original_does`, `the_archive_holds_each_file_stored_with_its_crc`, `sources_size_check_and_toasts_follow_the_original`, `download_all_saves_what_answered_as_a_zip_or_one_file` (local server); frame daa-on; plugins-off attachment-hover, hover-bar',
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
SKIPPED_SETTINGS = {
    'LimitlessScreenshare': {
        'fpsComponent': "ported as the plugin card's own section (Frame rate presets: chips with Delete and the add box), not a setting.",
        'resolutionComponent': "ported as the plugin card's own section (Resolution presets), not a setting.",
        'resolutions': "kept apart from the settings (storage key plugin-limitless-screenshare, the original's {label, value} list), written by the card's section.",
        'fpss': "kept apart from the settings (storage key plugin-limitless-screenshare), written by the card's section.",
    },
    'GifMaker': {
        'lastFontFamily': "not ported: the font list is Google Fonts' metadata and font files, fetched from fonts.google.com at start; plugin code sends no requests and the app has none of them, so the caption is set in the app's own gg sans.",
    },
    'Experiments': {
        'toolbarDevMenu': "not ported: it swaps the chat header's Help button for Discord's developer menu (`staff-help-popout`), whose rows open the Developer Options page and overrides that are not ported (GAP).",
    },
    'Quoter': {
        'quoteFont': "not ported: its five choices are Google fonts the original loads from fonts.googleapis.com at start; plugin code sends no requests and the app has none of them, so the quote is set in the app's own gg sans.",
    },
    'FixYoutubeEmbeds': {
        'fixBlockedEmbeds': "not ported yet: it rewrites YouTube's embedded player page (recoverEmbed.js) when a label blocks the embed; here that page is the provider player's system web view (provider_player.rs), which has no way to run a page script yet (dc_webhost). Planned with that call.",
        'blockAds': "not ported yet: it filters the ads of YouTube's embedded player and WatchTogether (playerFilter.js); the player here is the provider player's system web view (provider_player.rs), which has no way to run a page script yet (dc_webhost). Planned with that call.",
    },
    'CustomRPC': {
        'config': "ported as the plugin card's own section: the RPC form (notices, presets, every field), not a setting.",
    },
    'GlobalBadges': {
        'apiUrlPermission': "not ported: the desktop client's content security policy notice (Allow Host); this client reaches any host.",
        'showClientMods': 'ported as the sixteen Show switches it edits, listed one by one instead of one multi-select.',
    },
    'BetterActivities': {
        'divider': 'a 1px line between the member list and the popout settings in the plugin card (a COMPONENT, no value); the card here lists the settings without it.',
    },
    'MessageTags': {
        'tagsList': 'kept apart from the settings (storage key plugin-message-tags), written by the tag dialog and /tags delete.',
        'tagComponent': 'ported as the plugin card\'s own section (Registered Tags with Edit, Delete, Create Tag), not as a setting.',
    },
    'ClipsEnhancements': {
        'clipsLink': 'ported as the plugin card\'s own section: the button that opens Clips settings, not a setting.',
    },
    'PinDMs': {
        'userBasedCategoryList': 'kept apart from the settings (storage key plugin-pin-dms, per account), written by the Pin DMs menus and the category dialog.',
    },
    'IgnoreActivities': {
        'importCustomRPC': "not applicable yet: it copies CustomRPC's application id, and CustomRPC is not ported.",
        'ignoredActivities': 'kept apart from the settings (storage key plugin-ignore-activities), written by the Enable Activity check of a registered game.',
    },
    'FavoriteEmojiFirst': {
        'aliases': 'not ported yet: the emoji alias editor (Set Alias in the emoji menus, this list, `:alias:` written '
                   'out on send and offered in the autocomplete) is the next step of this port.',
        'clearAll': 'not ported yet: comes with the alias editor (see aliases).',
    },
}

# Already what gravecord does without a plugin.
NATIVE = {
    'PermissionFreeWill': "the channel permission editor here has neither the pinned client's lockout guard (\"Pretty sure "
                          "you don't want to do this\", STAGE_CHANNEL_CANNOT_OVERWRITE_PERMISSION) nor its onboarding "
                          "threshold warning yet (GAP: main app), so both edits already go through; when the app adds "
                          "them, lockout and onboarding need their hooks there.",
    'LimitMiddleClickPaste': "a middle click in the app neither pastes (no primary-selection paste in its text boxes) nor "
                             "opens a link or a picture (a middle press starts the pinned client's autoscroll instead), "
                             "which is what pasteScope and openScope ask for; the pinned client's own middle-click link "
                             "opening is not ported yet (GAP: main app), and when it is, openScope needs its hook there.",
    'IloveSpam': "the app does not hide the messages of users flagged as likely spammers (the user flag 1<<20 that the "
                 "pinned client's message list checks) yet (GAP: main app), so they already show as any other; when the "
                 "app adds the rule, this plugin needs its hook there.",
    'StopAutoUnread': 'the app never raises a channel to All Messages by itself: the pinned client\'s '
                      '`maybeAutoUpgradeChannel` (called by the unreads notice hook in module 626977) is not ported '
                      '(GAP: main app), so there is nothing to stop.',
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
    'Decor': 'its third-party avatar decorations service (decor-auth in the backup\'s data store is not copied).',
    'Streaks': 'DM streak counts (its service login in the data store is not copied).',
    'VoiceMessageTranscriber': 'an on-device Whisper model runner.',
}

errors = []
rows, setting_rows = [], []
counts = {'ported': 0, 'native': 0, 'not applicable': 0, 'planned': 0}
# Every other plugin of the checkout, with its verdict from tools/plugin-verdicts.tsv (name, port|native|n/a,
# usefulness, text) until it is built.
VERDICTS = {}
for line in open(os.path.join(REPO, 'tools/plugin-verdicts.tsv'), encoding='utf-8'):
    if line.startswith('#') or not line.strip():
        continue
    name, verdict, useful, text = line.rstrip('\n').split('\t')
    VERDICTS[name] = (verdict, useful, text)
covered = set(enabled) | {RENAMED.get(name, name) for name in enabled}
extra = sorted((name for name in sources if name not in covered), key=str.lower)
for name in extra:
    if name not in VERDICTS and name not in ours:
        errors.append(f'{name} is in the checkout but has no verdict in tools/plugin-verdicts.tsv')
for name in VERDICTS:
    if name not in sources:
        errors.append(f'{name} has a verdict but is not in the checkout')
in_backup = set(enabled)

for name in enabled + extra:
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
                why = SKIPPED_SETTINGS[name][key]
                state = why if why.startswith('ported as') else 'not ported: ' + why
            else:
                errors.append(f'{name}.{key} of the original is missing and has no reason')
                state = 'MISSING'
            setting_rows.append(f'| {name} | `{key}` | {kind} | `{default}` | {state} |')
        for key in sorted(here - {key for key, _, _ in original}):
            errors.append(f'{name}.{key} is not a setting of the original')
    elif name in NATIVE:
        status, note, test = 'native', NATIVE[name], ''
    elif name not in in_backup:
        verdict, useful, text = VERDICTS.get(name, ('port', '-', ''))
        status, test = {'port': 'planned', 'native': 'native', 'n/a': 'not applicable'}[verdict], ''
        note = text if verdict != 'port' else f'usefulness {useful}/5. {text}'
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
    out.write('Built-in equivalents of the Vencord/Equicord plugins the owner keeps on.\n')
    out.write('Framework: `app/src/plugins.rs` (registry, state, Backup & Restore) and the Gravecord settings\n')
    out.write('group (`app/src/ui/settings/mods.rs`). Written by\n')
    out.write('`tools/gen-plugins-md.py`, which fails on a missing setting or a plugin without a reason.\n\n')
    out.write('Plugins off is the pinned client: `cargo xtask plugins-off` captures every scenario of\n')
    out.write('`tests/plugins-off/scenarios.txt` with the plugin code skipped, with the master switch off (every plugin on and\n')
    out.write('every setting changed), and with every plugin off, and fails on one differing pixel or on any plugin effect.\n\n')
    out.write(f'The list is every plugin of the local Equicord checkout ({len(enabled) + len(extra)}): first the {len(enabled)} enabled in\n')
    out.write(f'the owner\'s Equicord backup of 2026-09-21, then the other {len(extra)}, whose verdicts are in\n')
    out.write('`tools/plugin-verdicts.tsv` (`src/plugins` is Vencord\'s, `src/equicordplugins` Equicord\'s).\n\n')
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
