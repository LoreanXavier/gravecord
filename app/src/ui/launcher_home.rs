//! The App Launcher's home in a text channel (module 621377 with surface `APP_LAUNCHER_IN_TEXT`). Below Recents it
//! lists the application directory's collections for that surface (module 503406 `An`:
//! `GET /application-directory-static/collections?surface=2&active_state=1&platform=4`, asked again after ten
//! minutes), in their `position` order and only those the desktop platform (`WEB`, 4) is in. The Promoted collection
//! (type 5) is a grid of two 224px banner cards across (`sectionTwoColumnContentContainer`); every other collection
//! shows its first four apps as 64px rows in one rounded box, with "View More" in its header when it holds more.
//! Measured in `design/dom/channel/apps-launcher-darker-cozy.json`; a recorded reply is in
//! `design/snapshot/TRAFFIC/app-launcher-collections-text.json`.

use super::activity_shelf::{Shelf, ShelfCard, shelf_key};
use super::{Root, color};
use crate::net::Request;
use gpui::{AnyElement, Context, FontWeight, IntoElement, SharedString, div, prelude::*, px, rgba};
use serde_json::Value;

/// The key of the text launcher's collections in `Root::fetched`.
pub(super) const COLLECTIONS_KEY: &str = "launcher-collections:text";
/// `t < n + 6e5`: collections younger than ten minutes are not asked for again.
const COLLECTIONS_FRESH_MS: i64 = 600_000;
/// The commands of one app (`APPLICATION_COMMAND_INDEX_APPLICATION`), which the page asks for an app this channel has
/// not installed (`queryInstallOnDemandApp` of module 264322).
pub(super) const APP_INDEX_PREFIX: &str = "launcher-app-index:";
/// `platform`: `WEB` of module 503406.
const PLATFORM_WEB: u64 = 4;
/// Rows a collection shows before "View More".
const ROWS_SHOWN: usize = 4;

/// A page opened over the launcher's home.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum LauncherPage {
    /// "View More": every app of a collection (`container_NIUjob`).
    Collection(String),
    /// An app's page (`tM`): its banner, icon, cover, overview and the commands it brings here.
    App {
        id: String,
        /// "Show More" was pressed under the description.
        expanded: bool,
    },
}

/// Module 735991 `kF`: the first letter in upper case and underscores as spaces.
fn title_case(name: &str) -> String {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) => format!("{}{}", first.to_uppercase(), chars.as_str()).replace('_', " "),
        None => String::new(),
    }
}

/// `tb`: "1-N" for a capped activity, else "Unlimited".
fn participants_label(max: Option<u64>) -> String {
    match max {
        Some(max) if max > 0 => format!("1-{max}"),
        _ => "Unlimited".to_owned(),
    }
}

/// Copy Link of the page (`eQ`): the activity's page with the account as referrer for an app with an activity (an
/// `embedded_surfaces` entry of `MAIN`, 0), else its authorize link (module 342384 `V`).
fn app_link(application: &Value, me: Option<u64>) -> String {
    let id = application["id"].as_str().unwrap_or_default();
    let activity = application["embedded_surfaces"]
        .as_array()
        .is_some_and(|surfaces| surfaces.iter().any(|surface| surface.as_u64() == Some(0)));
    if activity {
        return match me {
            Some(me) => format!("https://discord.com/activities/{id}?referrer_id={me}"),
            None => format!("https://discord.com/activities/{id}"),
        };
    }
    if let Some(custom) = application["custom_install_url"].as_str() {
        return custom.to_owned();
    }
    let mut query = format!("client_id={id}");
    let typed = application["integration_types_config"]
        .as_object()
        .is_some_and(|types| {
            types
                .values()
                .any(|config| !config["oauth2_install_params"].is_null())
        });
    if !typed {
        let params = &application["install_params"];
        if let Some(permissions) = params["permissions"].as_str() {
            query.push_str(&format!("&permissions={permissions}"));
        }
        if let Some(scopes) = params["scopes"].as_array() {
            let scopes: Vec<&str> = scopes.iter().filter_map(Value::as_str).collect();
            query.push_str(&format!("&scope={}", scopes.join("+")));
        }
    }
    format!("https://discord.com/oauth2/authorize?{query}")
}

/// One app of a collection.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Item {
    pub app_id: String,
    pub name: String,
    pub description: String,
    pub icon: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Collection {
    pub id: String,
    /// 5 is the Promoted grid; any other type lists rows.
    pub kind: u64,
    pub title: String,
    pub items: Vec<Item>,
}

/// The collections the desktop shows, in `position` order, each with its items in theirs.
pub(super) fn parse_collections(body: &Value) -> Vec<Collection> {
    let mut collections: Vec<(i64, Collection)> = body
        .as_array()
        .into_iter()
        .flatten()
        .filter(|collection| collection["platforms"].as_u64().unwrap_or(0) & PLATFORM_WEB != 0)
        .filter_map(|collection| {
            let mut items: Vec<(i64, Item)> = collection["application_directory_collection_items"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|item| {
                    let application = &item["application"];
                    let app_id = application["id"].as_str()?.to_owned();
                    Some((
                        item["position"].as_i64().unwrap_or(0),
                        Item {
                            app_id,
                            name: application["name"].as_str().unwrap_or_default().to_owned(),
                            description: application["description"]
                                .as_str()
                                .unwrap_or_default()
                                .to_owned(),
                            icon: application["icon"].as_str().map(str::to_owned),
                        },
                    ))
                })
                .collect();
            items.sort_by_key(|(position, _)| *position);
            Some((
                collection["position"].as_i64().unwrap_or(0),
                Collection {
                    id: collection["id"].as_str()?.to_owned(),
                    kind: collection["type"].as_u64().unwrap_or(0),
                    title: collection["title"].as_str().unwrap_or_default().to_owned(),
                    items: items.into_iter().map(|(_, item)| item).collect(),
                },
            ))
        })
        .filter(|(_, collection)| !collection.items.is_empty())
        .collect();
    collections.sort_by_key(|(position, _)| *position);
    collections
        .into_iter()
        .map(|(_, collection)| collection)
        .collect()
}

impl Root {
    /// On opening the launcher: the collections when not fresh, and this server's shelf for the banners.
    pub(super) fn request_launcher_home(&mut self) {
        let now = chrono::Utc::now().timestamp_millis();
        let stale = self
            .launcher_collections_at
            .is_none_or(|at| now - at >= COLLECTIONS_FRESH_MS);
        if stale && !self.requested.contains(COLLECTIONS_KEY) {
            self.requested.insert(COLLECTIONS_KEY.to_owned());
            let _ = self.requests.send(Request::Fetch {
                key: COLLECTIONS_KEY.to_owned(),
                route: "/application-directory-static/collections?surface=2&active_state=1&platform=4&locale=en-US&cache=true"
                    .to_owned(),
            });
        }
        let key = shelf_key(self.guild);
        if !self.fetched.contains_key(&key) && self.requested.insert(key.clone()) {
            let route = match self.guild {
                Some(guild) => format!("/activities/shelf?guild_id={guild}"),
                None => "/activities/shelf".to_owned(),
            };
            let _ = self.requests.send(Request::Fetch { key, route });
        }
    }

    pub(super) fn launcher_collections_fetched(&mut self, body: Value, cx: &mut Context<Self>) {
        self.requested.remove(COLLECTIONS_KEY);
        if crate::net::is_fetch_failure(&body) {
            cx.notify();
            return;
        }
        self.launcher_collections_at = Some(chrono::Utc::now().timestamp_millis());
        for collection in parse_collections(&body) {
            for item in collection.items {
                if let Some(hash) = &item.icon {
                    self.request_image(
                        format!("shelf-icon:{}", item.app_id),
                        format!(
                            "https://cdn.discordapp.com/app-icons/{}/{hash}.png?size=128",
                            item.app_id
                        ),
                    );
                }
            }
        }
        self.fetched.insert(COLLECTIONS_KEY.to_owned(), body);
        cx.notify();
    }

    /// Activities launched lately that this server's shelf still has, as 48px icons (Recents also lists them in the
    /// official launcher, which keeps one frecency list for commands and activities; here they follow the command
    /// apps).
    pub(super) fn launcher_recent_activities(
        &self,
        limit: usize,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let cards = match self.fetched.get(&shelf_key(self.guild)) {
            Some(body) => match Shelf::from_body(body) {
                Shelf::Cards(cards) => cards,
                _ => return Vec::new(),
            },
            None => return Vec::new(),
        };
        super::activity_shelf::stored_recents()
            .into_iter()
            .filter(|id| !self.slash.recents.iter().any(|(app, _, _)| app == id))
            .filter_map(|id| cards.iter().find(|card| card.app_id == id).cloned())
            .take(limit)
            .map(|card| {
                let picture = self
                    .images
                    .get(&format!("shelf-icon:{}", card.app_id))
                    .cloned();
                let app_id = card.app_id.clone();
                div()
                    .id(SharedString::from(format!(
                        "launcher-recent-{}",
                        card.app_id
                    )))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.slash.launcher = None;
                        this.launch_activity(&app_id, cx);
                    }))
                    .child(self.shelf_icon(picture, 48.))
                    .into_any_element()
            })
            .collect()
    }

    /// The card an app of the Promoted grid draws: its shelf entry (with the banner) when this server's shelf has it.
    fn promoted_card(&self, item: &Item) -> ShelfCard {
        let shelf = match self.fetched.get(&shelf_key(self.guild)) {
            Some(body) => match Shelf::from_body(body) {
                Shelf::Cards(cards) => cards,
                _ => Vec::new(),
            },
            None => Vec::new(),
        };
        let mut card = shelf
            .into_iter()
            .find(|card| card.app_id == item.app_id)
            .unwrap_or_else(|| ShelfCard {
                app_id: item.app_id.clone(),
                name: item.name.clone(),
                description: item.description.clone(),
                icon: item.icon.clone(),
                banner: None,
                promoted: false,
                label: 0,
                requires_age_gate: false,
            });
        // The text launcher's grid draws no PROMOTED or NEW pill (captured Promoted section).
        card.promoted = false;
        card.label = 0;
        card
    }

    /// `header_o-oxSF`: 4px above the 20px title, and "View More" (brand text) at the right, which opens the collection.
    pub(super) fn launcher_header(
        &self,
        title: String,
        more: Option<String>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .h(px(24.))
            .pt(px(4.))
            .flex()
            .justify_between()
            .text_size(px(16.))
            .line_height(px(20.))
            .font_weight(FontWeight(500.))
            .child(div().text_color(color("--text-strong")).child(title))
            .children(more.map(|collection| {
                div()
                    .id(SharedString::from(format!("launcher-more-{collection}")))
                    .cursor_pointer()
                    .text_color(color("--text-brand"))
                    .hover(|style| style.underline())
                    .child("View More")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(launcher) = this.slash.launcher.as_mut() {
                            launcher
                                .pages
                                .push(LauncherPage::Collection(collection.clone()));
                        }
                        cx.notify();
                    }))
            }))
            .into_any_element()
    }

    /// The application record a collection holds for this app.
    fn launcher_application(&self, id: &str) -> Option<Value> {
        self.fetched
            .get(COLLECTIONS_KEY)?
            .as_array()?
            .iter()
            .flat_map(|collection| {
                collection["application_directory_collection_items"]
                    .as_array()
                    .into_iter()
                    .flatten()
            })
            .map(|item| &item["application"])
            .find(|application| application["id"].as_str() == Some(id))
            .cloned()
    }

    /// Opens an app's page over the launcher, and asks for its cover (the `embedded_cover` asset at 2048, `tk`).
    pub(super) fn open_launcher_app(&mut self, id: String, cx: &mut Context<Self>) {
        let cover = self.fetched.get(&shelf_key(self.guild)).and_then(|body| {
            body["assets"][id.as_str()]
                .as_array()?
                .iter()
                .find(|asset| asset["name"].as_str() == Some("embedded_cover"))?["id"]
                .as_str()
                .map(str::to_owned)
        });
        let key = format!("{APP_INDEX_PREFIX}{id}");
        if !self.fetched.contains_key(&key) && self.requested.insert(key.clone()) {
            let _ = self.requests.send(Request::Fetch {
                key,
                route: format!("/applications/{id}/application-command-index"),
            });
        }
        if let Some(asset) = cover {
            self.request_image(
                format!("launcher-cover:{id}"),
                format!("https://cdn.discordapp.com/app-assets/{id}/{asset}.webp?size=2048"),
            );
        }
        if let Some(launcher) = self.slash.launcher.as_mut() {
            launcher.pages.push(LauncherPage::App {
                id,
                expanded: false,
            });
        }
        cx.notify();
    }

    /// A page over the home: its header (a round back button, the title centred) and its content.
    pub(super) fn launcher_page(&self, page: &LauncherPage, cx: &mut Context<Self>) -> AnyElement {
        let id = match page {
            LauncherPage::Collection(id) => id,
            LauncherPage::App { id, expanded } => return self.launcher_app_page(id, *expanded, cx),
        };
        let collection = self
            .fetched
            .get(COLLECTIONS_KEY)
            .map(parse_collections)
            .unwrap_or_default()
            .into_iter()
            .find(|collection| &collection.id == id);
        let (title, items) = collection
            .map(|collection| (collection.title, collection.items))
            .unwrap_or_default();
        let count = items.len();
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                // `header_NIUjob`: 16px from the top, 8px above the list; `backButton_NIUjob` is a pill of the
                // base-low fill around an 18px arrow.
                div()
                    .mt(px(16.))
                    .mb(px(8.))
                    .mx(px(16.))
                    .h(px(37.))
                    .relative()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .id("launcher-back")
                            .absolute()
                            .left_0()
                            .top_0()
                            .w(px(34.))
                            .h(px(37.))
                            .rounded_full()
                            .bg(color("--background-base-low"))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .child(super::icon(
                                "arrow_large_left",
                                18.,
                                "--interactive-text-active",
                            ))
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(launcher) = this.slash.launcher.as_mut() {
                                    launcher.pages.pop();
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-strong"))
                            .child(title),
                    ),
            )
            .child(
                div()
                    .id("launcher-page-scroll")
                    .flex_1()
                    .overflow_y_scroll()
                    .pl(px(16.))
                    .pr(px(24.))
                    .child(div().my(px(8.)).flex().flex_col().children(
                        items.iter().enumerate().map(|(index, item)| {
                            self.launcher_row(item, index == 0, index + 1 == count, cx)
                        }),
                    )),
            )
            .into_any_element()
    }

    /// One 64px row (`rowContainer_LekV6N`): a 40px icon, the name and one line of description, a divider under it but
    /// the last.
    fn launcher_row(
        &self,
        item: &Item,
        first: bool,
        last: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let picture = self
            .images
            .get(&format!("shelf-icon:{}", item.app_id))
            .cloned();
        let app_id = item.app_id.clone();
        div()
            .id(SharedString::from(format!("launcher-row-{}", item.app_id)))
            .relative()
            .h(px(64.))
            .p(px(12.))
            .flex()
            .items_center()
            .gap(px(12.))
            .bg(color("--background-surface-high"))
            .when(first, |this| this.rounded_t(px(12.)))
            .when(last, |this| this.rounded_b(px(12.)))
            .cursor_pointer()
            .hover(|style| style.bg(color("--interactive-background-hover")))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.open_launcher_app(app_id.clone(), cx);
            }))
            .child(self.shelf_icon(picture, 40.))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .h(px(20.))
                            .truncate()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .child(item.name.clone()),
                    )
                    .child(
                        div()
                            .h(px(18.))
                            .truncate()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(color("--text-subtle"))
                            .child(item.description.clone()),
                    ),
            )
            .when(!last, |this| {
                this.child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .bottom_0()
                        .h(px(1.))
                        .bg(rgba(0x97979f1f)),
                )
            })
            .into_any_element()
    }

    /// The collections under Recents, each a header with its content 12px below and 32px of space after.
    pub(super) fn launcher_collections(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let Some(body) = self.fetched.get(COLLECTIONS_KEY) else {
            return Vec::new();
        };
        let collections = parse_collections(body);
        let mut out = Vec::new();
        for collection in &collections {
            let content = if collection.kind == 5 {
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(12.))
                    .children(collection.items.iter().map(|item| {
                        let card = self.promoted_card(item);
                        self.shelf_card(
                            &card,
                            SharedString::from(format!("launcher-promoted-{}", item.app_id)),
                            true,
                            cx,
                        )
                    }))
                    .into_any_element()
            } else {
                let shown = collection.items.len().min(ROWS_SHOWN);
                div()
                    .flex()
                    .flex_col()
                    .children(collection.items.iter().take(shown).enumerate().map(
                        |(index, item)| self.launcher_row(item, index == 0, index + 1 == shown, cx),
                    ))
                    .into_any_element()
            };
            out.push(
                div()
                    .id(SharedString::from(format!(
                        "launcher-collection-{}",
                        collection.id
                    )))
                    .flex()
                    .flex_col()
                    .child(
                        self.launcher_header(
                            collection.title.clone(),
                            (collection.kind != 5 && collection.items.len() > ROWS_SHOWN)
                                .then(|| collection.id.clone()),
                            cx,
                        ),
                    )
                    .child(div().mt(px(8.)).mb(px(32.)).child(content))
                    .into_any_element(),
            );
        }
        out
    }
}

impl Root {
    /// A round header button of the app page (`headerButton_6GE03-`): the highest surface at half opacity around an
    /// 18px glyph.
    fn launcher_page_button(
        id: &'static str,
        glyph: &'static str,
        on_click: impl Fn(&mut Self, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        div()
            .id(id)
            .size(px(34.))
            .rounded_full()
            .bg(color("--background-surface-highest").opacity(0.5))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .child(super::icon(glyph, 18., "--interactive-text-active"))
            .on_click(cx.listener(move |this, _, _, cx| {
                on_click(this, cx);
                cx.notify();
            }))
    }

    /// `tM` of module 621377: the page of one app in the text launcher.
    fn launcher_app_page(&self, id: &str, expanded: bool, cx: &mut Context<Self>) -> AnyElement {
        let Some(application) = self.launcher_application(id) else {
            return div().into_any_element();
        };
        let app_id = id.to_owned();
        let name = application["name"].as_str().unwrap_or_default().to_owned();
        let description = application["description"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        let activity = application["embedded_activity_config"].is_object();
        let video = application["embedded_activity_config"]["activity_preview_video_asset_id"]
            .as_str()
            .is_some();
        let icon = self.images.get(&format!("shelf-icon:{app_id}")).cloned();
        let cover = self
            .images
            .get(&format!("launcher-cover:{app_id}"))
            .cloned();
        let me = self.stores.current_user.as_ref().map(|me| me.id);
        let link = app_link(&application, me);
        let bot = application["bot"]["id"]
            .as_str()
            .and_then(|id| id.parse::<u64>().ok());
        // The Play button runs the app's entry point command, named after it (`tg` with module 2098 `g`).
        // This channel's index when it has the app, else the app's own (asked for when the page opened).
        let index = self
            .command_index_body()
            .filter(|index| {
                super::activity_launch::primary_command(index, &app_id).is_some()
                    || super::slash::index_commands(index)
                        .iter()
                        .any(|command| command.app_id == app_id)
            })
            .or_else(|| {
                self.fetched
                    .get(&format!("{APP_INDEX_PREFIX}{app_id}"))
                    .cloned()
            });
        let play = index
            .as_ref()
            .and_then(|index| super::activity_launch::primary_command(index, &app_id))
            .and_then(|command| command["name"].as_str().map(title_case))
            .unwrap_or_else(|| "Launch".to_owned());
        let commands: Vec<super::slash::Cmd> = index
            .as_ref()
            .map(super::slash::index_commands)
            .unwrap_or_default()
            .into_iter()
            .filter(|command| command.app_id == app_id)
            .collect();
        let tags: Vec<String> = if activity {
            std::iter::once(participants_label(application["max_participants"].as_u64()))
                .chain(
                    application["tags"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|tag| tag.as_str().map(str::to_owned)),
                )
                .collect()
        } else {
            Vec::new()
        };
        let tag = |index: usize, text: String| {
            div()
                .h(px(22.))
                .px(px(4.))
                .py(px(2.))
                .rounded(px(4.))
                .bg(color("--background-base-lowest"))
                .flex()
                .items_center()
                .gap(px(4.))
                .when(index == 0, |this| {
                    this.child(super::icon("members", 16., "--interactive-text-default"))
                })
                .child(
                    div()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--interactive-text-default"))
                        .child(text),
                )
        };
        let open_dm = app_id.clone();
        let header = div()
            .absolute()
            .top(px(16.))
            .left(px(16.))
            .right(px(16.))
            .flex()
            .justify_between()
            .child(Self::launcher_page_button(
                "launcher-app-back",
                "arrow_large_left",
                |this, _| {
                    if let Some(launcher) = this.slash.launcher.as_mut() {
                        launcher.pages.pop();
                    }
                },
                cx,
            ))
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(Self::launcher_page_button(
                        "launcher-app-link",
                        "share_link",
                        move |this, cx| {
                            cx.write_to_clipboard(gpui::ClipboardItem::new_string(link.clone()));
                            this.show_success_toast("Link Copied!".to_owned(), cx);
                        },
                        cx,
                    ))
                    .child({
                        let more_app = open_dm.clone();
                        div()
                            .id("launcher-app-more")
                            .size(px(34.))
                            .rounded_full()
                            .bg(color("--background-surface-highest").opacity(0.5))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .child(super::icon(
                                "more_horizontal",
                                18.,
                                "--interactive-text-active",
                            ))
                            .on_click(cx.listener(move |this, event: &gpui::ClickEvent, _, cx| {
                                if let Ok(app) = more_app.parse::<u64>() {
                                    // `app-details-more-menu`: opens under the button, aligned to its right.
                                    this.open_context_menu(
                                        super::context_menu::Target::LauncherApp(app),
                                        event.position(),
                                        cx,
                                    );
                                }
                            }))
                    }),
            );
        let overview = div()
            .p(px(12.))
            .bg(color("--background-base-low"))
            .when(video, |this| this.rounded_b(px(8.)))
            .when(!video, |this| this.rounded(px(8.)))
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .h(px(32.))
                    .truncate()
                    .text_size(px(24.))
                    .line_height(px(32.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-strong"))
                    .child(name.clone()),
            )
            .when(!tags.is_empty(), |this| {
                this.child(
                    div().flex().flex_wrap().gap(px(4.)).children(
                        tags.into_iter()
                            .enumerate()
                            .map(|(index, text)| tag(index, text)),
                    ),
                )
            })
            .when(!description.is_empty(), |this| {
                // `tL` offers Show More only past two lines (measured by the page); here a description longer than
                // two lines of the 444px column at 16px, about 90 characters, stands for that measure.
                let long = description.chars().count() > 90;
                let toggle = app_id.clone();
                this.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .child(
                            div()
                                // `lineClamp2Plus`: two lines and an ellipsis until Show More.
                                .when(!expanded || !long, |this| {
                                    this.line_clamp(2).text_ellipsis()
                                })
                                .text_size(px(16.))
                                .line_height(px(22.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-default"))
                                .child(description.clone()),
                        )
                        .when(long, |this| {
                            this.child(
                                div()
                                    .id("launcher-app-show-more")
                                    .h(px(18.))
                                    .flex()
                                    .items_center()
                                    .cursor_pointer()
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-brand"))
                                    .child(if expanded { "Show Less" } else { "Show More" })
                                    .child(super::icon("chevron_down", 18., "--text-brand").when(
                                        expanded,
                                        |this| {
                                            this.with_transformation(gpui::Transformation::rotate(
                                                gpui::percentage(0.5),
                                            ))
                                        },
                                    ))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if let Some(launcher) = this.slash.launcher.as_mut()
                                            && let Some(LauncherPage::App { id, expanded }) =
                                                launcher.pages.last_mut()
                                            && *id == toggle
                                        {
                                            *expanded = !*expanded;
                                        }
                                        cx.notify();
                                    })),
                            )
                        }),
                )
            })
            .child({
                let launch = app_id.clone();
                div()
                    .flex()
                    .gap(px(8.))
                    .child(
                        div()
                            .id("launcher-app-play")
                            .flex_1()
                            .h(px(40.))
                            .rounded(px(8.))
                            .bg(color("--control-primary-background-default"))
                            .hover(|style| style.bg(color("--control-primary-background-hover")))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--white"))
                            .child(play)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.slash.launcher = None;
                                this.launch_activity(&launch, cx);
                            })),
                    )
                    .children(bot.map(|bot| {
                        div()
                            .id("launcher-app-dm")
                            .flex_1()
                            .h(px(40.))
                            .rounded(px(8.))
                            .bg(color("--control-secondary-background-default"))
                            .hover(|style| style.bg(color("--control-secondary-background-hover")))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--control-secondary-text-default"))
                            .child("Launch in DM")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.slash.launcher = None;
                                this.open_dm_with(bot, cx);
                            }))
                    }))
            });
        let count = commands.len();
        let use_app = (count > 0).then(|| {
            div()
                .px(px(16.))
                .pb(px(16.))
                .child(
                    div()
                        .mt(px(16.))
                        .mb(px(8.))
                        .h(px(32.))
                        .flex()
                        .items_center()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-strong"))
                        .child("Use this app"),
                )
                .child(
                    div()
                        .rounded(px(8.))
                        .overflow_hidden()
                        .bg(color("--background-base-lower"))
                        .flex()
                        .flex_col()
                        .children(commands.into_iter().enumerate().map(|(index, command)| {
                            let send = command.clone();
                            div()
                                .id(("launcher-app-command", index))
                                .py(px(8.))
                                .px(px(12.))
                                .bg(color("--background-base-low"))
                                .flex()
                                .items_center()
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w(px(0.))
                                        .flex()
                                        .flex_col()
                                        .gap(px(4.))
                                        .child(
                                            div()
                                                .text_size(px(14.))
                                                .line_height(px(18.))
                                                .font_weight(FontWeight(600.))
                                                .text_color(color("--text-strong"))
                                                .child(command.name.clone()),
                                        )
                                        .child(
                                            div()
                                                .h(px(16.))
                                                .truncate()
                                                .text_size(px(12.))
                                                .line_height(px(16.))
                                                .font_weight(FontWeight(500.))
                                                .text_color(color("--text-muted"))
                                                .child(command.description.clone()),
                                        ),
                                )
                                .child(
                                    div()
                                        .id(("launcher-app-send", index))
                                        .w(px(100.))
                                        .h(px(40.))
                                        .rounded(px(8.))
                                        .bg(color("--control-secondary-background-default"))
                                        .hover(|style| {
                                            style.bg(color("--control-secondary-background-hover"))
                                        })
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .gap(px(4.))
                                        .cursor_pointer()
                                        .text_size(px(16.))
                                        .line_height(px(20.))
                                        .font_weight(FontWeight(500.))
                                        .text_color(color("--control-secondary-text-default"))
                                        .child("Send")
                                        .child(super::icon(
                                            "send_message",
                                            20.,
                                            "--control-secondary-text-default",
                                        ))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.slash.launcher = None;
                                            this.begin_command(send.clone(), "", cx);
                                        })),
                                )
                                .when(index + 1 < count, |this| this.mb(px(1.)))
                        })),
                )
        });
        div()
            .id("launcher-app-page")
            .size_full()
            .relative()
            .overflow_y_scroll()
            .bg(color("--background-base-lower"))
            .child(
                div()
                    .flex()
                    .flex_col()
                    // `bannerBackground_6GE03-`: 106px of the base-low fill (the official tints it from the icon once
                    // its colours are read; the capture shows the fallback).
                    .child(
                        div()
                            .h(px(106.))
                            .flex_none()
                            .bg(color("--background-base-low")),
                    )
                    .child(div().h(px(54.)).flex_none())
                    .child(
                        div()
                            .px(px(16.))
                            .flex()
                            .flex_col()
                            .when(video, |this| {
                                this.child(
                                    div()
                                        .w_full()
                                        .h(px(263.25))
                                        .rounded_t(px(8.))
                                        .overflow_hidden()
                                        .bg(color("--background-base-low"))
                                        .children(cover.map(|cover| {
                                            gpui::img(cover)
                                                .size_full()
                                                .object_fit(gpui::ObjectFit::Cover)
                                        })),
                                )
                                .child(div().h(px(3.)))
                            })
                            .child(overview),
                    )
                    .children(use_app),
            )
            // `appIcon_46XT_r`: 80px with 24px corners, 12px from the left over the banner's foot.
            .child(
                div()
                    .absolute()
                    .left(px(12.))
                    .top(px(66.))
                    .size(px(80.))
                    .rounded(px(24.))
                    .overflow_hidden()
                    .children(icon.map(|icon| gpui::img(icon).size_full().rounded(px(24.)))),
            )
            .child(header)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::{app_link, parse_collections, participants_label, title_case};
    use serde_json::json;

    #[test]
    fn the_page_names_play_and_participants_as_the_official_helpers_do() {
        assert_eq!(title_case("play_now"), "Play now");
        assert_eq!(title_case("launch"), "Launch");
        assert_eq!(participants_label(None), "Unlimited");
        assert_eq!(participants_label(Some(0)), "Unlimited");
        assert_eq!(participants_label(Some(8)), "1-8");
    }

    #[test]
    fn copy_link_points_an_activity_at_its_page_and_an_app_at_authorize() {
        let activity = json!({"id": "7", "embedded_surfaces": [0]});
        assert_eq!(
            app_link(&activity, Some(3)),
            "https://discord.com/activities/7?referrer_id=3"
        );
        let bot = json!({"id": "8", "install_params": {"permissions": "0", "scopes": ["bot", "applications.commands"]}});
        assert_eq!(
            app_link(&bot, Some(3)),
            "https://discord.com/oauth2/authorize?client_id=8&permissions=0&scope=bot+applications.commands"
        );
    }

    #[test]
    fn collections_follow_their_positions_and_the_desktop_platform() {
        let body: serde_json::Value = serde_json::from_str(
            r#"[
                {"id": "b", "type": 4, "position": 2, "platforms": 7, "title": "Puzzle Games",
                 "application_directory_collection_items": [
                    {"position": 2, "application": {"id": "2", "name": "Two", "description": "d2"}},
                    {"position": 1, "application": {"id": "1", "name": "One", "description": "d1", "icon": "h"}}]},
                {"id": "a", "type": 5, "position": 1, "platforms": 7, "title": "Promoted",
                 "application_directory_collection_items": [{"position": 1, "application": {"id": "3", "name": "Three"}}]},
                {"id": "m", "type": 4, "position": 3, "platforms": 3, "title": "Mobile only",
                 "application_directory_collection_items": [{"position": 1, "application": {"id": "4", "name": "Four"}}]}
            ]"#,
        )
        .unwrap();
        let collections = parse_collections(&body);
        assert_eq!(
            collections
                .iter()
                .map(|c| c.title.as_str())
                .collect::<Vec<_>>(),
            ["Promoted", "Puzzle Games"]
        );
        assert_eq!(
            collections[1]
                .items
                .iter()
                .map(|i| i.name.as_str())
                .collect::<Vec<_>>(),
            ["One", "Two"]
        );
        assert_eq!(collections[1].items[0].icon.as_deref(), Some("h"));
    }
}
