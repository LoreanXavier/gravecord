//! The "High Contrast Mode" overrides (`S`, the SemanticColorHighContrast table of module 646514): when
//! `isHighContrastModeEnabled` is on the colour resolver replaces a semantic token by another raw colour, with its own
//! opacity, for the theme. Values are `0xRRGGBBAA` for Ash (`dark`), Light, Dark (`darker`) and Onyx (`midnight`), the
//! raw colour read from the generated token tables of the pinned build. Sorted by name.

pub(super) static TOKENS: &[(&str, [u32; 4])] = &[
    (
        "--ansi-blue",
        [0x66a5f3ff, 0x006fd9ff, 0x3789eaff, 0x0577e4ff],
    ),
    (
        "--ansi-bright-blue",
        [0xa0a9faff, 0x5865f2ff, 0x5865f2ff, 0x5865f2ff],
    ),
    (
        "--ansi-cyan",
        [0x41b0baff, 0x007c87ff, 0x0096a1ff, 0x008692ff],
    ),
    (
        "--ansi-green",
        [0x5bb277ff, 0x008043ff, 0x399b5dff, 0x138a4aff],
    ),
    (
        "--ansi-magenta",
        [0xff66d8ff, 0xc1389dff, 0xe444bbff, 0xcb3ba6ff],
    ),
    (
        "--ansi-red",
        [0xf77b77ff, 0xd22d39ff, 0xe75858ff, 0xda3e44ff],
    ),
    (
        "--ansi-yellow",
        [0xe09000ff, 0xa56100ff, 0xc07600ff, 0xae6800ff],
    ),
    (
        "--app-frame-border",
        [0xd4d5d8ff, 0x4f505aff, 0xbabbc0ff, 0xa4a5abff],
    ),
    (
        "--background-code",
        [0x5865f204, 0x5865f202, 0x5865f204, 0x5865f204],
    ),
    (
        "--background-feedback-mod-critical",
        [0xd22d3904, 0xd22d3904, 0xd22d3904, 0xd22d3904],
    ),
    (
        "--background-feedback-mod-info",
        [0x0074e304, 0x0074e304, 0x0074e304, 0x0074e304],
    ),
    (
        "--background-feedback-mod-positive",
        [0x00854504, 0x00854504, 0x00854504, 0x00854504],
    ),
    (
        "--background-feedback-mod-warning",
        [0xf8a30004, 0xf8a30004, 0xf8a30004, 0xf8a30004],
    ),
    (
        "--background-mod-muted",
        [0x97979f02, 0x97979f04, 0x97979f02, 0x97979f04],
    ),
    (
        "--background-mod-normal",
        [0x97979f0a, 0x97979f0a, 0x97979f0a, 0x97979f0f],
    ),
    (
        "--background-mod-strong",
        [0x97979f0f, 0x97979f14, 0x97979f0f, 0x97979f14],
    ),
    (
        "--background-mod-subtle",
        [0x97979f04, 0x97979f07, 0x97979f04, 0x97979f07],
    ),
    (
        "--badge-secondary-background",
        [0x97979f0a, 0x97979f0a, 0x97979f0a, 0x97979f0f],
    ),
    (
        "--badge-secondary-border",
        [0x97979f0a, 0x97979f0a, 0x97979f0a, 0x97979f0f],
    ),
    (
        "--badge-secondary-text",
        [0xffffffff, 0x000000ff, 0xffffffff, 0xffffffff],
    ),
    (
        "--border-feedback-critical",
        [0xffcfcdff, 0x931b26ff, 0xffa7a3ff, 0xfb8580ff],
    ),
    (
        "--border-feedback-info",
        [0xc5deffff, 0x004a92ff, 0x97c3fcff, 0x72adf5ff],
    ),
    (
        "--border-feedback-positive",
        [0xa8eabaff, 0x00582bff, 0x84d09aff, 0x65ba7fff],
    ),
    (
        "--border-feedback-warning",
        [0xffe0b0ff, 0x7b3e00ff, 0xfebc41ff, 0xef9b00ff],
    ),
    (
        "--border-focus",
        [0xbdd9ffff, 0x00519eff, 0x8ebefaff, 0x66a5f3ff],
    ),
    (
        "--border-muted",
        [0x96979eff, 0x7a7b83ff, 0x7d7e87ff, 0x6c6d76ff],
    ),
    (
        "--border-normal",
        [0xefeff1ff, 0x41424aff, 0xd1d1d5ff, 0xbabbc0ff],
    ),
    (
        "--border-strong",
        [0xffffffff, 0x2e2e34ff, 0xefeff1ff, 0xd4d5d8ff],
    ),
    (
        "--border-subtle",
        [0xd4d5d8ff, 0x4f505aff, 0xbabbc0ff, 0xa4a5abff],
    ),
    (
        "--card-border-default",
        [0xd4d5d8ff, 0x4f505aff, 0xbabbc0ff, 0xa4a5abff],
    ),
    (
        "--card-secondary-bg",
        [0x97979f04, 0x97979f07, 0x97979f04, 0x97979f07],
    ),
    (
        "--channel-icon",
        [0xbabbc0ff, 0x4f505aff, 0x9d9ea5ff, 0x96979eff],
    ),
    (
        "--channel-text-area-placeholder",
        [0xa4a5abff, 0x73747dff, 0x888991ff, 0x7a7b83ff],
    ),
    (
        "--channels-default",
        [0xbabbc0ff, 0x4f505aff, 0x9d9ea5ff, 0x96979eff],
    ),
    (
        "--chat-border",
        [0xd4d5d8ff, 0x4f505aff, 0xbabbc0ff, 0xa4a5abff],
    ),
    (
        "--chat-text-muted",
        [0xc2c2c7ff, 0x555660ff, 0xa4a5abff, 0x96979eff],
    ),
    (
        "--checkbox-border-active",
        [0xd4d5d8ff, 0x4f505aff, 0xbabbc0ff, 0xa4a5abff],
    ),
    (
        "--checkbox-border-default",
        [0xffffffff, 0x2e2e34ff, 0xefeff1ff, 0xd4d5d8ff],
    ),
    (
        "--checkbox-border-hover",
        [0xd4d5d8ff, 0x4f505aff, 0xbabbc0ff, 0xa4a5abff],
    ),
    (
        "--checkbox-border-selected-default",
        [0xd4d5d8ff, 0x4f505aff, 0xbabbc0ff, 0xa4a5abff],
    ),
    (
        "--checkbox-border-selected-hover",
        [0xd4d5d8ff, 0x4f505aff, 0xbabbc0ff, 0xa4a5abff],
    ),
    (
        "--control-connected-border-active",
        [0x99e0adff, 0x006936ff, 0x7ac991ff, 0x65ba7fff],
    ),
    (
        "--control-connected-border-default",
        [0x99e0adff, 0x006936ff, 0x7ac991ff, 0x65ba7fff],
    ),
    (
        "--control-connected-border-hover",
        [0x99e0adff, 0x006936ff, 0x7ac991ff, 0x65ba7fff],
    ),
    (
        "--control-critical-primary-border-active",
        [0xffc0bdff, 0xad242fff, 0xff9d98ff, 0xfb8580ff],
    ),
    (
        "--control-critical-primary-border-default",
        [0xffc0bdff, 0xad242fff, 0xff9d98ff, 0xfb8580ff],
    ),
    (
        "--control-critical-primary-border-hover",
        [0xffc0bdff, 0xad242fff, 0xff9d98ff, 0xfb8580ff],
    ),
    (
        "--control-critical-secondary-background-active",
        [0x97979f0f, 0x97979f14, 0x97979f0f, 0x97979f14],
    ),
    (
        "--control-critical-secondary-background-hover",
        [0x97979f0a, 0x97979f0a, 0x97979f0a, 0x97979f0f],
    ),
    (
        "--control-critical-secondary-border-active",
        [0xffc0bdff, 0xad242fff, 0xff9d98ff, 0xfb8580ff],
    ),
    (
        "--control-critical-secondary-border-default",
        [0xffc0bdff, 0xad242fff, 0xff9d98ff, 0xfb8580ff],
    ),
    (
        "--control-critical-secondary-border-hover",
        [0xffc0bdff, 0xad242fff, 0xff9d98ff, 0xfb8580ff],
    ),
    (
        "--control-critical-secondary-icon-active",
        [0xffcfcdff, 0x931b26ff, 0xffa7a3ff, 0xfb8580ff],
    ),
    (
        "--control-critical-secondary-icon-default",
        [0xffcfcdff, 0x931b26ff, 0xffa7a3ff, 0xfb8580ff],
    ),
    (
        "--control-critical-secondary-icon-hover",
        [0xffcfcdff, 0x931b26ff, 0xffa7a3ff, 0xfb8580ff],
    ),
    (
        "--control-critical-secondary-text-active",
        [0xffcfcdff, 0x931b26ff, 0xffa7a3ff, 0xfb8580ff],
    ),
    (
        "--control-critical-secondary-text-default",
        [0xffcfcdff, 0x931b26ff, 0xffa7a3ff, 0xfb8580ff],
    ),
    (
        "--control-critical-secondary-text-hover",
        [0xffcfcdff, 0x931b26ff, 0xffa7a3ff, 0xfb8580ff],
    ),
    (
        "--control-icon-only-background-active",
        [0x97979f0f, 0x97979f14, 0x97979f0f, 0x97979f14],
    ),
    (
        "--control-icon-only-background-hover",
        [0x97979f0a, 0x97979f0a, 0x97979f0a, 0x97979f0f],
    ),
    (
        "--control-icon-only-border-active",
        [0x96979eff, 0x7a7b83ff, 0x7d7e87ff, 0x6c6d76ff],
    ),
    (
        "--control-icon-only-border-hover",
        [0x96979eff, 0x7a7b83ff, 0x7d7e87ff, 0x6c6d76ff],
    ),
    (
        "--control-icon-only-icon-active",
        [0xffffffff, 0x000000ff, 0xffffffff, 0xffffffff],
    ),
    (
        "--control-icon-only-icon-default",
        [0xfbfbfbff, 0x393a41ff, 0xdcdcdfff, 0xc2c2c7ff],
    ),
    (
        "--control-icon-only-icon-hover",
        [0xffffffff, 0x000000ff, 0xffffffff, 0xffffffff],
    ),
    (
        "--control-overlay-secondary-border-active",
        [0x96979eff, 0x7a7b83ff, 0x7d7e87ff, 0x6c6d76ff],
    ),
    (
        "--control-overlay-secondary-border-default",
        [0x96979eff, 0x7a7b83ff, 0x7d7e87ff, 0x6c6d76ff],
    ),
    (
        "--control-overlay-secondary-border-hover",
        [0x96979eff, 0x7a7b83ff, 0x7d7e87ff, 0x6c6d76ff],
    ),
    (
        "--control-primary-border-active",
        [0xc0cdffff, 0x4250b6ff, 0xa3b5ffff, 0x8fa3feff],
    ),
    (
        "--control-primary-border-default",
        [0xc0cdffff, 0x4250b6ff, 0xa3b5ffff, 0x8fa3feff],
    ),
    (
        "--control-primary-border-hover",
        [0xc0cdffff, 0x4250b6ff, 0xa3b5ffff, 0x8fa3feff],
    ),
    (
        "--control-secondary-background-active",
        [0x97979f0f, 0x97979f14, 0x97979f0f, 0x97979f14],
    ),
    (
        "--control-secondary-background-default",
        [0x97979f04, 0x97979f07, 0x97979f04, 0x97979f07],
    ),
    (
        "--control-secondary-background-hover",
        [0x97979f0a, 0x97979f0a, 0x97979f0a, 0x97979f0f],
    ),
    (
        "--control-secondary-border-active",
        [0xafb0b6ff, 0x62636dff, 0x96979eff, 0x84858dff],
    ),
    (
        "--control-secondary-border-default",
        [0xafb0b6ff, 0x62636dff, 0x96979eff, 0x84858dff],
    ),
    (
        "--control-secondary-border-hover",
        [0xafb0b6ff, 0x62636dff, 0x96979eff, 0x84858dff],
    ),
    (
        "--control-secondary-icon-active",
        [0xffffffff, 0x000000ff, 0xffffffff, 0xffffffff],
    ),
    (
        "--control-secondary-icon-default",
        [0xffffffff, 0x000000ff, 0xffffffff, 0xffffffff],
    ),
    (
        "--control-secondary-icon-hover",
        [0xffffffff, 0x000000ff, 0xffffffff, 0xffffffff],
    ),
    (
        "--control-secondary-text-active",
        [0xffffffff, 0x000000ff, 0xffffffff, 0xffffffff],
    ),
    (
        "--control-secondary-text-default",
        [0xffffffff, 0x000000ff, 0xffffffff, 0xffffffff],
    ),
    (
        "--control-secondary-text-hover",
        [0xffffffff, 0x000000ff, 0xffffffff, 0xffffffff],
    ),
    (
        "--gradient-progress-pill-background",
        [0x81828aff, 0x8f9097ff, 0x696a73ff, 0x595a63ff],
    ),
    (
        "--icon-brand",
        [0xd1daffff, 0x3a48a3ff, 0xa3b5ffff, 0x8ca0fdff],
    ),
    (
        "--icon-default",
        [0xffffffff, 0x222327ff, 0xffffffff, 0xe4e4e6ff],
    ),
    (
        "--icon-feedback-critical",
        [0xffcfcdff, 0x931b26ff, 0xffa7a3ff, 0xfb8580ff],
    ),
    (
        "--icon-feedback-info",
        [0xc5deffff, 0x004a92ff, 0x97c3fcff, 0x72adf5ff],
    ),
    (
        "--icon-feedback-positive",
        [0xa8eabaff, 0x00582bff, 0x84d09aff, 0x65ba7fff],
    ),
    (
        "--icon-feedback-warning",
        [0xffe0b0ff, 0x7b3e00ff, 0xfebc41ff, 0xef9b00ff],
    ),
    (
        "--icon-link",
        [0xbdd9ffff, 0x00519eff, 0x8ebefaff, 0x66a5f3ff],
    ),
    (
        "--icon-muted",
        [0xd4d5d8ff, 0x4f505aff, 0xbabbc0ff, 0xa4a5abff],
    ),
    (
        "--icon-strong",
        [0xffffffff, 0x000000ff, 0xffffffff, 0xffffffff],
    ),
    (
        "--icon-subtle",
        [0xfbfbfbff, 0x393a41ff, 0xdcdcdfff, 0xc2c2c7ff],
    ),
    (
        "--icon-voice-disconnected",
        [0xffcfcdff, 0x931b26ff, 0xffa7a3ff, 0xfb8580ff],
    ),
    (
        "--input-background-default",
        [0x00000014, 0x00000004, 0x00000007, 0x00000004],
    ),
    (
        "--input-border-default",
        [0xafb0b6ff, 0x62636dff, 0x96979eff, 0x84858dff],
    ),
    (
        "--input-border-error-default",
        [0xffcfcdff, 0x931b26ff, 0xffa7a3ff, 0xfb8580ff],
    ),
    (
        "--input-border-hover",
        [0xefeff1ff, 0x41424aff, 0xd1d1d5ff, 0xbabbc0ff],
    ),
    (
        "--input-icon-default",
        [0xfbfbfbff, 0x393a41ff, 0xdcdcdfff, 0xc2c2c7ff],
    ),
    (
        "--input-placeholder-text-default",
        [0xafb0b6ff, 0x62636dff, 0x96979eff, 0x84858dff],
    ),
    (
        "--input-text-default",
        [0xffffffff, 0x222327ff, 0xffffffff, 0xe4e4e6ff],
    ),
    (
        "--input-text-error-default",
        [0xffffffff, 0x222327ff, 0xffffffff, 0xe4e4e6ff],
    ),
    (
        "--interactive-accent-background-default",
        [0x97979f04, 0x97979f07, 0x97979f04, 0x97979f07],
    ),
    (
        "--interactive-background-active",
        [0x97979f0a, 0x97979f0a, 0x97979f0a, 0x97979f0f],
    ),
    (
        "--interactive-background-default",
        [0x97979f02, 0x97979f04, 0x97979f02, 0x97979f04],
    ),
    (
        "--interactive-background-hover",
        [0x97979f04, 0x97979f07, 0x97979f04, 0x97979f07],
    ),
    (
        "--interactive-background-selected",
        [0x97979f0f, 0x97979f14, 0x97979f0f, 0x97979f14],
    ),
    (
        "--interactive-icon-active",
        [0xffffffff, 0x000000ff, 0xffffffff, 0xffffffff],
    ),
    (
        "--interactive-icon-default",
        [0xfbfbfbff, 0x393a41ff, 0xdcdcdfff, 0xc2c2c7ff],
    ),
    (
        "--interactive-icon-hover",
        [0xffffffff, 0x000000ff, 0xffffffff, 0xffffffff],
    ),
    (
        "--interactive-muted",
        [0x9d9ea5ff, 0x70717aff, 0x81828aff, 0x73747dff],
    ),
    (
        "--interactive-text-active",
        [0xffffffff, 0x000000ff, 0xffffffff, 0xffffffff],
    ),
    (
        "--interactive-text-default",
        [0xfbfbfbff, 0x393a41ff, 0xdcdcdfff, 0xc2c2c7ff],
    ),
    (
        "--interactive-text-hover",
        [0xffffffff, 0x000000ff, 0xffffffff, 0xffffffff],
    ),
    (
        "--keyword-highlight-background",
        [0x97979f0f, 0x97979f14, 0x97979f0f, 0x97979f14],
    ),
    (
        "--mention-background",
        [0x5865f23a, 0x5865f214, 0x5865f23a, 0x5865f23a],
    ),
    (
        "--mention-foreground",
        [0xe6eaffff, 0x1a275bff, 0xd8dfffff, 0xb6c5ffff],
    ),
    (
        "--message-automod-background-default",
        [0xd22d390a, 0xd22d3904, 0xd22d390a, 0xd22d390f],
    ),
    (
        "--message-automod-background-hover",
        [0xd22d390f, 0xd22d3907, 0xd22d390f, 0xd22d3914],
    ),
    (
        "--message-background-hover",
        [0x97979f04, 0x97979f07, 0x97979f04, 0x97979f07],
    ),
    (
        "--message-highlight-background-default",
        [0x5865f20a, 0x5865f20a, 0x5865f20a, 0x5865f20a],
    ),
    (
        "--message-highlight-background-hover",
        [0x5865f20f, 0x5865f20f, 0x5865f20f, 0x5865f20f],
    ),
    (
        "--message-mentioned-background-default",
        [0xf8a3000a, 0xf8a30004, 0xf8a3000a, 0xf8a3000f],
    ),
    (
        "--message-mentioned-background-hover",
        [0xf8a3000f, 0xf8a30007, 0xf8a3000f, 0xf8a30014],
    ),
    (
        "--mobile-guildbar-icon-default",
        [0xfbfbfbff, 0x393a41ff, 0xdcdcdfff, 0xc2c2c7ff],
    ),
    (
        "--mobile-text-heading-primary",
        [0xffffffff, 0x000000ff, 0xffffffff, 0xffffffff],
    ),
    (
        "--progressbar-track-background",
        [0x97979f0f, 0x97979f14, 0x97979f0f, 0x97979f14],
    ),
    (
        "--radio-border-active",
        [0xd4d5d8ff, 0x4f505aff, 0xbabbc0ff, 0xa4a5abff],
    ),
    (
        "--radio-border-default",
        [0xd4d5d8ff, 0x4f505aff, 0xbabbc0ff, 0xa4a5abff],
    ),
    (
        "--radio-border-hover",
        [0xd4d5d8ff, 0x4f505aff, 0xbabbc0ff, 0xa4a5abff],
    ),
    (
        "--radio-border-selected-default",
        [0xd4d5d8ff, 0x4f505aff, 0xbabbc0ff, 0xa4a5abff],
    ),
    (
        "--radio-border-selected-hover",
        [0xd4d5d8ff, 0x4f505aff, 0xbabbc0ff, 0xa4a5abff],
    ),
    (
        "--reaction-background-active",
        [0x97979f0f, 0x97979f14, 0x97979f0f, 0x97979f14],
    ),
    (
        "--reaction-background-default",
        [0x97979f04, 0x97979f07, 0x97979f04, 0x97979f07],
    ),
    (
        "--reaction-background-hover",
        [0x97979f0a, 0x97979f0a, 0x97979f0a, 0x97979f0f],
    ),
    (
        "--reaction-border-active",
        [0xffffffff, 0x2e2e34ff, 0xefeff1ff, 0xd4d5d8ff],
    ),
    (
        "--reaction-border-default",
        [0xd4d5d8ff, 0x4f505aff, 0xbabbc0ff, 0xa4a5abff],
    ),
    (
        "--reaction-border-hover",
        [0x96979eff, 0x7a7b83ff, 0x7d7e87ff, 0x6c6d76ff],
    ),
    (
        "--reaction-text-active",
        [0xffffffff, 0x000000ff, 0xffffffff, 0xffffffff],
    ),
    (
        "--reaction-text-default",
        [0xfbfbfbff, 0x393a41ff, 0xdcdcdfff, 0xc2c2c7ff],
    ),
    (
        "--reaction-text-hover",
        [0xffffffff, 0x000000ff, 0xffffffff, 0xffffffff],
    ),
    (
        "--redesign-button-tertiary-pressed-background",
        [0x97979f0f, 0x97979f14, 0x97979f0f, 0x97979f14],
    ),
    (
        "--redesign-button-tertiary-text",
        [0xffffffff, 0x222327ff, 0xffffffff, 0xe4e4e6ff],
    ),
    (
        "--spine-default",
        [0x8b8c94ff, 0x8b8c94ff, 0x767780ff, 0x666770ff],
    ),
    (
        "--status-danger",
        [0xf77b77ff, 0xd6363fff, 0xe75858ff, 0xd83a41ff],
    ),
    (
        "--switch-border-default",
        [0xd4d5d8ff, 0x4f505aff, 0xbabbc0ff, 0xa4a5abff],
    ),
    (
        "--switch-border-hover",
        [0xd4d5d8ff, 0x4f505aff, 0xbabbc0ff, 0xa4a5abff],
    ),
    (
        "--switch-border-selected-default",
        [0xd4d5d8ff, 0x4f505aff, 0xbabbc0ff, 0xa4a5abff],
    ),
    (
        "--switch-border-selected-hover",
        [0xd4d5d8ff, 0x4f505aff, 0xbabbc0ff, 0xa4a5abff],
    ),
    (
        "--tabs-indicator-default",
        [0xd1daffff, 0x3a48a3ff, 0xa3b5ffff, 0x8ca0fdff],
    ),
    (
        "--tabs-indicator-overlay",
        [0xffffffff, 0x000000ff, 0xffffffff, 0xffffffff],
    ),
    (
        "--text-brand",
        [0xd1daffff, 0x3a48a3ff, 0xa3b5ffff, 0x8ca0fdff],
    ),
    (
        "--text-code",
        [0xffffffff, 0x2f3035ff, 0xe0e0e3ff, 0xc9caceff],
    ),
    (
        "--text-code-addition",
        [0xc5ffd5ff, 0x003f1bff, 0x99e0adff, 0x7ecb94ff],
    ),
    (
        "--text-code-attribute",
        [0xe5efffff, 0x003d7aff, 0xabcfffff, 0x8abcfaff],
    ),
    (
        "--text-code-builtin",
        [0xffe7cfff, 0x672d00ff, 0xffc252ff, 0xf9a700ff],
    ),
    (
        "--text-code-bullet",
        [0xe5efffff, 0x003d7aff, 0xabcfffff, 0x8abcfaff],
    ),
    (
        "--text-code-comment",
        [0xc9caceff, 0x52535dff, 0xabacb2ff, 0x999aa1ff],
    ),
    (
        "--text-code-decorator",
        [0xffe7cfff, 0x672d00ff, 0xffc252ff, 0xf9a700ff],
    ),
    (
        "--text-code-deletion",
        [0xffdeddff, 0x6c0b19ff, 0xffc0bdff, 0xffa09bff],
    ),
    (
        "--text-code-error",
        [0xffdeddff, 0x7a111eff, 0xffbdb9ff, 0xff9d98ff],
    ),
    (
        "--text-code-escape",
        [0xffd7f4ff, 0x6f1258ff, 0xffb6edff, 0xff94e4ff],
    ),
    (
        "--text-code-keyword",
        [0xffdeddff, 0x7a111eff, 0xffbdb9ff, 0xff9d98ff],
    ),
    (
        "--text-code-link",
        [0xe5efffff, 0x003d7aff, 0xabcfffff, 0x8abcfaff],
    ),
    (
        "--text-code-namespace",
        [0xe5efffff, 0x003d7aff, 0xabcfffff, 0x8abcfaff],
    ),
    (
        "--text-code-number",
        [0xffe3d9ff, 0x6d2400ff, 0xfcc0a1ff, 0xf2a87dff],
    ),
    (
        "--text-code-operator",
        [0xffdeddff, 0x7a111eff, 0xffbdb9ff, 0xff9d98ff],
    ),
    (
        "--text-code-property",
        [0xc2f7fdff, 0x00444eff, 0x8edae1ff, 0x6cc6cfff],
    ),
    (
        "--text-code-regexp",
        [0xc1fcd1ff, 0x00461fff, 0x96dda9ff, 0x7ac991ff],
    ),
    (
        "--text-code-section",
        [0xffdeddff, 0x7a111eff, 0xffbdb9ff, 0xff9d98ff],
    ),
    (
        "--text-code-string",
        [0xc1fcd1ff, 0x00461fff, 0x96dda9ff, 0x7ac991ff],
    ),
    (
        "--text-code-tag",
        [0xffd7f4ff, 0x6f1258ff, 0xffb6edff, 0xff94e4ff],
    ),
    (
        "--text-code-title",
        [0xe5efffff, 0x003d7aff, 0xabcfffff, 0x8abcfaff],
    ),
    (
        "--text-code-type",
        [0xffd7f4ff, 0x6f1258ff, 0xffb6edff, 0xff94e4ff],
    ),
    (
        "--text-code-variable",
        [0xffffffff, 0x2f3035ff, 0xe0e0e3ff, 0xc9caceff],
    ),
    (
        "--text-default",
        [0xffffffff, 0x222327ff, 0xffffffff, 0xe4e4e6ff],
    ),
    (
        "--text-feedback-critical",
        [0xffcfcdff, 0x931b26ff, 0xffa7a3ff, 0xfb8580ff],
    ),
    (
        "--text-feedback-info",
        [0xc5deffff, 0x004a92ff, 0x97c3fcff, 0x72adf5ff],
    ),
    (
        "--text-feedback-positive",
        [0xa8eabaff, 0x00582bff, 0x84d09aff, 0x65ba7fff],
    ),
    (
        "--text-feedback-warning",
        [0xffe0b0ff, 0x7b3e00ff, 0xfebc41ff, 0xef9b00ff],
    ),
    (
        "--text-link",
        [0xbdd9ffff, 0x00519eff, 0x8ebefaff, 0x66a5f3ff],
    ),
    (
        "--text-muted",
        [0xd4d5d8ff, 0x4f505aff, 0xbabbc0ff, 0xa4a5abff],
    ),
    (
        "--text-strong",
        [0xffffffff, 0x000000ff, 0xffffffff, 0xffffffff],
    ),
    (
        "--text-subtle",
        [0xfbfbfbff, 0x393a41ff, 0xdcdcdfff, 0xc2c2c7ff],
    ),
    (
        "--text-voice-disconnected",
        [0xffcfcdff, 0x931b26ff, 0xffa7a3ff, 0xfb8580ff],
    ),
    (
        "--thread-channel-spine",
        [0x9d9ea5ff, 0x70717aff, 0x81828aff, 0x73747dff],
    ),
    (
        "--togglebutton-background-selected-active",
        [0x97979f0f, 0x97979f14, 0x97979f0f, 0x97979f14],
    ),
    (
        "--togglebutton-critical-icon-default",
        [0xffffffff, 0x222327ff, 0xffffffff, 0xe4e4e6ff],
    ),
    (
        "--togglebutton-critical-icon-hover",
        [0xffffffff, 0x222327ff, 0xffffffff, 0xe4e4e6ff],
    ),
    (
        "--togglebutton-icon-active",
        [0xffffffff, 0x222327ff, 0xffffffff, 0xe4e4e6ff],
    ),
    (
        "--togglebutton-icon-default",
        [0xffffffff, 0x222327ff, 0xffffffff, 0xe4e4e6ff],
    ),
    (
        "--togglebutton-icon-hover",
        [0xffffffff, 0x222327ff, 0xffffffff, 0xe4e4e6ff],
    ),
    (
        "--togglebutton-icon-only-icon-active",
        [0xffffffff, 0x000000ff, 0xffffffff, 0xffffffff],
    ),
    (
        "--togglebutton-icon-only-icon-default",
        [0xffffffff, 0x222327ff, 0xffffffff, 0xe4e4e6ff],
    ),
    (
        "--togglebutton-icon-only-icon-hover",
        [0xffffffff, 0x000000ff, 0xffffffff, 0xffffffff],
    ),
    (
        "--togglebutton-icon-only-icon-selected",
        [0xffffffff, 0x000000ff, 0xffffffff, 0xffffffff],
    ),
    (
        "--togglebutton-icon-only-icon-selected-hover",
        [0xffffffff, 0x000000ff, 0xffffffff, 0xffffffff],
    ),
    (
        "--togglebutton-icon-selected",
        [0xffffffff, 0x000000ff, 0xffffffff, 0xffffffff],
    ),
    (
        "--togglebutton-icon-selected-hover",
        [0xffffffff, 0x000000ff, 0xffffffff, 0xffffffff],
    ),
    (
        "--user-profile-background-hover",
        [0x97979f04, 0x97979f07, 0x97979f04, 0x97979f07],
    ),
    (
        "--user-profile-border",
        [0xd4d5d8ff, 0x4f505aff, 0xbabbc0ff, 0xa4a5abff],
    ),
    (
        "--user-profile-overlay-background-hover",
        [0x97979f04, 0x97979f07, 0x97979f04, 0x97979f07],
    ),
    (
        "--user-profile-toolbar-border",
        [0x96979eff, 0x7a7b83ff, 0x7d7e87ff, 0x6c6d76ff],
    ),
];
#[cfg(test)]
mod tests {
    use super::TOKENS;

    #[test]
    fn sorted_and_every_value_is_opaque_or_has_the_opacity_of_the_bundle() {
        assert!(TOKENS.windows(2).all(|pair| pair[0].0 < pair[1].0));
        assert!(TOKENS.iter().all(|(name, _)| name.starts_with("--")));
        assert!(TOKENS.len() > 200);
    }
}
