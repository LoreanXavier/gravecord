//! The profile beside a DM (module 289287, `oD` → `oM` → `oE`). The account's assignment to
//! `2026-06-user-profile-sidebar-redesign` picks the layout, as `rD` does: the redesign is a 304px card
//! (`user-profile-sidebar-redesign`, measured in `design/dom/dm/loreanxavier-darker-cozy.json`), the classic panel a
//! 340px column (`user-profile-sidebar`, from the stylesheet only: no capture of it exists).
//!
//! Colours come from `profile_theme`: the sidebar wears the viewer's theme (`themeOverride`), its colours from
//! `theme_colors` or the avatar palette for a Nitro profile; any other profile draws unthemed on
//! `--chat-background-default` with its banner in `primaryColor ?? accent_color ?? avatar palette` (module 915614).

use super::profile_theme::{self, ProfileTheme};
use super::{Root, avatar_source, color, icon};
use crate::net::Request;
use crate::parity::Parity;
use crate::ui::tooltip::{Side, Tip};
use dc_state::{Id, User};
use gpui::{
    AnyElement, Context, FontWeight, Hsla, IntoElement, Rgba, ScrollHandle, SharedString, Window,
    div, hsla, img, linear_color_stop, linear_gradient, prelude::*, px, rgba,
};
use serde_json::Value;

/// `rP` of module 289287: the redesigned sidebar (variation 1).
pub(super) const SIDEBAR_REDESIGN: &str = "2026-06-user-profile-sidebar-redesign";
/// `r2` of module 289287: variation 1 hides "View Full Profile" in the redesigned footer.
pub(super) const HIDE_FULL_PROFILE_BUTTON: &str = "2026-04-hide-view-full-profile-button";
/// Module 643056: variations 1 and 2 enable badge management (six badges and an overflow count).
pub(super) const BADGE_MANAGEMENT: &str = "2026-08-badge-management";

/// The badges a managed list shows before its overflow count (`S.slice(0, 6)` in module 567556).
const MANAGED_BADGES: usize = 6;
/// Games the collection breadcrumb shows (`slice(0, 4)` in module 312294).
const BREADCRUMB_GAMES: usize = 4;
/// Wishlist items the breadcrumb shows (`E.slice(0, 3)`).
const BREADCRUMB_ITEMS: usize = 3;
/// `--custom-wishlist-breadcrumb-card-size` in the redesigned sidebar.
const WISHLIST_CARD: f32 = 78.;

thread_local! {
    /// The sidebar's scroller (`scrollerFade_S_xUx6`), read back for the bottom fade, per recipient.
    static SCROLL: std::cell::RefCell<(Option<Id>, ScrollHandle)> =
        std::cell::RefCell::new((None, ScrollHandle::new()));
}

fn dm_profile_scroll(user: Id) -> ScrollHandle {
    SCROLL.with(|cell| {
        let mut state = cell.borrow_mut();
        if state.0 != Some(user) {
            state.0 = Some(user);
            state.1.set_offset(gpui::point(px(0.), px(0.)));
        }
        state.1.clone()
    })
}

/// Colours of a profile card: background stops, overlay, and the tokens it resolves.
pub(super) struct Palette {
    pub theme: Option<ProfileTheme>,
    pub top: Rgba,
    pub bottom: Rgba,
    pub overlay: Hsla,
    /// `--text-default`.
    pub text: Hsla,
    /// `--interactive-text-default`, the mutuals.
    pub subtle: Hsla,
    pub themed: bool,
}

impl Palette {
    /// A token inside the card: the profile theme's resolution, or the client's own.
    pub(super) fn token(&self, name: &str) -> Rgba {
        match &self.theme {
            Some(theme) => theme.token(name),
            None => color(name),
        }
    }

    /// Badge / server-tag pill fill (`--opacity-white-24`/`40` on light custom themes).
    pub(super) fn pill_background(&self, hover: bool) -> Rgba {
        match &self.theme {
            Some(theme) => theme.pill_background(hover),
            None => color(if hover {
                "--background-mod-strong"
            } else {
                "--background-mod-subtle"
            }),
        }
    }

    /// Widget add tile fill (`addButtonContainer_ECBSq8`).
    pub(super) fn tile_background(&self, hover: bool) -> Rgba {
        match &self.theme {
            Some(theme) => theme.tile_background(hover),
            None => color(if hover {
                "--background-mod-normal"
            } else {
                "--background-mod-subtle"
            }),
        }
    }

    /// `.profile_s3DB6Y` vertical backdrop (`top` solid for `banner_height`, then `top -> bottom`).
    pub(super) fn card_backdrop(&self, banner_height: f32) -> Option<gpui::Div> {
        let theme = self.theme.as_ref()?;
        let (start, end) = theme.card_gradient_stops();
        Some(
            div()
                .absolute()
                .inset_0()
                .flex()
                .flex_col()
                .child(div().w_full().h(px(banner_height)).flex_none().bg(start))
                .child(div().w_full().flex_1().bg(linear_gradient(
                    180.,
                    linear_color_stop(start, 0.),
                    linear_color_stop(end, 1.),
                ))),
        )
    }

    /// `.inner_P1YEjQ` 300px vertical overlay fade over `modal_background` (`253781.63b02f3f5d17fef5.css`).
    pub(super) fn modal_backdrop(&self) -> Option<gpui::Div> {
        let theme = self.theme.as_ref()?;
        let (top, bottom) = theme.modal_inner_stops();
        Some(
            div()
                .absolute()
                .inset_0()
                .flex()
                .flex_col()
                .child(div().w_full().h(px(300.)).flex_none().bg(linear_gradient(
                    180.,
                    linear_color_stop(top, 0.),
                    linear_color_stop(bottom, 1.),
                )))
                .child(div().w_full().flex_1().bg(bottom)),
        )
    }
}

/// An entry of `mutual_friends`: a user object, or one wrapped as `{user}`.
pub(super) fn mutual_user(entry: &serde_json::Value) -> Option<dc_state::User> {
    let value = if entry["user"].is_object() {
        &entry["user"]
    } else {
        entry
    };
    dc_state::User::from_value(value)
}

/// Discord's snowflake epoch, for the account's creation date.
const EPOCH_MS: i64 = 1_420_070_400_000;

/// Module 743987: the snowflake's time through `An` of module 370480 (`toLocaleDateString` with a short month) in
/// the interface locale.
pub(super) fn member_since(user: Id) -> String {
    super::timestamps::short_month_date((user >> 22) as i64 + EPOCH_MS)
}

/// The sidebar's profile read (`oD`): `type: "sidebar"`, mutual friends' count unless a bot, the friends themselves
/// only in the redesign, mutual guilds always.
pub(super) fn sidebar_route(user: Id, bot: bool, redesign: bool) -> String {
    format!(
        "/users/{user}/profile?type=sidebar&with_mutual_guilds=true&with_mutual_friends={}&with_mutual_friends_count={}",
        !bot && redesign,
        !bot
    )
}

/// The games named by a profile's game widgets (`CURRENT_GAMES`, `FAVORITE_GAMES`, `PLAYED_GAMES`,
/// `WANT_TO_PLAY_GAMES`), each once, in widget order (module 312294 `eo`, after module 851767 `F`).
pub(super) fn widget_games(body: &Value) -> Vec<String> {
    let mut games: Vec<String> = Vec::new();
    for widget in body["widgets"].as_array().into_iter().flatten() {
        let data = &widget["data"];
        let kind = data["type"].as_str().unwrap_or_default();
        if !matches!(
            kind,
            "current_games" | "favorite_games" | "played_games" | "want_to_play_games"
        ) {
            continue;
        }
        for game in data["games"].as_array().into_iter().flatten() {
            if let Some(id) = game["game_id"].as_str()
                && !games.iter().any(|known| known == id)
            {
                games.push(id.to_owned());
            }
        }
    }
    games
}

/// `GET /games` takes at most twenty ids (module 920495).
pub(super) const GAMES_PER_FETCH: usize = 20;

/// The fetch key of one twenty-id batch of a profile's widget games; the first keeps the plain key.
pub(super) fn profile_games_key(user: Id, chunk: usize) -> String {
    if chunk == 0 {
        format!("profile-games:{user}")
    } else {
        format!("profile-games:{user}:{chunk}")
    }
}

/// Whether a fetch key is one of that profile's game batches.
pub(super) fn is_profile_games_key(key: &str, user: Id) -> bool {
    let base = format!("profile-games:{user}");
    key == base
        || key
            .strip_prefix(base.as_str())
            .is_some_and(|rest| rest.starts_with(':'))
}

fn games_route(ids: &[String]) -> String {
    let mut url = reqwest::Url::parse("https://discord.com/api/v9/games").expect("constant URL");
    for id in ids {
        url.query_pairs_mut().append_pair("game_ids", id);
    }
    format!("/games?{}", url.query().unwrap_or_default())
}

/// A `GET /games` record's icon hash: `media.icon` of type `hash` (module 997013 `getIconURL`), else the record's
/// `icon_hash`. The records carry no top-level `icon`; the capture demo's synthetic records still do.
pub(super) fn game_icon_hash(game: &Value) -> Option<&str> {
    let media = &game["media"]["icon"];
    (media["type"] == "hash")
        .then(|| media["value"].as_str())
        .flatten()
        .or_else(|| game["icon_hash"].as_str())
        .or_else(|| game["icon"].as_str())
        .filter(|hash| !hash.is_empty())
}

/// The cards `renderCards` of module 312294 makes from the widgets: one collection breadcrumb for every game widget
/// together, then one per personal widget (application widgets need their surfaces, which are not read here; clips
/// need their own experiment).
pub(super) fn widget_card_count(body: &Value) -> usize {
    let widgets = body["widgets"].as_array().map(Vec::as_slice).unwrap_or(&[]);
    let games = !widget_games(body).is_empty();
    let personal = widgets
        .iter()
        .filter(|widget| widget["data"]["type"] == "personal")
        .count();
    usize::from(games) + personal
}

/// The unowned items of a wishlist answer, in order (`items.filter(e => !e.isOwned)`), as sku ids.
pub(super) fn unowned_wishlist(body: &Value) -> Vec<String> {
    body["wishlist_items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|item| item["is_owned"] != true)
        .filter_map(|item| {
            item["sku_id"]
                .as_str()
                .or_else(|| item["sku"]["id"].as_str())
                .map(str::to_owned)
        })
        .collect()
}

/// Every item of a wishlist answer (including owned items), as `(sku_id, is_owned)`.
pub(super) fn wishlist_entries(body: &Value) -> Vec<(String, bool)> {
    body["wishlist_items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let sku = item["sku_id"]
                .as_str()
                .or_else(|| item["sku"]["id"].as_str())?
                .to_owned();
            Some((sku, item["is_owned"] == true))
        })
        .collect()
}

/// What a managed badge list shows: how many badges, and the overflow count (`D ? [S.slice(0,6)]`, `M`).
pub(super) fn managed_badges(count: usize, managed: bool) -> (usize, usize) {
    if managed {
        (
            count.min(MANAGED_BADGES),
            count.saturating_sub(MANAGED_BADGES),
        )
    } else {
        (count, 0)
    }
}

impl Root {
    pub(super) fn sidebar_redesign(&self) -> bool {
        self.stores.experiments.variant(SIDEBAR_REDESIGN) == Some(1)
    }

    fn full_profile_button_hidden(&self) -> bool {
        self.stores.experiments.variant(HIDE_FULL_PROFILE_BUTTON) == Some(1)
    }

    pub(super) fn badge_management(&self) -> bool {
        matches!(
            self.stores.experiments.variant(BADGE_MANAGEMENT),
            Some(1 | 2)
        )
    }

    /// The route `oD` reads the DM recipient's profile with.
    pub(super) fn dm_profile_route(&self, user: Id) -> String {
        let bot = self.stores.users.get(&user).is_some_and(|known| known.bot);
        sidebar_route(user, bot, self.sidebar_redesign())
    }

    /// The first two colours of the user's avatar palette, once the avatar has loaded.
    pub(super) fn avatar_palette_pair(&self, user: &User) -> Option<[u32; 2]> {
        let key = avatar_source(user).0;
        let picture = self.images.get(&key)?;
        profile_theme::palette_pair(&profile_theme::cached_palette(&key, picture))
    }

    /// The profile's theme on a surface, or `None` for an unthemed card. `sidebar` is the DM sidebar, whose theme is
    /// the viewer's (`themeOverride`).
    pub(super) fn profile_theme(
        &self,
        user: Id,
        guild: Option<Id>,
        sidebar: bool,
    ) -> Option<ProfileTheme> {
        let body = self.fetched.get(&format!("profile:{user}"))?;
        let known = self.stores.users.get(&user);
        let palette = known.and_then(|known| self.avatar_palette_pair(known));
        let colours = profile_theme::profile_colours(body, palette, guild)?;
        let viewer = super::theme();
        Some(ProfileTheme::new(
            colours,
            sidebar.then_some(viewer),
            viewer,
            self.pref_on("sync_profile_themes", false),
        ))
    }

    pub(super) fn profile_palette(&self, user: Id, guild: Option<Id>, sidebar: bool) -> Palette {
        match self.profile_theme(user, guild, sidebar) {
            Some(theme) => Palette {
                top: theme.primary,
                bottom: theme.secondary,
                overlay: theme.overlay.into(),
                text: theme.token("--text-default").into(),
                subtle: theme.token("--interactive-text-default").into(),
                themed: true,
                theme: Some(theme),
            },
            None => Palette {
                top: color("--chat-background-default"),
                bottom: color("--chat-background-default"),
                overlay: hsla(0., 0., 0., 0.),
                text: color("--text-default").into(),
                subtle: color("--interactive-text-default").into(),
                themed: false,
                theme: None,
            },
        }
    }

    pub(super) fn dm_palette(&self, user: Id) -> Palette {
        self.profile_palette(user, None, true)
    }

    /// The banner's fill (module 915614): `primaryColor` (`themeColors[0] ?? accentColor`), else the avatar's first
    /// palette colour, `PRIMARY_800` while that loads.
    pub(super) fn profile_banner_fill(&self, user: &User, guild: Option<Id>) -> Rgba {
        let body = self.fetched.get(&format!("profile:{}", user.id));
        let listed = body.and_then(|body| {
            let theme = |profile: &Value| profile["theme_colors"][0].as_u64();
            let for_guild = guild.is_some_and(|g| {
                body["guild_member_profile"]["guild_id"].as_str() == Some(g.to_string().as_str())
            });
            for_guild
                .then(|| theme(&body["guild_member_profile"]))
                .flatten()
                .or_else(|| theme(&body["user_profile"]))
                .or_else(|| body["user_profile"]["accent_color"].as_u64())
                .or_else(|| body["user"]["accent_color"].as_u64())
        });
        if let Some(value) = listed {
            return rgba(((value as u32) << 8) | 0xff);
        }
        match self.avatar_palette_pair(user) {
            Some([first, _]) => rgba((first << 8) | 0xff),
            None => color("--primary-800"),
        }
    }

    /// The bio as markdown in `body-sm/normal`: 14px on 18px lines, links in `--text-link` and clickable.
    pub(super) fn bio(&self, bio: &str, text: Hsla, cx: &mut Context<Self>) -> impl IntoElement {
        let blocks = dc_markdown::parse(bio);
        div().flex().flex_col().children(blocks.iter().map(|block| {
            let spans = super::block_spans(block).to_vec();
            let pieces = spans
                .iter()
                .map(|span| self.piece(span, text, 14., 400., 18.))
                .collect();
            let links = super::link_runs(&spans);
            let root = cx.weak_entity();
            crate::inline::inline(pieces, px(18.)).on_piece_click(move |piece, _, cx| {
                if let Some(Some((url, label))) = links.get(piece).cloned() {
                    let _ = root.update(cx, |this, cx| this.follow_link(&url, &label, cx));
                }
            })
        }))
    }

    /// `avatars_NbMTpy` at size 16: up to three mutual friends, each overlapping the next by 3px (module 791556).
    pub(super) fn mutual_avatars(&self, body: Option<&serde_json::Value>) -> impl IntoElement {
        let users: Vec<dc_state::User> = body
            .and_then(|body| body["mutual_friends"].as_array())
            .into_iter()
            .flatten()
            .filter_map(mutual_user)
            .take(3)
            .collect();
        let last = users.len().saturating_sub(1);
        div()
            .flex()
            .items_center()
            .children(users.into_iter().enumerate().map(|(index, user)| {
                let picture = self.images.get(&avatar_source(&user).0).cloned();
                div()
                    .size(px(16.))
                    .rounded_full()
                    .when(index != last, |this| this.mr(px(-3.)))
                    .children(picture.map(|picture| img(picture).size(px(16.)).rounded_full()))
            }))
    }

    /// `container_IqWmhA managed_IqWmhA` (module 567556): the badges in a pill, six and a "+N" overflow button when
    /// badge management is on. `None` without badges.
    pub(super) fn badge_pill(
        &self,
        place: &'static str,
        user: Id,
        palette: &Palette,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let mut badges = self.profile_badges(place, user, cx);
        if badges.is_empty() {
            return None;
        }
        let (shown, overflow) = managed_badges(badges.len(), self.badge_management());
        badges.truncate(shown);
        let username = self
            .stores
            .users
            .get(&user)
            .map(|known| known.display_name().to_owned());
        let hover = palette.pill_background(true);
        let overflow_button = (overflow > 0).then(|| {
            // `overflowSlot_IqWmhA overflowButton_IqWmhA`: "+N" in `body-sm/normal` `text-default`; it opens the badge
            // directory (`openBadgeDirectoryModal`) for this user.
            let tip = Tip::body(
                super::tooltip::Body::Text(dc_intl::t!("eIHfGZ", overflow_count = overflow).into()),
                Side::Top,
            )
            .after(300);
            let target = div()
                .id(SharedString::from(format!("{place}-badge-overflow-{user}")))
                .h(px(20.))
                .min_w(px(20.))
                .px(px(2.))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .text_size(px(14.))
                .line_height(px(18.))
                .text_color(palette.token("--text-default"))
                .child(dc_intl::t!("n7CiSK", count = overflow))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.open_badge_modal(Some(user), username.clone(), None, false, cx);
                }));
            self.with_tip(
                format!("{place}-badge-overflow-tip-{user}"),
                tip,
                target,
                cx,
            )
            .into_any_element()
        });
        Some(
            div()
                .id(SharedString::from(format!("{place}-badges-{user}")))
                .parity(match place {
                    "popout" => "popout-badges",
                    "full-profile" => "full-profile-badges",
                    "profile-editor" => "profile-editor-badges",
                    _ => "dm-profile-badges",
                })
                // `.container_IqWmhA`: 20px icons + 1px padding + 1px border = 24px outer height.
                .min_h(px(24.))
                .mx(px(-1.))
                .px(px(3.))
                .py(px(1.))
                .border_1()
                .border_color(palette.token("--border-muted"))
                .rounded(px(8.))
                .bg(palette.pill_background(false))
                .hover(move |this| this.bg(hover))
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(2.))
                .children(badges)
                .children(overflow_button)
                .into_any_element(),
        )
    }

    /// Server tag chiplet matching the 24px profile badge pill border and surface.
    pub(super) fn profile_server_tag_pill(
        &self,
        tag: &dc_state::ServerTag,
        palette: &Palette,
    ) -> gpui::Div {
        let badge = tag
            .badge_url()
            .and_then(|url| self.images.get(&url).cloned());
        div()
            .h(px(24.))
            .px(px(6.))
            .border_1()
            .border_color(palette.token("--border-muted"))
            .rounded(px(8.))
            .bg(palette.pill_background(false))
            .flex()
            .items_center()
            .gap(px(4.))
            .children(badge.map(|image| img(image).size(px(14.)).rounded(px(3.))))
            .child(
                div()
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .font_weight(FontWeight(600.))
                    .text_color(palette.token("--text-default"))
                    .child(tag.tag.clone()),
            )
    }

    /// The pill as the official profiles draw it (module 534400 without `disableGuildProfile`): a press opens the
    /// server's tag card beside it. The lists (direct messages, members, voice) draw the plain chip.
    pub(super) fn profile_server_tag_button(
        &self,
        tag: &dc_state::ServerTag,
        palette: &Palette,
        key: String,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let guild = tag.guild_id;
        let anchor_key = key.clone();
        self.profile_server_tag_pill(tag, palette)
            .id(gpui::SharedString::from(key.clone()))
            .relative()
            .cursor_pointer()
            .child(self.anchor_probe(key))
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                let anchor = this.tip_anchors.borrow().get(anchor_key.as_str()).copied();
                if let Some(anchor) = anchor {
                    this.open_guild_tag_popout(guild, anchor, cx);
                }
            }))
    }

    /// `displayNameRow` and `usernameAndPronounsRow` of module 465829, with the note button (module 243166) after the
    /// name in the sidebar.
    fn profile_names(
        &self,
        place: &'static str,
        user: &User,
        guild: Option<Id>,
        palette: &Palette,
        note_visible: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let user_id = user.id;
        let body = self.fetched.get(&format!("profile:{user_id}"));
        let pronouns = body
            .and_then(|body| {
                let for_guild = guild.is_some_and(|g| {
                    body["guild_member_profile"]["guild_id"].as_str()
                        == Some(g.to_string().as_str())
                });
                for_guild
                    .then(|| {
                        body["guild_member_profile"]["pronouns"]
                            .as_str()
                            .filter(|pronouns| !pronouns.is_empty())
                    })
                    .flatten()
                    .or_else(|| body["user_profile"]["pronouns"].as_str())
            })
            .filter(|pronouns| !pronouns.is_empty())
            .map(str::to_owned);
        let has_pronouns = pronouns.is_some();
        let note = Some(self.stores.notes.text_of(user_id))
            .filter(|note| !note.is_empty())
            .map(str::to_owned);
        let note_button = {
            let (glyph, label) = match &note {
                Some(_) => ("profile_note", dc_intl::t!("gs+qcM")),
                None => ("profile_note_add", dc_intl::t!("1ZZtts")),
            };
            let text: SharedString = note.clone().unwrap_or_else(|| label.to_owned()).into();
            let tip = Tip::body(super::tooltip::Body::Text(text), Side::Top);
            let strong = palette.token("--icon-strong");
            let target = div()
                .id(SharedString::from(format!("{place}-note-{user_id}")))
                .size(px(24.))
                .p(px(4.))
                .cursor_pointer()
                .when(!note_visible, |this| this.opacity(0.))
                .child(
                    gpui::svg()
                        .path(format!("icons/{glyph}.svg"))
                        .size(px(16.))
                        .text_color(palette.token("--icon-subtle"))
                        .hover(move |this| this.text_color(strong)),
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.open_full_profile(user_id, window, cx);
                }));
            self.with_tip(format!("{place}-note-tip-{user_id}"), tip, target, cx)
        };
        let open = move |id: &'static str, cx: &mut Context<Self>| {
            div()
                .id(SharedString::from(format!("{place}-{id}-{user_id}")))
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.open_full_profile(user_id, window, cx);
                }))
        };
        let strong = palette.token("--text-strong");
        let default = palette.token("--text-default");
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(24.))
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(
                        open("name", cx)
                            .min_w(px(0.))
                            .text_size(px(20.))
                            .line_height(px(24.))
                            .font_weight(FontWeight(600.))
                            .text_color(default)
                            .child(super::name_styles::name(
                                format!("{place}-name-{user_id}"),
                                self.dm_display_name(user),
                                self.name_styles(user, None).as_ref(),
                                super::name_styles::Display::Animated,
                            )),
                    )
                    .child(div().flex_none().self_start().child(note_button)),
            )
            .child(
                div()
                    .min_h(px(24.))
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_y(px(2.))
                    .gap_x(px(if has_pronouns { 6. } else { 9.33 }))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(strong)
                    .child(open("username", cx).child(user.username.clone()))
                    .when_some(pronouns, |this, pronouns| {
                        let tip = Tip::text(dc_intl::tr("Pronouns"), Side::Top).after(300);
                        let target = div()
                            .id(SharedString::from(format!("{place}-pronouns-{user_id}")))
                            .mr(px(2.))
                            .child(pronouns);
                        // `dotSpacer_C2XpHA`: 4px in `--text-default`, 1px of margin each side.
                        this.child(div().mx(px(1.)).size(px(4.)).rounded_full().bg(default))
                            .child(self.with_tip(
                                format!("{place}-pronouns-tip-{user_id}"),
                                tip,
                                target,
                                cx,
                            ))
                    })
                    .children(user.tag.as_ref().map(|tag| {
                        if let Some(url) = tag.badge_url()
                            && !self.images.contains_key(&url)
                        {
                            self.want_image(url.clone(), url, cx);
                        }
                        self.profile_server_tag_button(
                            tag,
                            palette,
                            format!("{place}-tag-{user_id}"),
                            cx,
                        )
                    }))
                    .children(self.badge_pill(place, user_id, palette, cx)),
            )
    }

    /// `mutuals_PXYMZ1` (module 791556): avatars and the friends' count, a dot, the servers' count; `body-xs` when both
    /// show, `body-sm` for one. Each opens the full profile on its tab.
    fn profile_mutuals(
        &self,
        place: &'static str,
        user: Id,
        palette: &Palette,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let body = self.fetched.get(&format!("profile:{user}"));
        for mutual in body
            .and_then(|body| body["mutual_friends"].as_array())
            .into_iter()
            .flatten()
            .filter_map(mutual_user)
            .take(3)
        {
            let (key, url) = avatar_source(&mutual);
            if !self.images.contains_key(&key) {
                self.want_image(key, url, cx);
            }
        }
        let friends = body
            .and_then(|body| {
                body["mutual_friends"]
                    .as_array()
                    .map(Vec::len)
                    .or_else(|| body["mutual_friends_count"].as_u64().map(|n| n as usize))
            })
            .unwrap_or(0);
        let servers = body
            .and_then(|body| body["mutual_guilds"].as_array().map(Vec::len))
            .unwrap_or(0);
        if friends == 0 && servers == 0 {
            return None;
        }
        let both = friends > 0 && servers > 0;
        let (size, line) = if both { (12., 16.) } else { (14., 18.) };
        let subtle = palette.subtle;
        let section = |id: String| {
            div()
                .id(SharedString::from(id))
                .py(px(4.))
                .flex()
                .items_center()
                .gap(px(4.))
                .cursor_pointer()
                .text_size(px(size))
                .line_height(px(line))
                .text_color(subtle)
        };
        Some(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap_x(px(6.))
                .gap_y(px(2.))
                .when(friends > 0, |this| {
                    this.child(
                        section(format!("{place}-mutual-friends"))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.profile = None;
                                this.open_full_profile_tab(
                                    user,
                                    super::full_profile::Tab::Friends,
                                    window,
                                    cx,
                                )
                            }))
                            .child(self.mutual_avatars(body))
                            // Module 791556: the count through 837531 (`5s9jl+`).
                            .child(dc_intl::t!("5s9jl+", count = friends)),
                    )
                })
                .when(both, |this| {
                    this.child(div().size(px(4.)).rounded_full().bg(subtle))
                })
                .when(servers > 0, |this| {
                    this.child(
                        section(format!("{place}-mutual-servers"))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.profile = None;
                                this.open_full_profile_tab(
                                    user,
                                    super::full_profile::Tab::Servers,
                                    window,
                                    cx,
                                )
                            }))
                            // Module 791556: the count through 186272 (`eE3oep`).
                            .child(dc_intl::t!("eE3oep", count = servers)),
                    )
                })
                .into_any_element(),
        )
    }

    /// Every `/games` record that has arrived for a profile's widget games, over all its batches.
    pub(super) fn profile_game_records(&self, user: Id) -> Vec<&Value> {
        let count = self
            .fetched
            .get(&format!("profile:{user}"))
            .map_or(0, |body| widget_games(body).len());
        (0..count.div_ceil(GAMES_PER_FETCH).max(1))
            .filter_map(|chunk| self.fetched.get(&profile_games_key(user, chunk)))
            .filter_map(Value::as_array)
            .flatten()
            .collect()
    }

    /// The games of the collection breadcrumb, once their answer is in: `(id, name, image key)`.
    fn breadcrumb_games(&self, user: Id) -> (Vec<(String, String, String)>, usize) {
        let Some(body) = self.fetched.get(&format!("profile:{user}")) else {
            return (Vec::new(), 0);
        };
        let ids = widget_games(body);
        let records = self.profile_game_records(user);
        let games: Vec<(String, String, String)> = ids
            .iter()
            .filter_map(|id| {
                let game = records
                    .iter()
                    .find(|game| game["id"].as_str() == Some(id))?;
                game_icon_hash(game)?;
                Some((
                    id.clone(),
                    game["name"].as_str().unwrap_or_default().to_owned(),
                    format!("profile-game:{id}"),
                ))
            })
            .collect();
        (games, ids.len())
    }

    /// Asks for the game data, icons and wishlist a profile's cards need (module 312294's `x`, the wishlist read of
    /// `fw`), each once.
    pub(super) fn request_profile_cards(&mut self, user: Id) {
        let Some(body) = self.fetched.get(&format!("profile:{user}")).cloned() else {
            return;
        };
        let games = widget_games(&body);
        // 920495 asks for every id, twenty at a time.
        for (chunk, ids) in games.chunks(GAMES_PER_FETCH).enumerate() {
            let key = profile_games_key(user, chunk);
            if self.requested.insert(key.clone()) {
                let _ = self.requests.send(Request::Fetch {
                    key,
                    route: games_route(ids),
                });
            }
        }
        let pictures: Vec<(String, String)> = self
            .profile_game_records(user)
            .into_iter()
            .flat_map(|game| {
                let id = game["id"].as_str().unwrap_or_default();
                let icon = game_icon_hash(game).map(|icon| {
                    (
                        super::profile_board::icon_key(id),
                        format!("https://cdn.discordapp.com/app-icons/{id}/{icon}.png?size=48"),
                    )
                });
                // `getCoverURL` of module 997013 (the Board's covers, module 201438).
                let cover = super::inventory_popout::game_cover_url(game)
                    .map(|url| (super::profile_board::cover_key(id), url));
                [icon, cover]
                    .into_iter()
                    .flatten()
                    .filter(|_| !id.is_empty())
            })
            .collect();
        for (key, url) in pictures {
            self.request_image(key, url);
        }
        if let Some(url) = self
            .stores
            .users
            .get(&user)
            .and_then(|known| known.tag.as_ref())
            .and_then(|tag| tag.badge_url())
        {
            self.request_image(url.clone(), url);
        }
        let widget_urls: Vec<String> = super::profile_editor::save::parse_widgets(&body)
            .into_iter()
            .flat_map(|widget| match widget.data {
                super::profile_editor::save::WidgetData::FavoriteGames { games }
                | super::profile_editor::save::WidgetData::CurrentGames { games }
                | super::profile_editor::save::WidgetData::WantToPlayGames { games }
                | super::profile_editor::save::WidgetData::PlayedGames { games } => games
                    .into_iter()
                    .flat_map(|g| {
                        let resolved = self.profile_editor_resolve_game(&g.game_id);
                        resolved.cover.into_iter().chain(resolved.icon)
                    })
                    .collect::<Vec<_>>(),
                super::profile_editor::save::WidgetData::Application { application_id } => {
                    super::profile_editor::widgets::featured_app(&application_id)
                        .map(|(_, url)| vec![url])
                        .unwrap_or_default()
                }
                _ => Vec::new(),
            })
            .collect();
        for url in widget_urls {
            self.request_image(url.clone(), url);
        }
        // `getFirstWishlistId`: the first key of `wishlist_settings`.
        if let Some(wishlist) = super::shop::data::wishlist_ids(&body).into_iter().next() {
            let key = format!("profile-wishlist:{user}");
            if self.requested.insert(key.clone()) {
                let _ = self.requests.send(Request::Fetch {
                    key,
                    route: super::shop::data::wishlist_route(&wishlist),
                });
            }
        }
        if let Some(list) = self.fetched.get(&format!("profile-wishlist:{user}")) {
            let all_skus: Vec<String> = wishlist_entries(list)
                .into_iter()
                .map(|(sku, _)| sku)
                .collect();
            self.shop_need_products(&all_skus);
            self.editor_wishlist_need(&all_skus);
        }
    }

    /// The collection breadcrumb (`eo` of module 312294): "Game Collection" and up to four game icons, the last one
    /// counting the rest. Pressing it opens the full profile's Board.
    fn game_collection(
        &self,
        place: &str,
        user: Id,
        palette: &Palette,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let (games, total) = self.breadcrumb_games(user);
        let body = self.fetched.get(&format!("profile:{user}"))?;
        if widget_games(body).is_empty() {
            return None;
        }
        let cards = widget_card_count(body);
        let shown: Vec<_> = games.into_iter().take(BREADCRUMB_GAMES).collect();
        let last = shown.len().saturating_sub(1);
        let rest = total.saturating_sub(shown.len());
        let light_card = palette
            .theme
            .map_or(super::theme() == dc_tokens::Theme::Light, |theme| {
                theme.light()
            });
        let tile = palette.token(if palette.themed {
            if light_card {
                "--opacity-black-32"
            } else {
                "--opacity-white-24"
            }
        } else {
            "--background-mod-strong"
        });
        let icons = shown.into_iter().enumerate().map(|(index, (_, _, key))| {
            let counted = index == last && total > BREADCRUMB_GAMES;
            div()
                .relative()
                .size(px(24.))
                .rounded(px(4.))
                .bg(tile)
                .children(self.images.get(&key).cloned().map(|picture| {
                    img(picture)
                        .size(px(24.))
                        .rounded(px(4.))
                        .when(counted, |this| this.opacity(0.5))
                }))
                .when(counted, |this| {
                    this.child(
                        div()
                            .absolute()
                            .top_0()
                            .left_0()
                            .size(px(24.))
                            .rounded(px(4.))
                            .bg(hsla(0., 0., 0., 0.6))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--neutral-9"))
                            .child(format!("+{rest}")),
                    )
                })
        });
        let overlay = palette.token("--user-profile-overlay-background");
        let hover = palette.token("--user-profile-overlay-background-hover");
        let card = div()
            .id(SharedString::from(format!("{place}-game-collection")))
            .parity("dm-profile-game-collection")
            .relative()
            .cursor_pointer()
            .h(px(40.))
            .p(px(8.))
            .rounded(px(8.))
            .bg(overlay)
            .hover(move |this| this.bg(hover))
            // `--shadow-low` on the first of several cards.
            .when(cards > 1, |this| {
                this.shadow(vec![gpui::BoxShadow {
                    color: hsla(0., 0., 0., if light_card { 0.08 } else { 0.14 }),
                    offset: gpui::point(px(0.), px(1.)),
                    blur_radius: px(4.),
                    spread_radius: px(0.),
                    inset: false,
                }])
            })
            .flex()
            .items_center()
            .justify_between()
            .child(
                div()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(500.))
                    .text_color(palette.token("--text-strong"))
                    .child(dc_intl::t!("JjiwFx")),
            )
            .child(div().flex().items_center().gap(px(4.)).children(icons))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.open_full_profile_tab(user, super::full_profile::Tab::Board, window, cx);
            }));
        // `container_M2WrKU`: with more than one card a background card peeks 8px under the first.
        Some(
            div()
                .relative()
                .when(cards > 1, |this| this.pb(px(8.)))
                .when(cards > 1, |this| {
                    this.child(
                        div()
                            .absolute()
                            .left(px(8.))
                            .right(px(8.))
                            .bottom_0()
                            .h(px(8.))
                            .rounded_b(px(8.))
                            .bg(overlay),
                    )
                })
                .child(card)
                .into_any_element(),
        )
    }

    /// The wishlist breadcrumb (`of` of module 289287): "Wishlist", "View All" past three items, and up to three
    /// unowned items. The heading opens the full profile's Wishlist; an item opens its product.
    fn wishlist_breadcrumb(
        &mut self,
        place: &str,
        user: Id,
        palette: &Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if self.stores.users.get(&user).is_some_and(|known| known.bot) {
            return None;
        }
        let items = unowned_wishlist(self.fetched.get(&format!("profile-wishlist:{user}"))?);
        if items.is_empty() {
            return None;
        }
        let more = items.len() > BREADCRUMB_ITEMS;
        let mut cards = Vec::new();
        for sku in items.into_iter().take(BREADCRUMB_ITEMS) {
            let preview = self.editor_wishlist_preview(&sku, WISHLIST_CARD, window, cx);
            let tile = palette.token("--user-profile-overlay-background");
            cards.push(
                div()
                    .id(SharedString::from(format!("{place}-wishlist-{sku}")))
                    .size(px(WISHLIST_CARD))
                    .flex_none()
                    .rounded(px(8.))
                    .overflow_hidden()
                    .bg(tile)
                    .cursor_pointer()
                    .children(preview)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.shop_open_product(&sku, cx);
                    }))
                    .into_any_element(),
            );
        }
        let view_all = more.then(|| {
            div()
                .id(SharedString::from(format!("{place}-wishlist-all")))
                .h(px(24.))
                .px(px(8.))
                .rounded(px(8.))
                .border_1()
                .border_color(palette.token("--border-muted"))
                .bg(palette.token("--control-secondary-background-default"))
                .cursor_pointer()
                .flex()
                .items_center()
                .text_size(px(12.))
                .line_height(px(16.))
                .text_color(palette.token("--control-secondary-text-default"))
                .child(dc_intl::t!("y6PSA3"))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.open_full_profile_tab(
                        user,
                        super::full_profile::Tab::Wishlist,
                        window,
                        cx,
                    );
                }))
        });
        // `cardsContainer_78NJ3W`: `gap: calc(50% - size * 3 / 2)` of the 252px row.
        let gap = (252. / 2. - WISHLIST_CARD * 3. / 2.).max(0.);
        Some(
            div()
                .id(SharedString::from(format!("{place}-wishlist")))
                .parity("dm-profile-wishlist")
                .p(px(8.))
                .rounded(px(8.))
                .bg(palette.token("--user-profile-overlay-background"))
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .id(SharedString::from(format!("{place}-wishlist-heading")))
                                .cursor_pointer()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .font_weight(FontWeight(500.))
                                .text_color(palette.token("--text-strong"))
                                .child(dc_intl::t!("7lZ31J"))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.open_full_profile_tab(
                                        user,
                                        super::full_profile::Tab::Wishlist,
                                        window,
                                        cx,
                                    );
                                })),
                        )
                        .children(view_all),
                )
                .child(div().flex().gap(px(gap)).children(cards))
                .into_any_element(),
        )
    }

    /// The 80px avatar with its status cut out of a ring the card's colour shows through (module 718019), over the
    /// banner's cut-out (`cutout` of module 915614: radius 46, `avatarOffsetX - themePadding` in, `avatarOffsetY +
    /// themePadding` up from the banner's bottom).
    fn sidebar_avatar(
        &self,
        user: &User,
        palette: &Palette,
        left: f32,
        top: f32,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let (avatar_key, avatar_url) = avatar_source(user);
        if !self.images.contains_key(&avatar_key) {
            self.want_image(avatar_key, avatar_url, cx);
        }
        let user_id = user.id;
        let presence = self.stores.status(user_id).to_owned();
        let ring = div()
            .absolute()
            .left(px(left - 6.))
            .top(px(top - 6.))
            .size(px(92.))
            .rounded_full()
            .bg(palette.top)
            .child(div().size_full().rounded_full().bg(palette.overlay));
        let avatar = div()
            .id("dm-profile-avatar")
            .parity("dm-profile-avatar")
            .absolute()
            .left(px(left))
            .top(px(top))
            .cursor_pointer()
            .on_click(
                cx.listener(move |this, _, window, cx| this.open_full_profile(user_id, window, cx)),
            )
            .child(self.profile_avatar(user, &presence, cx));
        // gpui places an absolute child against its own parent, so the pair is laid over the whole header.
        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .child(ring)
            .child(avatar)
    }

    pub(super) fn dm_profile_width(&self, viewport_width: f32) -> f32 {
        if viewport_width < 1132. || !self.show_dm_profile || self.dm_call_shown() {
            return 0.;
        }
        let Some(channel) = self.current_dm() else {
            return 0.;
        };
        let Some(user) = self.stores.dm_recipient(channel) else {
            return 0.;
        };
        let ignored = !self.is_blocked(user.id) && self.stores.is_ignored(user.id);
        let gated = (self.is_blocked(user.id)
            || (ignored && !self.stores.ignore_speedbump_disabled))
            && !self.shown_blocked_profiles.contains(&user.id);
        if gated {
            return 320.;
        }
        if self.sidebar_redesign() {
            if self.fetched.contains_key(&format!("profile:{}", user.id)) {
                320.
            } else {
                0.
            }
        } else {
            340.
        }
    }

    pub(super) fn dm_profile(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if f32::from(window.viewport_size().width) < 1132.
            || !self.show_dm_profile
            || self.dm_call_shown()
        {
            return None;
        }
        let channel = self.current_dm()?;
        let user = self.stores.dm_recipient(channel)?.clone();
        // Blocked, or ignored while the speedbump is on: "Show Profile?" first.
        let ignored = !self.is_blocked(user.id) && self.stores.is_ignored(user.id);
        let gated = (self.is_blocked(user.id)
            || (ignored && !self.stores.ignore_speedbump_disabled))
            && !self.shown_blocked_profiles.contains(&user.id);
        if gated {
            return Some(
                div()
                    .parity("dm-profile")
                    .flex_none()
                    .w(px(320.))
                    .h_full()
                    .p(px(8.))
                    .bg(color("--background-base-lower"))
                    .child(
                        div()
                            .size_full()
                            .rounded(px(8.))
                            .bg(color("--background-base-lowest"))
                            .child(self.blocked_profile_gate(&user, ignored, cx)),
                    )
                    .into_any_element(),
            );
        }
        let redesign = self.sidebar_redesign();
        // `oD` draws nothing in the redesign until the profile has loaded (no skeleton, no jump).
        let loaded = self.fetched.contains_key(&format!("profile:{}", user.id));
        if redesign && !loaded {
            return None;
        }
        self.request_profile_cards(user.id);
        if redesign {
            Some(self.dm_profile_redesign(&user, window, cx))
        } else {
            Some(self.dm_profile_classic(&user, window, cx))
        }
    }

    fn sidebar_parts(
        &mut self,
        user: &User,
        palette: &Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> SidebarParts {
        let user_id = user.id;
        let body = self.fetched.get(&format!("profile:{user_id}"));
        let bio = body
            .and_then(|body| body["user_profile"]["bio"].as_str())
            .filter(|bio| !bio.is_empty())
            .map(str::to_owned);
        let is_private = body.is_some_and(|body| body["private"] == true);
        let hovering = self.profile_effect_hovered("dm-profile");
        let names = self
            .profile_names("dm-profile", user, None, palette, hovering, cx)
            .into_any_element();
        let mutuals = self.profile_mutuals("dm-profile", user_id, palette, cx);
        let private =
            is_private.then(|| super::profile_board::private_notice(user.display_name(), palette));
        let bio = bio.map(|bio| {
            self.bio(&bio, palette.token("--text-strong").into(), cx)
                .into_any_element()
        });
        // `rK.A` heading `text-strong` over `rq.A` (module 743987) in `body-sm/normal`.
        let member_since = div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .h(px(16.))
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .font_weight(FontWeight(600.))
                    .text_color(palette.token("--text-strong"))
                    // Module 289287: `a6XYD9`, `A//N4k` for a bot.
                    .child(if user.bot {
                        dc_intl::t!("A//N4k")
                    } else {
                        dc_intl::t!("a6XYD9")
                    }),
            )
            .child(
                div()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(palette.token("--text-default"))
                    .child(member_since(user_id)),
            )
            .into_any_element();
        // Not in the pinned client: the VoiceStats plugin's "Voice Time" (`plugin_hooks/voice_stats.rs`).
        let voice_time = self.voice_time_section(
            user_id,
            true,
            palette.token("--text-strong").into(),
            palette.token("--text-default").into(),
        );
        let games = self.game_collection("dm-profile", user_id, palette, cx);
        let activity = self.live_activity_cards(
            user_id,
            super::live_activity::Surface::Sidebar,
            palette.text,
            palette.subtle,
            cx,
        );
        let wishlist = self.wishlist_breadcrumb("dm-profile", user_id, palette, window, cx);
        SidebarParts {
            names,
            mutuals,
            private,
            bio,
            member_since,
            voice_time,
            games,
            activity,
            wishlist,
        }
    }

    fn sidebar_effect(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
        user: Id,
        width: f32,
    ) -> Option<AnyElement> {
        self.profile_effect_sku(user).and_then(|sku| {
            let hovering = self.profile_effect_hovered("dm-profile");
            self.profile_effect_layer(
                "dm-profile",
                sku,
                super::profile_effects::Surface::profile(width, hovering),
                window,
                cx,
            )
        })
    }

    /// The redesigned sidebar (`oM` with `isRedesignEnabled`).
    fn dm_profile_redesign(
        &mut self,
        user: &User,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let user_id = user.id;
        let palette = self.dm_palette(user_id);
        let parts = self.sidebar_parts(user, &palette, window, cx);
        let banner = self.profile_banner_fill(user, None);
        // `.custom-user-profile-theme.user-profile-sidebar.user-profile-sidebar-redesign :where(.avatar)`: 14/55 with
        // the 2px theme padding, 16/57 without it.
        let (left, top) = if palette.themed {
            (14., 55.)
        } else {
            (16., 57.)
        };
        let header = div()
            .relative()
            .h(px(144.))
            .mb(px(8.))
            .child(
                div()
                    .h(px(105.))
                    .overflow_hidden()
                    .relative()
                    .bg(banner)
                    .children(
                        self.profile_banner_image(user_id)
                            .map(|image| img(image).size_full().object_fit(gpui::ObjectFit::Cover)),
                    )
                    .children(self.live_banner(user_id, 300., 105., cx)),
            )
            .child(self.sidebar_avatar(user, &palette, left, top, cx));
        // `wrapper_xpwejc`: 8px from the top and the end, above the profile effect.
        let banner_buttons = div()
            .absolute()
            .top(px(8.))
            .right(px(8.))
            .flex()
            .gap(px(8.))
            .child(self.profile_friend_button(user_id, cx))
            .child(self.profile_more_button(user_id, cx));
        let effect = self.sidebar_effect(window, cx, user_id, 300.);
        let has_cards =
            parts.games.is_some() || !parts.activity.is_empty() || parts.wishlist.is_some();
        // `body_A7qlid`: 16px in from each side, 20px between sections in the redesign.
        let body = div()
            .mx(px(16.))
            .flex()
            .flex_col()
            .gap(px(20.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(parts.names)
                    .children(parts.mutuals),
            )
            .children(parts.private)
            .children(parts.bio)
            .child(parts.member_since)
            .children(parts.voice_time)
            .when(has_cards, |this| {
                this.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .children(parts.games)
                        .children(parts.activity)
                        .children(parts.wishlist),
                )
            });
        let scroll = dm_profile_scroll(user_id);
        // `scrollerFade_S_xUx6`: the last 32px of the scroller fade out until the end is reached.
        let fades =
            scroll.max_offset().y > px(0.) && -scroll.offset().y < scroll.max_offset().y - px(1.);
        let fade_to = card_end(&palette);
        let transparent = Rgba { a: 0., ..fade_to };
        let scroller = div()
            .relative()
            .flex_1()
            .min_h(px(0.))
            .child(
                div()
                    .id("dm-profile-scroll")
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&scroll)
                    .on_scroll(cx.listener(|_, _, _, cx| cx.notify()))
                    .pb(px(16.))
                    .child(header)
                    .child(body),
            )
            .when(fades, |this| {
                this.child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .bottom_0()
                        .h(px(16.))
                        .bg(linear_gradient(
                            180.,
                            linear_color_stop(transparent, 0.),
                            linear_color_stop(fade_to, 1.),
                        )),
                )
            });
        let footer = (!self.full_profile_button_hidden()).then(|| {
            // `redesignFooter_YBm90d`: 72px, a secondary `md` full-width button.
            div().h(px(72.)).flex_none().p(px(16.)).child(
                div()
                    .id("dm-profile-full")
                    .parity("dm-profile-full")
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_full_profile(user_id, window, cx)
                    }))
                    .h(px(40.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(palette.token("--border-muted"))
                    .bg(palette.token("--control-secondary-background-default"))
                    .hover({
                        let hover = palette.token("--control-secondary-background-hover");
                        move |this| this.bg(hover)
                    })
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(500.))
                    .text_color(palette.token("--control-secondary-text-default"))
                    .child(dc_intl::t!("+Xp3hq")),
            )
        });
        // `--shadow-border`: white at .08 in a dark theme, black in the light one.
        let light_ring = palette
            .theme
            .map_or(super::theme() == dc_tokens::Theme::Light, |theme| {
                theme.light()
            });
        let card = div()
            .id("dm-profile-card")
            .parity("dm-profile-card")
            .on_hover(cx.listener(|this, inside: &bool, _, cx| {
                this.profile_effect_hover("dm-profile", *inside, cx);
            }))
            .w(px(304.))
            .max_h_full()
            .flex()
            .flex_col()
            .p(px(2.))
            .rounded(px(8.))
            // `box-shadow: var(--shadow-border)`: a 1px ring outside the card.
            .shadow(vec![gpui::BoxShadow {
                color: hsla(0., 0., if light_ring { 0. } else { 1. }, 0.08),
                offset: gpui::point(px(0.), px(0.)),
                blur_radius: px(0.),
                spread_radius: px(1.),
                inset: false,
            }])
            // `linear-gradient(primary 105px, secondary calc(100% - 72px))`.
            .bg(linear_gradient(
                180.,
                linear_color_stop(palette.top, 0.15),
                linear_color_stop(palette.bottom, 0.88),
            ))
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.))
                    .relative()
                    .rounded(px(6.))
                    .overflow_hidden()
                    .bg(palette.overlay)
                    .flex()
                    .flex_col()
                    .child(scroller)
                    .children(footer)
                    .children(effect)
                    .child(banner_buttons),
            );
        div()
            .parity("dm-profile")
            .flex_none()
            .w(px(320.))
            .h_full()
            .p(px(8.))
            .bg(color("--background-base-lower"))
            .flex()
            .justify_center()
            .child(card)
            .into_any_element()
    }

    /// The classic sidebar (`oM` without the redesign). No capture of it exists; its numbers are the stylesheet's:
    /// 340px wide and the column's height, a 120px banner, the 80px avatar 16px in and 72px down, the bio and Member
    /// Since in one 12px overlay box 16px in, the widget and wishlist breadcrumbs 12px below with 16px sides, and a
    /// 44px footer link under a 1px `--user-profile-border` line.
    fn dm_profile_classic(
        &mut self,
        user: &User,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let user_id = user.id;
        let palette = self.dm_palette(user_id);
        let parts = self.sidebar_parts(user, &palette, window, cx);
        let banner = self.profile_banner_fill(user, None);
        let header = div()
            .relative()
            .h(px(159.))
            .mb(px(8.))
            .child(
                div()
                    .h(px(120.))
                    .overflow_hidden()
                    .relative()
                    .bg(banner)
                    .children(
                        self.profile_banner_image(user_id)
                            .map(|image| img(image).size_full().object_fit(gpui::ObjectFit::Cover)),
                    )
                    .children(self.live_banner(user_id, 340., 120., cx)),
            )
            .child(self.sidebar_avatar(user, &palette, 16., 72., cx));
        let banner_buttons = div()
            .absolute()
            .top(px(8.))
            .right(px(8.))
            .flex()
            .gap(px(8.))
            .child(self.profile_friend_button(user_id, cx))
            .child(self.profile_more_button(user_id, cx));
        let effect = self.sidebar_effect(window, cx, user_id, 340.);
        let overlay_box = div()
            .p(px(12.))
            .rounded(px(8.))
            .bg(palette.token("--user-profile-overlay-background"))
            .flex()
            .flex_col()
            .gap(px(12.))
            .children(parts.bio.map(|bio| {
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(
                        div()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .font_weight(FontWeight(600.))
                            .text_color(palette.token("--text-strong"))
                            .child(dc_intl::t!("ZzAR2Y")),
                    )
                    .child(bio)
            }))
            .child(parts.member_since)
            .children(parts.voice_time);
        let body = div()
            .mx(px(16.))
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(parts.names)
            .children(parts.private)
            .children(parts.activity)
            .child(overlay_box);
        let footer = (!self.full_profile_button_hidden()).then(|| {
            div()
                .id("dm-profile-full")
                .parity("dm-profile-full")
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.open_full_profile(user_id, window, cx)
                }))
                .h(px(44.))
                .flex_none()
                .border_t_1()
                .border_color(palette.token("--user-profile-border"))
                // `footerButton_YBm90d`: the secondary colour under `--profile-gradient-end`.
                .bg(card_end(&palette))
                .hover({
                    let hover = palette.token("--user-profile-overlay-background-hover");
                    move |this| this.bg(hover)
                })
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(14.))
                .line_height(px(18.))
                .text_color(palette.token("--text-strong"))
                .child(dc_intl::t!("+Xp3hq"))
        });
        let scroller = div()
            .id("dm-profile-scroll")
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .pb(px(16.))
            .child(header)
            .child(body)
            .children(
                parts
                    .games
                    .map(|games| div().mt(px(12.)).mx(px(16.)).child(games)),
            )
            .children(
                parts
                    .wishlist
                    .map(|list| div().mt(px(12.)).mx(px(16.)).child(list)),
            )
            .children(parts.mutuals.map(|mutuals| {
                div()
                    .mt(px(12.))
                    .mx(px(16.))
                    .p(px(12.))
                    .rounded(px(8.))
                    .bg(palette.token("--user-profile-overlay-background"))
                    .child(mutuals)
            }));
        div()
            .id("dm-profile-card")
            .parity("dm-profile")
            .on_hover(cx.listener(|this, inside: &bool, _, cx| {
                this.profile_effect_hover("dm-profile", *inside, cx);
            }))
            .relative()
            .flex_none()
            .w(px(340.))
            .h_full()
            .bg(linear_gradient(
                180.,
                linear_color_stop(palette.top, 0.15),
                linear_color_stop(palette.bottom, 0.9),
            ))
            .child(
                div()
                    .size_full()
                    .bg(palette.overlay)
                    .flex()
                    .flex_col()
                    .child(scroller)
                    .children(footer),
            )
            .children(effect)
            .child(banner_buttons)
            .into_any_element()
    }
}

/// `--profile-gradient-end`: the overlay over the secondary colour, where the card's gradient ends.
fn card_end(palette: &Palette) -> Rgba {
    if !palette.themed {
        return palette.bottom;
    }
    let mut end = palette.bottom;
    let overlay: Rgba = palette.overlay.into();
    let keep = 1. - overlay.a;
    end.r = end.r * keep + overlay.r * overlay.a;
    end.g = end.g * keep + overlay.g * overlay.a;
    end.b = end.b * keep + overlay.b * overlay.a;
    end
}

struct SidebarParts {
    names: AnyElement,
    mutuals: Option<AnyElement>,
    /// i?.private: the notice of module 501193 (289287), under the names.
    private: Option<AnyElement>,
    bio: Option<AnyElement>,
    member_since: AnyElement,
    voice_time: Option<AnyElement>,
    games: Option<AnyElement>,
    activity: Vec<AnyElement>,
    wishlist: Option<AnyElement>,
}

/// `bannerButton_C4gRrZ`: 32px round on the banner, a 16px white icon, with custom-theme backdrop blur (675327/82171).
pub(super) fn round_button(name: &'static str) -> gpui::Div {
    round_button_themed(name, false, false)
}

pub(super) fn round_button_themed(name: &'static str, themed: bool, light: bool) -> gpui::Div {
    div()
        .size(px(32.))
        .rounded_full()
        .border_1()
        .border_color(hsla(0., 0., 1., 0.08))
        .bg(hsla(0., 0., 0., 0.56))
        .when(themed, |this| {
            this.backdrop_blur(px(if light { 24. } else { 36. }))
        })
        .flex()
        .items_center()
        .justify_center()
        .child(icon(name, 16., "--white"))
}

/// Capture only: a realistic Nitro profile for the DM's other person, shaped like the profile answer module 851767
/// reads (`premium_type`, `user_profile.theme_colors`, `badges`, `widgets`, `wishlist_settings`, mutuals), with the
/// game list and the wishlist answers it leads to. Nothing is sent; game icons are drawn locally.
#[cfg(feature = "capture")]
impl Root {
    pub fn script_experiment(&mut self, name: &str, variant: i64, cx: &mut Context<Self>) {
        self.stores.experiments.force(name, variant);
        cx.notify();
    }

    pub fn script_profile_demo(&mut self, words: &[&str], cx: &mut Context<Self>) -> String {
        let Some(user) = self
            .current_dm()
            .and_then(|dm| self.stores.dm_recipient(dm))
            .map(|known| known.id)
        else {
            return "profile demo: no DM open".to_owned();
        };
        let variant = words.first().copied().unwrap_or("themed");
        if variant == "popout" {
            // As if the name at this point of the window had been pressed (`left` opens it to the left).
            let left = words.get(1) == Some(&"left");
            let anchor = gpui::Bounds::new(
                gpui::point(px(if left { 1000. } else { 400. }), px(80.)),
                gpui::size(px(40.), px(20.)),
            );
            self.open_profile(user, anchor, left, cx);
            return "profile demo: popout open".to_owned();
        }
        let mut body = self
            .fetched
            .get(&format!("profile:{user}"))
            .cloned()
            .unwrap_or_else(|| serde_json::json!({}));
        let mutual = self
            .stores
            .users
            .values()
            .filter(|known| known.id != user)
            .take(2)
            .map(|known| serde_json::json!({"id": known.id.to_string(), "username": known.username, "avatar": known.avatar}))
            .collect::<Vec<_>>();
        let first_guild = self.stores.guilds.first().map(|guild| guild.id.to_string());
        // Seven badges, one more than a managed list shows; their pictures are drawn locally (`badge:demo-N`).
        let badge_tints = [
            0x5865f2_u32,
            0x23a55a,
            0xf0b232,
            0xeb459e,
            0x3e70dd,
            0x1abc9c,
            0xe67e22,
        ];
        let badges: Vec<Value> = (0..badge_tints.len())
            .map(|index| serde_json::json!({"id": format!("demo_{index}"), "description": format!("Demo badge {}", index + 1), "icon": format!("demo-{index}")}))
            .collect();
        let circle = |tint: u32, size: u32| {
            let picture = image::RgbaImage::from_fn(size, size, |x, y| {
                let centre = size as f32 / 2. - 0.5;
                let inside = (x as f32 - centre).hypot(y as f32 - centre) <= size as f32 / 2. - 1.;
                image::Rgba([
                    (tint >> 16) as u8,
                    (tint >> 8) as u8,
                    tint as u8,
                    if inside { 255 } else { 0 },
                ])
            });
            let mut png = Vec::new();
            let _ = picture.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png);
            std::sync::Arc::new(gpui::Image::from_bytes(gpui::ImageFormat::Png, png))
        };
        for (index, tint) in badge_tints.into_iter().enumerate() {
            self.images
                .insert(format!("badge:demo-{index}"), circle(tint, 40));
            self.requested.insert(format!("badge:demo-{index}"));
        }
        let themed = variant == "themed" || variant == "light";
        body["premium_type"] = serde_json::json!(if themed { 2 } else { 0 });
        body["user"] = serde_json::json!({"id": user.to_string()});
        body["user_profile"] = serde_json::json!({
            "bio": "i arrive in harmony",
            "pronouns": "he/him",
            "accent_color": 0x252539,
            "theme_colors": if variant == "light" { serde_json::json!([0xf2c6d8, 0x9fb7e8]) }
                else if themed { serde_json::json!([0x3a2a6b, 0x0f3f4a]) } else { Value::Null },
        });
        body["badges"] = Value::Array(badges);
        body["mutual_friends"] = Value::Array(mutual);
        body["mutual_friends_count"] = serde_json::json!(2);
        body["mutual_guilds"] = serde_json::json!(
            first_guild
                .iter()
                .map(|id| serde_json::json!({"id": id}))
                .collect::<Vec<_>>()
        );
        body["widgets"] = serde_json::json!([
            {"id": "1", "data": {"type": "favorite_games", "games": [
                {"game_id": "1001"}, {"game_id": "1002"}, {"game_id": "1003"}]}},
            {"id": "2", "data": {"type": "played_games", "games": [
                {"game_id": "1004"}, {"game_id": "1005"}, {"game_id": "1001"}]}}
        ]);
        body["wishlist_settings"] = serde_json::json!({"demo-wishlist": {"privacy": 0}});
        self.fetched.insert(format!("profile:{user}"), body);
        let names = ["ULTRAKILL", "Signalis", "Hades", "Hollow Knight", "Celeste"];
        let tints = [0x7a9a3a_u32, 0xc0392b, 0xd35400, 0x34495e, 0x8e44ad];
        let games: Vec<Value> = (0..5)
            .map(|index| serde_json::json!({"id": format!("100{}", index + 1), "name": names[index], "icon": "demo"}))
            .collect();
        self.fetched
            .insert(format!("profile-games:{user}"), Value::Array(games));
        self.requested.insert(format!("profile-games:{user}"));
        for (index, tint) in tints.into_iter().enumerate() {
            let picture = image::RgbaImage::from_fn(48, 48, |x, y| {
                let edge = x < 6 || y < 6 || x > 41 || y > 41;
                let rgb = if edge { 0x202020 } else { tint };
                image::Rgba([(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8, 255])
            });
            let mut png = Vec::new();
            let _ = picture.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png);
            self.images.insert(
                format!("profile-game:100{}", index + 1),
                std::sync::Arc::new(gpui::Image::from_bytes(gpui::ImageFormat::Png, png)),
            );
        }
        // Wishlist items: the skus named, else the first products the Shop's home answer brought (asked for here
        // when it has not been read: a second `profile demo` then finds them).
        let mut skus: Vec<String> = words.iter().skip(1).map(|sku| (*sku).to_owned()).collect();
        if skus.is_empty() {
            if self.fetched.contains_key("shop:home") {
                let mut known: Vec<String> = self.shop.products.keys().cloned().collect();
                known.sort();
                skus = known.into_iter().take(4).collect();
            } else {
                self.shop_request("shop:home", super::shop::data::shop_home_route("home"));
            }
        }
        let items: Vec<Value> = skus
            .iter()
            .map(|sku| serde_json::json!({"sku_id": sku, "is_owned": false, "sku_product_line": 1}))
            .collect();
        self.fetched.insert(
            format!("profile-wishlist:{user}"),
            serde_json::json!({"id": "demo-wishlist", "wishlist_items": items}),
        );
        self.requested.insert(format!("profile-wishlist:{user}"));
        self.request_profile_images(user);
        cx.notify();
        format!(
            "profile demo: {variant} for the DM's user, {} wishlist items",
            skus.len()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_games_record_names_its_icon_in_media_not_at_the_top() {
        // The shape `GET /games` answers with: `icon_hash` and `media.icon`, no top-level `icon`.
        let record = json!({
            "id": "5", "name": "Game", "icon_hash": "abc",
            "media": {"icon": {"type": "hash", "value": "def"},
                      "cover": {"type": "url", "value": "https://images-ext-1.discordapp.net/external/x/https/images.igdb.com/c.jpg"}}
        });
        assert_eq!(game_icon_hash(&record), Some("def"));
        assert_eq!(
            game_icon_hash(&json!({"id": "5", "icon_hash": "abc"})),
            Some("abc")
        );
        assert_eq!(
            game_icon_hash(&json!({"id": "5", "media": {"icon": {"type": "url", "value": "x"}}})),
            None
        );
        assert_eq!(game_icon_hash(&json!({"id": "5"})), None);
        let cover = crate::ui::inventory_popout::game_cover_url(&record).expect("a cover");
        assert!(
            cover.ends_with("keep_aspect_ratio=true&format=webp"),
            "{cover}"
        );
    }

    #[test]
    fn widget_games_are_fetched_twenty_at_a_time() {
        let ids: Vec<String> = (0..45).map(|n| (1000 + n).to_string()).collect();
        let routes: Vec<String> = ids.chunks(GAMES_PER_FETCH).map(games_route).collect();
        assert_eq!(routes.len(), 3);
        assert_eq!(routes[0].matches("game_ids=").count(), 20);
        assert_eq!(routes[2].matches("game_ids=").count(), 5);
        assert!(routes[1].starts_with("/games?game_ids=1020&"));
        assert_eq!(profile_games_key(7, 0), "profile-games:7");
        assert_eq!(profile_games_key(7, 2), "profile-games:7:2");
        assert!(is_profile_games_key("profile-games:7", 7));
        assert!(is_profile_games_key("profile-games:7:1", 7));
        assert!(!is_profile_games_key("profile-games:70", 7));
    }

    #[test]
    fn member_since_reads_the_snowflake() {
        // loreanxavier's account, created on 29 October 2021 in the capture.
        assert!(member_since(903_350_000_000_000_000).ends_with("2021"));
    }

    #[test]
    fn the_sidebar_reads_its_profile_as_od_does() {
        assert_eq!(
            sidebar_route(7, false, true),
            "/users/7/profile?type=sidebar&with_mutual_guilds=true&with_mutual_friends=true&with_mutual_friends_count=true"
        );
        assert!(
            sidebar_route(7, false, false)
                .contains("with_mutual_friends=false&with_mutual_friends_count=true")
        );
        assert!(
            sidebar_route(7, true, true)
                .contains("with_mutual_friends=false&with_mutual_friends_count=false")
        );
    }

    #[test]
    fn managed_badges_show_six_and_count_the_rest() {
        assert_eq!(managed_badges(7, true), (6, 1));
        assert_eq!(managed_badges(4, true), (4, 0));
        assert_eq!(managed_badges(7, false), (7, 0));
    }

    #[test]
    fn game_widgets_merge_into_one_breadcrumb() {
        let body = json!({"widgets": [
            {"data": {"type": "favorite_games", "games": [{"game_id": "1"}, {"game_id": "2"}]}},
            {"data": {"type": "played_games", "games": [{"game_id": "2"}, {"game_id": "3"}]}},
            {"data": {"type": "personal", "header": "x", "sections": []}},
            {"data": {"type": "application", "application_id": "9"}}
        ]});
        assert_eq!(widget_games(&body), ["1", "2", "3"]);
        assert_eq!(widget_card_count(&body), 2);
        assert_eq!(widget_card_count(&json!({})), 0);
    }

    #[test]
    fn the_wishlist_breadcrumb_leaves_owned_items_out() {
        let list = json!({"wishlist_items": [
            {"sku_id": "1", "is_owned": false},
            {"sku_id": "2", "is_owned": true},
            {"sku": {"id": "3"}}
        ]});
        assert_eq!(unowned_wishlist(&list), ["1", "3"]);
        assert_eq!(
            wishlist_entries(&list),
            vec![
                ("1".to_owned(), false),
                ("2".to_owned(), true),
                ("3".to_owned(), false)
            ]
        );
    }

    #[test]
    fn experiments_pick_the_sidebar_layout() {
        let mut assignments = dc_state::experiments::Assignments::default();
        assert_eq!(assignments.variant(SIDEBAR_REDESIGN), None);
        assignments.force(SIDEBAR_REDESIGN, 1);
        assert_eq!(assignments.variant(SIDEBAR_REDESIGN), Some(1));
    }
}
