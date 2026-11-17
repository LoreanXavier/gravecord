"""Writes PLUGINS.md: the status of every plugin an Equicord backup enables.

Usage: python tools/gen-plugins-md.py <equicord-backup.json> <Equicord checkout> PLUGINS.md

Reads the backup only for plugin names and enabled flags (never setting values or the data store), and the Equicord
checkout for each plugin's source path and description. The tables below are kept by hand as plugins are ported.
"""
import json, os, re, sys

BACKUP, CHECKOUT, OUT = sys.argv[1:4]
ROOT = os.path.join(CHECKOUT, 'src')

backup = json.load(open(BACKUP, encoding='utf-8'))
enabled = sorted((k for k, v in backup['settings']['plugins'].items() if v.get('enabled')), key=str.lower)

sources = {}
for dp, dn, fn in os.walk(ROOT):
    for f in fn:
        if not f.endswith(('.ts', '.tsx')):
            continue
        path = os.path.join(dp, f)
        text = open(path, encoding='utf-8', errors='replace').read()
        for m in re.finditer(r'definePlugin\(\{\s*name:\s*"([^"]+)"', text):
            rel = os.path.relpath(path, ROOT).replace(os.sep, '/')
            desc = re.search(r'description:\s*"([^"]*)"', text[m.end():m.end() + 1500])
            sources.setdefault(m.group(1), (rel, desc.group(1) if desc else ''))

PORTED = {
    'AlwaysTrust': 'links.rs: masked links open without "Leaving Discord" (`domain`).',
    'ClearURLs': 'plugins/clear_urls.rs: ClearURLs rules bundled; send and edit.',
    'MessageClickActions': 'plugin_hooks.rs: double-click edit/reply/copy (`doubleClickAction`, `doubleClickOthersAction`, '
                           '`doubleClickModifier`, `disableInDms`). On by default.',
    'NoTypingAnimation': 'typing_avatar.rs: still dots, no animation frames.',
    'UnlimitedAccounts': 'account_switcher.rs: `maxAccounts` (0 = no limit).',
    'NoPendingCount': 'friend request count 0 (`hideFriendRequestsCount`), no Message Requests badge (`hideMessageRequestsCount`); '
                      'no Nitro offer badge exists here.',
    'RevealAllSpoilers': 'Ctrl-click opens every text spoiler of the message, Ctrl+Shift-click of the channel.',
    'DisableCallIdle': 'dm_call.rs: no leave after 3 minutes alone in a DM call.',
    'GifPaste': 'gifs.rs: picking a GIF inserts its link and a space in the composer.',
    'VolumeBooster': 'User Volume slider to 200% x `multiplier` (context menu, call tile menu); dc-voice gain cap raised.',
}
TESTS = {
    'NoPendingCount': 'frames pending-off/pending-on',
    'RevealAllSpoilers': '`ui::plugin_hooks::tests`; frames spoilers-off/spoilers-on',
    'DisableCallIdle': 'frames idle-off/idle-on (195 s alone in a simulated DM call)',
    'GifPaste': 'frames gifpaste-off/gifpaste-on',
    'AlwaysTrust': 'frames trust-off/trust-on',
    'ClearURLs': '`plugins::clear_urls::tests` (6); frames clearurls-off/on',
    'MessageClickActions': '`ui::plugin_hooks::tests` (3); frames mca-off/on',
    'NoTypingAnimation': 'none yet (headless frame not taken)',
    'UnlimitedAccounts': 'frames ua-off/ua-on (5 vs 7 listed)',
    'VolumeBooster': 'frames vb-off/vb-on (100% at 1/2 vs 1/4 of the track)',
}

# Already what gravecord does without a plugin.
NATIVE = {
    'NoTrack': 'gravecord sends no analytics, metrics or Sentry reports; its only `/science` post is the overlay problem report the user submits.',
    'ForceOwnerCrown': 'the member list shows the owner crown at any server size.',
    'NormalizeMessageLinks': 'Copy Message Link already writes discord.com links.',
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
    'Settings': 'its settings UI is the Gravecord settings group itself; `settingsLocation` not ported (the group sits before Developer).',
    'YoutubeAdblock': 'needs an ad filter inside the YouTube web view of embeds; deferred with the web host.',
    'VCSupport': 'Equicord support-server helper.',
    'NoDevtoolsWarning': 'no web console in a native client.',
    'CrashHandler': 'recovers React render crashes; there is no React here.',
    'ReactErrorDecoder': 'decodes minified React errors; there is no React here.',
    'ThemeAttributes': 'adds DOM attributes for CSS themes; there is no DOM or CSS here.',
    'EquicordHelper': 'workarounds for Discord web crashes in the mod.',
    'SupportHelper': 'support-channel helper of the mod.',
}
PLANNED_NOTES = {
    'ColorSighted': 'status masks in member_avatars.rs.',
    'OnePingPerDM': 'notification sound per DM.',
    'SilentMessageToggle': 'composer button, @silent flag.',
    'CallTimer': 'timer in the voice panel.',
    'MemberCount': 'member list header counts.',
    'TypingTweaks': 'typing line avatars and role colours.',
    'TypingIndicator': 'typing dots in the channel list.',
    'PinDMs': 'pinned DM categories.',
    'BetterFolders': 'folder sidebar.',
    'MessageLogger': 'deleted/edited message log (needs a store of deletes).',
    'PermissionsViewer': 'permission breakdown in profiles.',
    'PlatformIndicators': 'client status icons.',
    'ShowConnections': 'connections in profiles.',
    'RoleColorEverywhere': 'role colours in mentions, lists, voice.',
    'WhoReacted': 'reactor avatars on reactions.',
    'ViewIcons': 'open avatars/icons full size.',
    'ImageZoom': 'lens on image viewer.',
    'BetterUploadButton': 'left click uploads, right click menu.',
    'SedEnhanced': 's/a/b/ edits your last message.',
    'ValidReply': 'fetch missing replied messages.',
    'ValidUser': 'fetch unknown mentioned users.',
    'NoUnblockToJump': 'jump to blocked users\' messages.',
    'CustomRPC': 'custom rich presence (needs the RPC/activity editor).',
    'LimitlessScreenshare': 'resolution/fps sliders past the tier limits (owner: later, FakeNitro-style).',
    'StreamingCodecDisabler': 'codec choice in dc-media.',
    'IgnoreActivities': 'hide listed activities.',
    'NewGuildSettings': 'notification defaults on join.',
    'SpotifyCrack': 'no auto-pause of Spotify on voice.',
    'VoiceDownload': 'download button on voice messages.',
    'VoiceMessages': 'record voice messages on desktop.',
    'Translate+': 'message translation (needs a translation service).',
    'VoiceMessageTranscriber': 'on-device Whisper (needs a model runner).',
    'Decor': 'third-party avatar decorations service.',
    'USRBG': 'third-party banners service.',
    'UserPFP': 'third-party avatars service.',
    'GlobalBadges': 'third-party badge service.',
}

rows = []
counts = {'ported': 0, 'native': 0, 'not applicable': 0, 'planned': 0}
for name in enabled:
    src, desc = sources.get(name, ('', ''))
    if name in PORTED:
        status, note = 'ported', PORTED[name]
        test = TESTS[name]
    elif name in NATIVE:
        status, note, test = 'native', NATIVE[name], ''
    elif name in API or name in NOT_APPLICABLE:
        status = 'not applicable'
        note = NOT_APPLICABLE.get(name, 'client-mod plumbing (an API for other plugins), nothing to port.')
        test = ''
    else:
        status = 'planned'
        note = PLANNED_NOTES.get(name, '')
        test = ''
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
    out.write('`tools/gen-plugins-md.py`.\n\n')
    out.write('The list is the plugins enabled in the owner\'s Equicord backup of 2026-09-21 (156). Sources were read in the local\n')
    out.write('Equicord checkout of 2026-11-14 (`src/plugins` is Vencord\'s, `src/equicordplugins` Equicord\'s).\n\n')
    out.write(f'Totals: {counts["ported"]} ported, {counts["native"]} native, {counts["planned"]} planned, '
              f'{counts["not applicable"]} not applicable.\n\n')
    out.write('Status: **ported** works and has its proof; **native** is what gravecord already does without a plugin;\n')
    out.write('**planned** is not built yet (an import keeps its settings for when it is); **not applicable** has nothing to\n')
    out.write('port in a native client.\n\n')
    out.write('| plugin | status | source | what it does (original) | here / what it needs | proof |\n')
    out.write('| - | - | - | - | - | - |\n')
    out.write('\n'.join(rows) + '\n')
print(counts)
