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

fn tone((r, g, b): (u8, u8, u8)) -> Rgba {
    gpui::rgb(u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b))
}

impl Root {
    /// `Open in Mod View`: the panel replaces the member list and starts its fetches.
    pub(super) fn open_mod_view(&mut self, guild: Id, user: Id, cx: &mut Context<Self>) {
        self.mod_view = Some(ModView { guild, user });
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
        icon_name: &'static str,
        name: &str,
        value: AnyElement,
        weight: f32,
        chevron: bool,
        last: bool,
    ) -> AnyElement {
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(8.))
            .px(px(12.))
            .py(px(12.))
            .bg(tone(ITEM))
            .when(!last, |row| row.mb(px(1.)))
            .child(icon(icon_name, 16., "--interactive-icon-default"))
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
                div()
                    .h(px(20.))
                    .text_size(px(14.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-strong"))
                    .child(label.to_owned()),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .rounded(px(8.))
                    .overflow_hidden()
                    .border_1()
                    .border_color(gpui::black().opacity(0.16))
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
                        div()
                            .size(px(48.))
                            .flex_none()
                            .children(avatar.map(|image| img(image).size(px(48.)).rounded_full())),
                    )
                    .child(
                        div()
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
                    .child(icon("close", 24., "--interactive-icon-default"))
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
                .bg(tone(ACTIONS))
                .when(!enabled, |tab| tab.opacity(0.5))
                .when(enabled, |tab| tab.cursor_pointer())
                .child(icon(glyph, 24., "--interactive-icon-default"))
        };
        let actions = div()
            .flex()
            .gap(px(1.))
            .mt(px(1.))
            .child(tab("mod-view-message", "chat_bubble", false))
            .child(tab("mod-view-kick", "oauth_entry_hammer", false))
            .child(tab("mod-view-ban", "oauth_entry_hammer", false))
            .child(tab("mod-view-timeout", "automod_timeout", false))
            .child(
                tab("mod-view-copy-id", "menu_id", true).on_click(cx.listener(
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
                    "voice_view_chat",
                    "Messages",
                    number(self.mod_view_count(open, "messages")),
                    600.,
                    false,
                    false,
                ))
                .child(self.mod_view_item(
                    "clips_link",
                    "Links",
                    number(self.mod_view_count(open, "links")),
                    600.,
                    false,
                    false,
                ))
                .child(self.mod_view_item(
                    "clips_image",
                    "Media",
                    number(self.mod_view_count(open, "media")),
                    600.,
                    false,
                    false,
                ))
                .child(self.mod_view_item(
                    "role_shield",
                    "Audit Log",
                    number(audit),
                    600.,
                    audit > 0,
                    true,
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
                        .bg(gpui::rgba(0x97979f33))
                        .when(elevated, |chip| {
                            chip.child(icon("role_shield", 16., "--text-brand"))
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
                            .text_size(px(14.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-strong"))
                            .child("Mod Permissions"),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .text_size(px(12.))
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
                    .border_1()
                    .border_color(gpui::black().opacity(0.16))
                    .bg(tone(ITEM))
                    .p(px(12.))
                    .child(chip_list),
            );

        // Roles: the highest role's chip first, the rest after it (`ta`).
        let mut role_row = div().flex().flex_wrap().items_center().gap(px(4.));
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
                "plus_small",
                20.,
                "--interactive-icon-default",
            )));
        }
        let roles_section = Self::mod_view_section(
            "Roles",
            div()
                .p(px(12.))
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
                    "user_check",
                    "Passed Verification Level",
                    mark,
                    500.,
                    false,
                    false,
                ))
                .child(self.mod_view_item(
                    "sb_discord",
                    "Discord Join Date",
                    text(short_date(created)),
                    500.,
                    false,
                    false,
                ))
                .child(self.mod_view_item(
                    "member_invite_link",
                    "Server Join Date",
                    text(joined),
                    500.,
                    false,
                    false,
                ))
                .child(self.mod_view_item(
                    "user_plus",
                    "Join Method",
                    text("Unknown".to_owned()),
                    500.,
                    false,
                    true,
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
                    .child(icon("member_mod_view", 16., "--interactive-icon-default"))
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
                            .child(icon("system_refresh", 16., "--interactive-icon-default"))
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
        let body = div()
            .id("mod-view-scroll")
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap(px(20.))
            .px(px(16.))
            .pt(px(12.))
            .pb(px(16.))
            .bg(tone(ITEM))
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
            .child(body);

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
