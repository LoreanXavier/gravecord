//! "Open in Mod View": the moderator sidebar beside the chat (module 956972, `guildSidebar_z_gUBg`, 480 wide).
//!
//! The official client opens `modViewPanel: INFO` in the guild sidebar (module 121127 `Open in Mod View`): the member
//! card (avatar, names, close, the Message / Kick / Ban / Timeout / Copy ID row, DOM `design/dom/menus2/
//! mod-view-self-darker-cozy.json`), then the "Mod View" card with its refresh button and Esc key and a scroller of
//! panel sections: Server Activity (`tp`), Mod Permissions (`tm`), Roles (`ta`) and Account (`te`).
//!
//! Server Activity counts come from the guild search with `author_id` (`/guilds/{id}/messages/search`, the
//! `tabs` request of module 956972 `eZ`, read here as the three GETs the search panel already uses) and the Audit
//! Log count from `/guilds/{id}/audit-logs?limit=50&target_id={user}` (`el`).

use super::{Root, avatar_source, color, icon, permission_spec};
use crate::net::Request;
use dc_state::{Id, permissions};
use gpui::{
    AnyElement, Context, FontWeight, InteractiveElement, IntoElement, ParentElement, Rgba,
    StatefulInteractiveElement, Styled, div, img, linear_color_stop, linear_gradient,
    prelude::FluentBuilder, px,
};

/// The key prefix of the panel's fetches: `modview:{guild}:{user}:{what}`.
pub(super) const KEY_PREFIX: &str = "modview:";

/// The panel's state: the member it shows.
pub(super) struct ModView {
    pub guild: Id,
    pub user: Id,
    /// The section scroller (`innerContainer_4sPjw-`) and its thin scrollbar.
    pub scroll: gpui::ScrollHandle,
}

/// The permissions the panel lists (`tT`: `tg.dR`, `Q.nfo` and the rest the module adds). The module orders them by
/// the guild permission spec; the titles come from it.
const LISTED: [u64; 15] = [
    permissions::ADMINISTRATOR,
    permissions::KICK_MEMBERS,
    permissions::BAN_MEMBERS,
    permissions::MANAGE_GUILD,
    permissions::MANAGE_CHANNELS,
    permissions::MANAGE_ROLES,
    permissions::MANAGE_MESSAGES,
    permissions::MANAGE_THREADS,
    permissions::MANAGE_GUILD_EXPRESSIONS,
    permissions::CREATE_GUILD_EXPRESSIONS,
    1 << 33, // MANAGE_EVENTS
    1 << 44, // CREATE_EVENTS
    permissions::MODERATE_MEMBERS,
    1 << 17, // MENTION_EVERYONE
    permissions::MANAGE_WEBHOOKS,
];

/// The elevated ones (`tu.Eo(flag, Q.nfo)`): the chips with the shield. Read from the DOM's `elevatedPermission`
/// chips of the capture: Manage Channels, Manage Roles, Manage Server, Kick, Ban, Timeout, Manage Messages, Manage
/// Threads and Posts, Administrator.
const ELEVATED: [u64; 9] = [
    permissions::MANAGE_CHANNELS,
    permissions::MANAGE_ROLES,
    permissions::MANAGE_GUILD,
    permissions::KICK_MEMBERS,
    permissions::BAN_MEMBERS,
    permissions::MODERATE_MEMBERS,
    permissions::MANAGE_MESSAGES,
    permissions::MANAGE_THREADS,
    permissions::ADMINISTRATOR,
];

/// The count `All (n)` shows for an Administrator or the owner: every key of `Permissions` (53 in the capture).
const ALL_COUNT: usize = 53;

/// A member's guild permissions (`cc` over their roles): the owner and Administrators have everything.
fn member_permissions(guild: &dc_state::Guild, user: Id, roles: &[Id]) -> u64 {
    if guild.owner_id == Some(user) {
        return permissions::ALL;
    }
    let mut total = guild.role_permissions.get(&guild.id).copied().unwrap_or(0);
    for role in roles {
        total |= guild.role_permissions.get(role).copied().unwrap_or(0);
    }
    if total & permissions::ADMINISTRATOR != 0 {
        return permissions::ALL;
    }
    total
}

/// The titles of the listed permissions a member holds, in the order of the guild spec.
fn chip_titles(total: u64) -> Vec<(&'static str, bool)> {
    let mut out = Vec::new();
    for section in permission_spec::GUILD {
        for permission in section.permissions {
            if LISTED.contains(&permission.flag) && total & permission.flag != 0 {
                out.push((permission.title, ELEVATED.contains(&permission.flag)));
            }
        }
    }
    out
}

/// `Intl.NumberFormat` for the counts.
fn count_text(value: i64) -> String {
    let digits = value.max(0).to_string();
    let mut out = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// The dates of the Account section (`toLocaleDateString` with `month: short, day: numeric, year: numeric`).
fn short_date(ms: i64) -> String {
    use chrono::TimeZone;
    chrono::Local
        .timestamp_millis_opt(ms)
        .single()
        .map(|date| date.format("%b %-d, %Y").to_string())
        .unwrap_or_default()
}

/// The panel's tones, sampled from the capture (the user's profile theme tints the official panel).
const FRAME_TOP: (u8, u8, u8) = (37, 40, 98);
const FRAME_BOTTOM: (u8, u8, u8) = (82, 84, 100);
const CARD: (u8, u8, u8) = (21, 25, 45);
const ACTIONS: (u8, u8, u8) = (30, 33, 52);
const HEADER: (u8, u8, u8) = (15, 20, 34);
const ITEM: (u8, u8, u8) = (13, 17, 30);
/// The scroller behind the section cards, and the 1px line between the rows of a card.
const SCROLLER: (u8, u8, u8) = (26, 31, 50);
const LINE: (u8, u8, u8) = (19, 21, 35);

fn tone((r, g, b): (u8, u8, u8)) -> Rgba {
    gpui::rgb(u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b))
}

/// The status of the member card's 48px avatar (`memberAvatar_ebzPvR`, the 57px mask): a 12px dot at its corner in a
/// 16px hole of the card's colour; the dnd bar, the offline hole and the idle bite are cut in the same colour.
fn card_status_dot(status: &str) -> impl IntoElement {
    let tint = match status {
        "dnd" => "--icon-status-dnd",
        "idle" => "--icon-status-idle",
        "invisible" | "offline" => "--icon-status-offline",
        _ => "--icon-status-online",
    };
    let cut = tone(CARD);
    div()
        .absolute()
        .right(px(-2.))
        .bottom(px(-2.))
        .size(px(16.))
        .rounded_full()
        .bg(cut)
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .relative()
                .size(px(12.))
                .rounded_full()
                .bg(color(tint))
                .flex()
                .items_center()
                .justify_center()
                .when(status == "dnd", |dot| {
                    dot.child(div().w(px(7.2)).h(px(2.4)).rounded(px(1.2)).bg(cut))
                })
                .when(matches!(status, "offline" | "invisible"), |dot| {
                    dot.child(div().size(px(6.)).rounded_full().bg(cut))
                })
                .when(status == "idle", |dot| {
                    dot.child(
                        div()
                            .absolute()
                            .left(px(-1.5))
                            .top(px(-1.5))
                            .size(px(9.))
                            .rounded_full()
                            .bg(cut),
                    )
                }),
        )
}

impl Root {
    /// `Open in Mod View`: the panel replaces the member list and starts its fetches.
    pub(super) fn open_mod_view(&mut self, guild: Id, user: Id, cx: &mut Context<Self>) {
        self.mod_view = Some(ModView {
            guild,
            user,
            scroll: gpui::ScrollHandle::new(),
        });
        self.request_mod_view(guild, user);
        cx.notify();
    }

    /// The refresh button's work (`m`): the member, the search counts and the audit log again.
    fn request_mod_view(&mut self, guild: Id, user: Id) {
        let base =
            format!("/guilds/{guild}/messages/search?author_id={user}&limit=1&include_nsfw=true");
        let routes = [
            ("messages", base.clone()),
            ("links", format!("{base}&has=link")),
            ("media", format!("{base}&has=image&has=video&has=sound")),
            (
                "audit",
                format!("/guilds/{guild}/audit-logs?limit=50&target_id={user}"),
            ),
        ];
        for (what, route) in routes {
            let _ = self.requests.send(Request::Fetch {
                key: format!("{KEY_PREFIX}{guild}:{user}:{what}"),
                route,
            });
        }
    }

    /// The answer of one fetch: kept for the panel.
    pub(super) fn mod_view_fetched(
        &mut self,
        key: &str,
        body: serde_json::Value,
        cx: &mut Context<Self>,
    ) {
        self.fetched.insert(key.to_owned(), body);
        cx.notify();
    }

    /// The count of one activity row: `-1` while it loads, as the module keeps it.
    fn mod_view_count(&self, open: &ModView, what: &str) -> i64 {
        let key = format!("{KEY_PREFIX}{}:{}:{what}", open.guild, open.user);
        let Some(body) = self.fetched.get(&key) else {
            return -1;
        };
        if body["fetch_failed"] == true {
            return 0;
        }
        if what == "audit" {
            return body["audit_log_entries"]
                .as_array()
                .map_or(0, |entries| entries.len() as i64);
        }
        body["total_results"].as_i64().unwrap_or(0)
    }

    /// One `modInfoItem` row: a 16px icon, the name and the value or a chevron.
    fn mod_view_item(
        &self,
        icon_view: AnyElement,
        name: &str,
        value: AnyElement,
        weight: f32,
        chevron: bool,
        edge: u8,
    ) -> AnyElement {
        // `edge` bit 1: first row of its group, bit 2: last. The group's 8px corners are drawn by its end rows, because
        // an overflow clip here is a plain rectangle.
        let (first, last) = (edge & 1 != 0, edge & 2 != 0);
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(8.))
            .px(px(12.))
            .py(px(12.))
            .bg(tone(ITEM))
            .when(first, |row| row.rounded_t(px(8.)))
            .when(last, |row| row.rounded_b(px(8.)))
            .when(!last, |row| row.mb(px(1.)))
            .child(icon_view)
            .child(
                div()
                    .flex_1()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(weight))
                    .text_color(color("--text-default"))
                    .child(name.to_owned()),
            )
            .child(value)
            .when(chevron, |row| {
                row.child(icon("menu_caret_right", 16., "--interactive-icon-default"))
            })
            .into_any_element()
    }

    /// A section's 20px label above its rounded group (`panelSection`, 8px between).
    fn mod_view_section(label: &str, body: AnyElement) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .flex_none()
            .gap(px(8.))
            .child(
                // `label_VhFsFg`, `body-md/medium`: 16px in a 20px line (117.75px for "Mod Permissions").
                div()
                    .h(px(20.))
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-strong"))
                    .child(label.to_owned()),
            )
            .child(
                // `modInfoItemContainer_M3Y1qv`: 8px radius, the rows 1px apart.
                div()
                    .flex()
                    .flex_col()
                    .rounded(px(8.))
                    .overflow_hidden()
                    .bg(tone(LINE))
                    .child(body),
            )
            .into_any_element()
    }

    /// The panel itself, beside the chat in place of the member list.
    pub(super) fn mod_view_panel(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let open = self.mod_view.as_ref()?;
        let (guild_id, user_id) = (open.guild, open.user);
        let guild = self.stores.guild(guild_id)?;
        let user = self
            .stores
            .users
            .get(&user_id)
            .cloned()
            .or_else(|| {
                self.stores
                    .current_user
                    .clone()
                    .filter(|current| current.id == user_id)
            })
            .unwrap_or_default();
        let member = self.stores.members.get(guild_id, user_id);
        let roles: Vec<Id> = member
            .map(|member| member.roles.clone())
            .unwrap_or_default();
        let total = member_permissions(guild, user_id, &roles);
        let chips = chip_titles(total);
        let all = if total == permissions::ALL {
            ALL_COUNT
        } else {
            total.count_ones() as usize
        };
        let avatar = self.images.get(&avatar_source(&user).0).cloned();
        let presence = if self
            .stores
            .current_user
            .as_ref()
            .is_some_and(|me| me.id == user_id)
        {
            self.stores.status.clone()
        } else {
            self.stores
                .presences
                .get(&user_id)
                .cloned()
                .unwrap_or_else(|| "offline".to_owned())
        };
        // `modInfoItemIcon_M3Y1qv`: 16px; the Server Join Date row shows the server's icon (`guildIcon_4sPjw-`,
        // `iconSizeSmol`: 16px, its acronym at 8px when it has no picture).
        let glyph =
            |name: &'static str| icon(name, 16., "--interactive-icon-default").into_any_element();
        let guild_glyph = match self.images.get(&format!("guild:{guild_id}")).cloned() {
            Some(picture) => img(picture).size(px(16.)).rounded_full().into_any_element(),
            None => div()
                .size(px(16.))
                .flex_none()
                .rounded_full()
                .bg(tone(ITEM))
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(8.))
                .line_height(px(10.))
                .font_weight(FontWeight(500.))
                .text_color(color("--text-strong"))
                .child(guild.acronym())
                .into_any_element(),
        };

        // The member card: names on the left, the close button on the right.
        let top = div()
            .flex()
            .items_center()
            .justify_between()
            .h(px(72.))
            .px(px(8.))
            .pt(px(12.))
            .pb(px(12.))
            .bg(tone(CARD))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        // `memberAvatar_ebzPvR`: 48px, the status in a 12px dot at its corner (the 57px mask).
                        div()
                            .relative()
                            .size(px(48.))
                            .flex_none()
                            .children(avatar.map(|image| img(image).size(px(48.)).rounded_full()))
                            .child(card_status_dot(&presence)),
                    )
                    .child(
                        // `memberNameAndTagContainer_ebzPvR`: its top is the avatar's (names at y 52 and 76).
                        div()
                            .self_start()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .h(px(24.))
                                    .text_size(px(20.))
                                    .line_height(px(24.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-strong"))
                                    .child(user.display_name().to_owned()),
                            )
                            .child(
                                div()
                                    .h(px(20.))
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-strong"))
                                    .child(user.username.clone()),
                            ),
                    ),
            )
            .child(
                div()
                    .id("mod-view-close")
                    .p(px(4.))
                    .cursor_pointer()
                    .child(icon("mod_close", 24., "--text-strong"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.mod_view = None;
                        cx.notify();
                    })),
            );

        // Message, Kick, Ban, Timeout, Copy ID: five 90px tabs with 1px between (`bottomRowAction`). The disabled
        // ones are dimmed (`bottomRowActionDisabled`) on our own card; Copy ID copies the member's id.
        let tab = |id: &'static str, glyph: &'static str, enabled: bool| {
            div()
                .id(id)
                .flex_1()
                .h(px(48.))
                .flex()
                .items_center()
                .justify_center()
                // The disabled ones sit on the grey wash of `innerBottomRowAction` with a half-faded icon; Copy ID is
                // plain on the card's colour with the white icon (menus2/mod-view-self: 112/115/127 and 246).
                .bg(tone(if enabled { CARD } else { ACTIONS }))
                .when(enabled, |tab| tab.cursor_pointer())
                .child(div().when(!enabled, |glyph| glyph.opacity(0.6)).child(icon(
                    glyph,
                    24.,
                    if enabled {
                        "--text-strong"
                    } else {
                        "--interactive-icon-default"
                    },
                )))
        };
        let actions = div()
            .flex()
            .gap(px(1.))
            .mt(px(1.))
            .child(tab("mod-view-message", "mod_tab_message", false))
            .child(tab("mod-view-kick", "mod_tab_kick", false))
            .child(tab("mod-view-ban", "mod_tab_ban", false))
            .child(tab("mod-view-timeout", "mod_tab_timeout", false))
            .child(
                tab("mod-view-copy-id", "mod_tab_copy_id", true).on_click(cx.listener(
                    move |_, _, _, cx| {
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(user_id.to_string()));
                    },
                )),
            );
        let card = div()
            .flex()
            .flex_col()
            .flex_none()
            .rounded(px(8.))
            .overflow_hidden()
            .child(top)
            .child(actions);

        // Server Activity.
        let number = |value: i64| {
            if value < 0 {
                div().into_any_element()
            } else {
                div()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--interactive-text-default"))
                    .child(count_text(value))
                    .into_any_element()
            }
        };
        let audit = self.mod_view_count(open, "audit");
        let activity = Self::mod_view_section(
            "Server Activity",
            div()
                .flex()
                .flex_col()
                .child(self.mod_view_item(
                    glyph("mod_info_messages"),
                    "Messages",
                    number(self.mod_view_count(open, "messages")),
                    600.,
                    false,
                    1,
                ))
                .child(self.mod_view_item(
                    glyph("mod_info_links"),
                    "Links",
                    number(self.mod_view_count(open, "links")),
                    600.,
                    false,
                    0,
                ))
                .child(self.mod_view_item(
                    glyph("mod_info_media"),
                    "Media",
                    number(self.mod_view_count(open, "media")),
                    600.,
                    false,
                    0,
                ))
                .child(self.mod_view_item(
                    glyph("role_shield"),
                    "Audit Log",
                    number(audit),
                    600.,
                    audit > 0,
                    2,
                ))
                .into_any_element(),
        );

        // Mod Permissions: the title row with `All (n)`, then the chips in a group.
        let chip_list = if chips.is_empty() {
            div()
                .text_size(px(14.))
                .text_color(color("--text-feedback-positive"))
                .child("No Mod Permissions")
                .into_any_element()
        } else {
            let mut wrap = div().flex().flex_wrap().gap(px(8.)).w_full();
            for (title, elevated) in chips {
                wrap = wrap.child(
                    div()
                        .h(px(24.))
                        .px(px(4.))
                        .gap(px(4.))
                        .flex()
                        .items_center()
                        .rounded(px(4.))
                        // `elevatedPermission_whSvVg`: 0.2 of the grey with the shield, 0.12 without.
                        .bg(gpui::rgba(if elevated { 0x97979f33 } else { 0x97979f1f }))
                        .when(elevated, |chip| {
                            chip.child(icon("mod_chip_shield", 16., "--text-brand"))
                        })
                        .child(
                            div()
                                .text_size(px(12.))
                                .line_height(px(16.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--interactive-text-default"))
                                .child(title),
                        ),
                );
            }
            wrap.into_any_element()
        };
        let permissions_section = div()
            .flex()
            .flex_col()
            .flex_none()
            .gap(px(8.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .h(px(20.))
                    .child(
                        div()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-strong"))
                            .child("Mod Permissions"),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            // `viewAllPermissions_whSvVg`: `body-sm/medium`, 14px.
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--interactive-text-default"))
                            .child(format!("All ({all})"))
                            .child(icon("menu_caret_right", 16., "--interactive-icon-default")),
                    ),
            )
            .child(
                div()
                    .rounded(px(8.))
                    .bg(tone(ITEM))
                    .p(px(12.))
                    .child(chip_list),
            );

        // Roles: the highest role's chip first, the rest after it (`ta`).
        // 26px tall: the golden card is 50px with 12px padding.
        let mut role_row = div()
            .flex()
            .flex_wrap()
            .items_center()
            .min_h(px(26.))
            .gap(px(4.));
        let mut held: Vec<Id> = roles
            .iter()
            .copied()
            .filter(|id| *id != guild_id && guild.roles.contains_key(id))
            .collect();
        held.sort_by_key(|id| {
            std::cmp::Reverse(guild.role_info.get(id).map_or(0, |info| info.position))
        });
        for (index, id) in held.iter().enumerate() {
            let info = guild.role_info.get(id).copied().unwrap_or_default();
            let tint = if info.color == 0 {
                color("--primary-300")
            } else {
                gpui::rgb(info.color)
            };
            role_row = role_row.child(
                div()
                    .h(px(22.))
                    .px(px(4.))
                    .gap(px(4.))
                    .flex()
                    .items_center()
                    .rounded(px(4.))
                    .border_1()
                    .border_color(gpui::rgba(
                        (info.color << 8) | if index == 0 { 0x99 } else { 0x33 },
                    ))
                    .bg(gpui::rgba((info.color << 8) | 0x27))
                    .child(div().size(px(12.)).rounded_full().bg(tint))
                    .child(
                        div()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-default"))
                            .child(guild.roles.get(id).cloned().unwrap_or_default()),
                    ),
            );
        }
        if self.my_permission(guild_id, None, permissions::MANAGE_ROLES) {
            role_row = role_row.child(div().id("mod-view-add-role").cursor_pointer().child(icon(
                "mod_add_role",
                20.,
                "--interactive-icon-default",
            )));
        }
        let roles_section = Self::mod_view_section(
            "Roles",
            div()
                .p(px(12.))
                .rounded(px(8.))
                .bg(tone(ITEM))
                .child(role_row)
                .into_any_element(),
        );

        // Account (`te`): verification, Discord and server join dates, the join method.
        let mark = div()
            .child(icon("checkmark_large", 16., "--status-positive"))
            .into_any_element();
        let text = |value: String| {
            div()
                .text_size(px(14.))
                .line_height(px(18.))
                .font_weight(FontWeight(500.))
                .text_color(color("--text-default"))
                .child(value)
                .into_any_element()
        };
        let created = (user_id >> 22) as i64 + 1_420_070_400_000;
        let joined = member
            .and_then(|member| member.joined_at.as_deref())
            .and_then(|joined| chrono::DateTime::parse_from_rfc3339(joined).ok())
            .map(|time| time.timestamp_millis())
            .map(short_date)
            .unwrap_or_default();
        let account = Self::mod_view_section(
            "Account",
            div()
                .flex()
                .flex_col()
                .child(self.mod_view_item(
                    glyph("mod_info_verified"),
                    "Passed Verification Level",
                    mark,
                    500.,
                    false,
                    1,
                ))
                .child(self.mod_view_item(
                    glyph("sb_discord"),
                    "Discord Join Date",
                    text(short_date(created)),
                    500.,
                    false,
                    0,
                ))
                .child(self.mod_view_item(
                    guild_glyph,
                    "Server Join Date",
                    text(joined),
                    500.,
                    false,
                    0,
                ))
                .child(self.mod_view_item(
                    glyph("mod_info_join_method"),
                    "Join Method",
                    text("Unknown".to_owned()),
                    500.,
                    false,
                    2,
                ))
                .into_any_element(),
        );

        // The "Mod View" card: its 48px header with the refresh button and the Esc key, then the scroller.
        let refresh = move |this: &mut Self, cx: &mut Context<Self>| {
            this.request_mod_view(guild_id, user_id);
            cx.notify();
        };
        let header = div()
            .flex()
            .flex_none()
            .items_center()
            .justify_between()
            .h(px(48.))
            .px(px(16.))
            .bg(tone(HEADER))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(icon(
                        "mod_header_mod_view",
                        16.,
                        "--interactive-icon-default",
                    ))
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .child("Mod View"),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .id("mod-view-refresh")
                            .cursor_pointer()
                            .child(icon("mod_refresh", 16., "--interactive-icon-default"))
                            .on_click(cx.listener(move |this, _, _, cx| refresh(this, cx))),
                    )
                    .child(
                        div()
                            .id("mod-view-esc")
                            .px(px(7.))
                            .h(px(23.))
                            .flex()
                            .items_center()
                            .rounded(px(4.))
                            .bg(gpui::rgba(0x97979f33))
                            .text_size(px(12.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .cursor_pointer()
                            .child("ESC")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.mod_view = None;
                                cx.notify();
                            })),
                    ),
            );
        let scroll = open.scroll.clone();
        let body = div()
            .id("mod-view-scroll")
            .track_scroll(&scroll)
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap(px(20.))
            // `innerContainer_4sPjw-`: 16px left, 16px right plus the 8px scrollbar gutter, 12px above.
            .pl(px(16.))
            .pr(px(24.))
            .pt(px(12.))
            .pb(px(12.))
            .bg(tone(SCROLLER))
            .child(activity)
            .child(permissions_section)
            .child(roles_section)
            .child(account);
        let sections = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.))
            .rounded(px(8.))
            .overflow_hidden()
            .child(header)
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h(px(0.))
                    .flex()
                    .flex_col()
                    .child(body)
                    .child(super::menu::menu_scrollbar(scroll.clone())),
            );

        Some(
            div()
                .flex_none()
                .w(px(480.))
                .h_full()
                .pl(px(8.))
                .child(
                    div()
                        .size_full()
                        .rounded_tl(px(8.))
                        .p(px(8.))
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .bg(linear_gradient(
                            180.,
                            linear_color_stop(tone(FRAME_TOP), 0.),
                            linear_color_stop(tone(FRAME_BOTTOM), 1.),
                        ))
                        .child(card)
                        .child(sections),
                )
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_group_thousands() {
        assert_eq!(count_text(0), "0");
        assert_eq!(count_text(1234567), "1,234,567");
    }

    #[test]
    fn an_administrator_lists_all_fifteen_in_spec_order() {
        let titles = chip_titles(permissions::ALL);
        let names: Vec<&str> = titles.iter().map(|(title, _)| *title).collect();
        assert_eq!(names.first(), Some(&"Manage Channels"));
        assert_eq!(names.last(), Some(&"Administrator"));
        assert_eq!(titles.iter().filter(|(_, elevated)| *elevated).count(), 9);
    }

    #[test]
    fn nothing_granted_lists_nothing() {
        assert!(chip_titles(0).is_empty());
    }
}
