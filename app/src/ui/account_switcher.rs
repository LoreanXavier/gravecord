//! Switch Accounts: the accounts this device knows (`MultiAccountStore`, module 274303), the "Switch Accounts" list of
//! the avatar popout, the Manage Accounts dialog with its "..." menu, the Add Account dialog, and the switch itself
//! (`MultiAccountSwitchStore`, module 376463; the toast of module 12489). Geometry comes from
//! `design/dom/rail/account*.json`.
//!
//! The list (id, name, avatar hash, token status) lives in the local storage like the official store's `users`; the
//! tokens never do: each sits in the credential store under `account_entry(id)`, and the shell moves them.

use super::{Root, avatar_source, color, icon, login, modal};
use dc_state::User;
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, MouseDownEvent, SharedString, anchored, deferred,
    div, img, prelude::*, px,
};
use serde_json::{Value, json};

/// The store keeps at most five accounts (`splice(5)` in `CONNECTION_OPEN`); the one that would be sixth is dropped
/// with its token.
pub(super) const MAX_ACCOUNTS: usize = 5;
/// The list's natural top: the "Switch Accounts" row's top edge in the avatar popout (capture).
const SUBMENU_TOP: f32 = 589.;
const STORAGE_KEY: &str = "MultiAccountStore";
/// What the stand-in code on the Add Account dialog encodes: a fixed text, never a real remote-auth fingerprint.
const ADD_QR_TEXT: &str = "https://discord.com/ra/gravecord-add-account-preview";

/// One entry of `MultiAccountStore.users`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Account {
    pub id: u64,
    pub username: String,
    pub avatar: Option<String>,
    pub discriminator: String,
    /// `tokenStatus`: 0 invalid, 1 validating, 2 valid.
    pub status: u8,
}

impl Account {
    /// The account as a user, for the avatar picture.
    fn user(&self) -> User {
        User {
            id: self.id,
            username: self.username.clone(),
            avatar: self.avatar.clone(),
            discriminator: self.discriminator.clone(),
            ..User::default()
        }
    }
}

#[derive(Default)]
pub(super) struct Accounts {
    pub users: Vec<Account>,
}

impl Accounts {
    pub(super) fn from_json(value: &Value) -> Self {
        let users = value["users"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|user| {
                Some(Account {
                    id: user["id"].as_str()?.parse().ok()?,
                    username: user["username"].as_str()?.to_owned(),
                    avatar: user["avatar"].as_str().map(str::to_owned),
                    discriminator: user["discriminator"].as_str().unwrap_or("0").to_owned(),
                    status: user["tokenStatus"].as_u64().unwrap_or(2) as u8,
                })
            })
            .collect();
        Self { users }
    }

    pub(super) fn to_json(&self) -> Value {
        json!({
            "users": self.users.iter().map(|account| json!({
                "id": account.id.to_string(),
                "avatar": account.avatar,
                "username": account.username,
                "discriminator": account.discriminator,
                "tokenStatus": account.status,
            })).collect::<Vec<_>>(),
        })
    }

    /// `CONNECTION_OPEN`: the signed in user is added at the end or refreshed in place; past five, the tail goes and
    /// its ids are returned so their tokens can be forgotten.
    pub(super) fn seen(&mut self, user: &User) -> Vec<u64> {
        let entry = Account {
            id: user.id,
            username: user.username.clone(),
            avatar: user.avatar.clone(),
            discriminator: user.discriminator.clone(),
            status: 2,
        };
        match self.users.iter_mut().find(|account| account.id == user.id) {
            Some(existing) => *existing = entry,
            None => self.users.push(entry),
        }
        // Five, or what the UnlimitedAccounts plugin allows.
        let most = super::plugin_hooks::max_accounts();
        if self.users.len() > most {
            self.users
                .split_off(most)
                .into_iter()
                .map(|account| account.id)
                .collect()
        } else {
            Vec::new()
        }
    }

    /// `MULTI_ACCOUNT_REMOVE_ACCOUNT`.
    pub(super) fn remove(&mut self, id: u64) {
        self.users.retain(|account| account.id != id);
    }
}

/// The open dialogs of the switcher.
#[derive(Default)]
pub(super) struct State {
    pub accounts: Accounts,
    loaded: bool,
    /// The user whose entry was last handed to the shell, so each session announces itself once.
    announced: Option<u64>,
    /// A switch was made: the toast shows once the new session knows its user.
    pub switched: bool,
    pub manage: bool,
    /// The "..." menu of an account card: the account and where it opened.
    pub more: Option<(u64, gpui::Point<gpui::Pixels>)>,
    pub add: Option<AddAccount>,
}

/// The Add Account dialog: the two login fields are drawn and never submitted from here.
pub(super) struct AddAccount {
    pub reveal: bool,
}

impl Root {
    /// Once a frame: the account list is read, and the signed in user is entered into it.
    pub(super) fn accounts_frame(&mut self, cx: &mut Context<Self>) {
        if !self.account_switcher.loaded {
            self.account_switcher.loaded = true;
            if let Some(value) = crate::storage::get(STORAGE_KEY) {
                self.account_switcher.accounts = Accounts::from_json(&value);
            }
        }
        let Some(user) = self.stores.current_user.clone() else {
            return;
        };
        if self.account_switcher.accounts.users.iter().any(|account| {
            account.id == user.id
                && account.username == user.username
                && account.avatar == user.avatar
                && account.status == 2
        }) && self.account_switcher.announced == Some(user.id)
        {
            return;
        }
        let dropped = self.account_switcher.accounts.seen(&user);
        crate::storage::set(STORAGE_KEY, self.account_switcher.accounts.to_json());
        if self.account_switcher.announced != Some(user.id) {
            self.account_switcher.announced = Some(user.id);
            cx.emit(super::logout::RootEvent::AccountSeen(user.id));
        }
        for id in dropped {
            cx.emit(super::logout::RootEvent::AccountRemoved(id));
        }
        if std::mem::take(&mut self.account_switcher.switched) {
            self.put_app_toast(format!("Switched to {}", user.username), true, cx);
        }
    }

    /// The shell reconnected with another account's token: the toast shows once that session knows its user.
    pub fn announce_switch(&mut self) {
        self.account_switcher.switched = true;
    }

    /// `onSwitchError`: the stored token did not open a session.
    pub fn switch_failed(&mut self, cx: &mut Context<Self>) {
        self.put_app_toast("Switch failed, sorry :(".to_owned(), false, cx);
    }

    /// A tap on an account of the list: the shell reconnects with its token. The active account does nothing.
    pub(super) fn switch_account(&mut self, id: u64, cx: &mut Context<Self>) {
        self.account_switcher.manage = false;
        self.status_popout = None;
        if self.stores.current_user.as_ref().map(|user| user.id) == Some(id) {
            cx.notify();
            return;
        }
        cx.emit(super::logout::RootEvent::SwitchAccount(id));
        cx.notify();
    }

    /// The avatar of an account in a row: the picture when it is loaded, else the plain blurple disc.
    fn account_avatar(&self, account: &Account, size: f32) -> AnyElement {
        let picture = self.images.get(&avatar_source(&account.user()).0).cloned();
        div()
            .size(px(size))
            .flex_none()
            .rounded_full()
            .overflow_hidden()
            .bg(color("--brand-500"))
            .children(picture.map(|picture| img(picture).size(px(size)).rounded_full()))
            .into_any_element()
    }

    /// The list of the Switch Accounts submenu: a row per account (the signed in one marked), then Manage Accounts.
    /// 220 wide at the popout's right edge (capture: left 296, top 589 of an account row of 40, a rule, a row of 34).
    pub(super) fn accounts_submenu_view(&self, cx: &mut Context<Self>) -> AnyElement {
        let me = self.stores.current_user.as_ref().map(|user| user.id);
        let mut rows: Vec<AnyElement> = Vec::new();
        for account in &self.account_switcher.accounts.users {
            let id = account.id;
            rows.push(
                div()
                    .id(SharedString::from(format!("account-row-{id}")))
                    .h(px(40.))
                    .flex_none()
                    .p(px(8.))
                    .rounded(px(2.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .cursor_pointer()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-default"))
                    .hover(|style| {
                        style
                            .bg(color("--interactive-background-hover"))
                            .rounded(px(4.))
                    })
                    .child(self.account_avatar(account, 24.))
                    .child(div().child(account.username.clone()))
                    .when(me == Some(id), |this| {
                        this.child(
                            div()
                                .size(px(18.))
                                .flex_none()
                                .rounded_full()
                                .bg(color("--brand-500"))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(icon("checkmark_large", 12., "--white")),
                        )
                    })
                    .on_click(cx.listener(move |this, _, _, cx| this.switch_account(id, cx)))
                    .into_any_element(),
            );
        }
        rows.push(
            div()
                .my(px(8.))
                .h(px(1.))
                .flex_none()
                .bg(color("--border-subtle"))
                .into_any_element(),
        );
        rows.push(
            div()
                .id("account-manage")
                .h(px(34.))
                .flex_none()
                .p(px(8.))
                .rounded(px(2.))
                .flex()
                .items_center()
                .cursor_pointer()
                .text_size(px(14.))
                .line_height(px(18.))
                .font_weight(FontWeight(500.))
                .text_color(color("--text-strong"))
                .hover(|style| {
                    style
                        .bg(color("--interactive-background-hover"))
                        .rounded(px(4.))
                })
                .child("Manage Accounts")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.status_popout = None;
                    this.account_switcher.manage = true;
                    cx.notify();
                }))
                .into_any_element(),
        );
        // `Popout` (module 265872) puts the list at the row's top edge, right of it; `nudgeTopAlignment` (module
        // 623646) then moves it up until it ends 12px above the window's bottom. 107 high with one account.
        let height =
            107. + 40. * self.account_switcher.accounts.users.len().saturating_sub(1) as f32;
        let top = SUBMENU_TOP.min(self.viewport.1 - 12. - height);
        div()
            .absolute()
            .size_full()
            .child(
                div()
                    .id("status-accounts")
                    .occlude()
                    .absolute()
                    .left(px(296.))
                    .top(px(top))
                    .w(px(220.))
                    .p(px(8.))
                    .rounded(px(8.))
                    .bg(color("--background-surface-higher"))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .flex()
                    .flex_col()
                    .children(rows),
            )
            .into_any_element()
    }

    /// Manage Accounts (module 970573): a card per account with 1px separators, `seV8yt` ("Active account")
    /// under the signed in one, `tYX2ps` ("Please log in again.") + `DSN+hw` ("Log in") secondary button when
    /// `status == 0` (invalid token), `"Switch"` secondary button for valid inactive accounts, a `"..."` menu
    /// (`PdRCRg` "More Options" / `41qiDU` "Manage account") with `lSLMaU` ("Log out"), and Add an account in footer.
    pub(super) fn manage_accounts_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.account_switcher.manage {
            return None;
        }
        let me = self.stores.current_user.as_ref().map(|user| user.id);
        let hide_private = self.streamer_hides_personal();
        let mut cards: Vec<AnyElement> = Vec::new();
        for (idx, account) in self.account_switcher.accounts.users.iter().enumerate() {
            if idx > 0 {
                cards.push(
                    div()
                        .w_full()
                        .h(px(1.))
                        .bg(color("--border-subtle"))
                        .into_any_element(),
                );
            }
            let id = account.id;
            let active = me == Some(id);
            let invalid = !active && account.status == 0;
            let legacy_discrim = (!hide_private
                && !account.discriminator.is_empty()
                && account.discriminator != "0")
                .then(|| format!("#{}", account.discriminator));
            cards.push(
                div()
                    .id(SharedString::from(format!("manage-account-{id}")))
                    .h(px(64.))
                    .flex_none()
                    .px(px(16.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(self.account_avatar(account, 40.))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .flex()
                                    .items_baseline()
                                    .gap(px(2.))
                                    .child(
                                        div()
                                            .text_size(px(16.))
                                            .line_height(px(20.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--text-strong"))
                                            .truncate()
                                            .child(account.username.clone()),
                                    )
                                    .children(legacy_discrim.map(|disc| {
                                        div()
                                            .text_size(px(14.))
                                            .line_height(px(18.))
                                            .text_color(color("--text-default"))
                                            .child(disc)
                                    })),
                            )
                            .when(active, |this| {
                                this.child(
                                    div()
                                        .text_size(px(14.))
                                        .line_height(px(18.))
                                        .font_weight(FontWeight(600.))
                                        .text_color(color("--text-feedback-positive"))
                                        .child(dc_intl::t("seV8yt")),
                                )
                            })
                            .when(invalid, |this| {
                                this.child(
                                    div()
                                        .text_size(px(14.))
                                        .line_height(px(18.))
                                        .font_weight(FontWeight(600.))
                                        .text_color(color("--text-feedback-critical"))
                                        .child(dc_intl::t("tYX2ps")),
                                )
                            }),
                    )
                    .when(!active, |row| {
                        row.child(
                            div()
                                .id(SharedString::from(format!("manage-account-action-{id}")))
                                .h(px(32.))
                                .px(px(12.))
                                .rounded(px(8.))
                                .bg(color("--control-secondary-background-default"))
                                .hover(|s| s.bg(color("--control-secondary-background-hover")))
                                .border_1()
                                .border_color(color("--control-secondary-border-default"))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-strong"))
                                .child(if invalid {
                                    dc_intl::t("DSN+hw")
                                } else {
                                    dc_intl::tr("Switch")
                                })
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if invalid {
                                        this.account_switcher.manage = false;
                                        this.account_switcher.add =
                                            Some(AddAccount { reveal: false });
                                        cx.notify();
                                    } else {
                                        this.switch_account(id, cx);
                                    }
                                })),
                        )
                    })
                    .child(
                        self.with_tip(
                            format!("manage-more-tip-{id}"),
                            super::tooltip::Tip::text(
                                dc_intl::t("PdRCRg"),
                                super::tooltip::Side::Top,
                            ),
                            div()
                                .id(SharedString::from(format!("manage-more-{id}")))
                                .size(px(40.))
                                .flex_none()
                                .rounded(px(8.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .cursor_pointer()
                                .hover(|style| style.bg(color("--interactive-background-hover")))
                                .child(icon("more_horizontal", 20., "--icon-default"))
                                .on_mouse_down(
                                    gpui::MouseButton::Left,
                                    cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                                        this.account_switcher.more = Some((id, event.position));
                                        cx.stop_propagation();
                                        cx.notify();
                                    }),
                                ),
                            cx,
                        ),
                    )
                    .into_any_element(),
            );
        }
        let list = div()
            .flex()
            .flex_col()
            .rounded(px(8.))
            .bg(color("--interactive-background-hover"))
            .children(cards);
        let full =
            self.account_switcher.accounts.users.len() >= super::plugin_hooks::max_accounts();
        let spec = modal::Spec {
            id: "manage-accounts",
            title: "Manage Accounts".into(),
            subtitle: Some(
                div()
                    .child("Switch accounts, sign in, sign out, go wild.")
                    .into_any_element(),
            ),
            body: Some(list.into_any_element()),
            buttons: vec![modal::button(
                "manage-accounts-add",
                "Add an account",
                if full {
                    modal::Tone::PrimaryDisabled
                } else {
                    modal::Tone::Secondary
                },
                |this, _, cx| {
                    this.account_switcher.manage = false;
                    this.account_switcher.add = Some(AddAccount { reveal: false });
                    cx.notify();
                },
            )],
            close: Box::new(|this, _, _| {
                this.account_switcher.manage = false;
                this.account_switcher.more = None;
            }),
        };
        Some(modal::render(spec, cx))
    }

    /// Add Account: the login form of the sign-in page in a 680 wide dialog (`design/dom/rail/accounts-modal-add-login`).
    /// The two fields are drawn and never submitted from here, and Continue does nothing: a second login is the
    /// sign-in page's business. The code on the right is a stand-in for the remote-auth QR code.
    pub(super) fn add_account_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let add = self.account_switcher.add.as_ref()?;
        let reveal = add.reveal;
        let field = |height: f32| {
            div()
                .w_full()
                .h(px(height))
                .flex_none()
                .rounded(px(8.))
                .border_1()
                .border_color(color("--input-border-default"))
                .bg(color("--input-background-default"))
                .flex()
                .items_center()
                .px(px(12.))
        };
        let form = div()
            .w(px(358.))
            .pt(px(84.))
            .flex_none()
            .flex()
            .flex_col()
            .child(
                div()
                    .mb(px(9.))
                    .child(login::label("Email or Phone Number", true)),
            )
            .child(field(44.))
            .child(
                div()
                    .mt(px(19.))
                    .mb(px(9.))
                    .child(login::label("Password", true)),
            )
            .child(
                field(40.).justify_end().child(
                    div()
                        .id("add-account-reveal")
                        .cursor_pointer()
                        .child(icon(
                            if reveal { "eye_slash" } else { "login_eye" },
                            20.,
                            "--icon-default",
                        ))
                        .on_click(cx.listener(|this, _, _, cx| {
                            if let Some(add) = this.account_switcher.add.as_mut() {
                                add.reveal = !add.reveal;
                            }
                            cx.notify();
                        })),
                ),
            )
            .child(div().mt(px(4.)).child(login::text_button(
                "add-account-forgot",
                "Forgot your password?",
            )));
        let code = login::qr_code_card(ADD_QR_TEXT, 176., 8., 4.).map(|card| {
            div()
                .relative()
                .size(px(176.))
                .flex_none()
                .child(card)
                .child(
                    img(crate::branding::qr_logo())
                        .absolute()
                        .left(px(63.))
                        .top(px(63.))
                        .size(px(50.)),
                )
        });
        let qr =
            div()
                .w(px(240.))
                .flex_none()
                .pt(px(16.))
                .flex()
                .flex_col()
                .items_center()
                .children(code)
                .child(
                    div()
                        .mt(px(32.))
                        .child(login::heading("Log in with QR Code")),
                )
                .child(
                    div()
                        .mt(px(8.))
                        .w(px(240.))
                        .text_center()
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .text_color(color("--text-default"))
                        .child(login::rich(
                            "Scan this with the Discord mobile app to log in instantly.",
                            19..37,
                        )),
                )
                .child(div().w(px(256.)).p(px(8.)).flex().justify_center().child(
                    login::text_button("add-account-passkey", "Or sign in with a passkey"),
                ));
        let body = div().px(px(24.)).flex().gap(px(32.)).child(form).child(qr);
        let back = div()
            .id("add-account-back")
            .cursor_pointer()
            .text_size(px(16.))
            .line_height(px(20.))
            .font_weight(FontWeight(500.))
            .text_color(color("--text-strong"))
            .child("Back")
            .on_click(cx.listener(|this, _, _, cx| {
                this.account_switcher.add = None;
                this.account_switcher.manage = true;
                cx.notify();
            }));
        let spec = modal::Spec {
            id: "add-account",
            title: "Add Account".into(),
            subtitle: Some(
                div()
                    .child("Logging in to another account will let you easily switch between accounts on this device.")
                    .into_any_element(),
            ),
            body: Some(body.into_any_element()),
            buttons: vec![modal::button(
                "add-account-continue",
                "Continue",
                modal::Tone::Primary,
                |_, _, _| {},
            )],
            close: Box::new(|this, _, _| {
                this.account_switcher.add = None;
            }),
        };
        Some(modal::render_pinned_with_input(
            spec,
            680.,
            104.,
            512.,
            back.into_any_element(),
            cx,
        ))
    }

    /// The "..." menu of a card (`manage-multi-account`, module 970573, `41qiDU` "Manage account"):
    /// `lSLMaU` ("Log out"). The active account goes through the Log Out confirmation; another one is
    /// signed out of this device (its stored token is forgotten).
    pub(super) fn account_more_menu(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (id, at) = self.account_switcher.more?;
        let _aria_label = dc_intl::t("41qiDU");
        let frame = div()
            .id("account-more-menu")
            .occlude()
            .w(px(144.))
            .rounded(px(8.))
            .bg(color("--background-surface-higher"))
            .border_1()
            .border_color(color("--border-subtle"))
            .shadow(vec![gpui::BoxShadow {
                color: gpui::hsla(0., 0., 0., 0.24),
                offset: gpui::point(px(0.), px(12.)),
                blur_radius: px(24.),
                spread_radius: px(0.),
                inset: false,
            }])
            .p(px(8.))
            .child(
                div()
                    .id("account-log-out")
                    .h(px(34.))
                    .p(px(8.))
                    .rounded(px(2.))
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-feedback-critical"))
                    .whitespace_nowrap()
                    .hover(|style| {
                        style
                            .bg(color("--background-feedback-mod-critical"))
                            .rounded(px(4.))
                    })
                    .child(dc_intl::t("lSLMaU"))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.account_switcher.more = None;
                        if this.stores.current_user.as_ref().map(|user| user.id) == Some(id) {
                            this.account_switcher.manage = false;
                            this.logout_confirm = true;
                        } else {
                            this.account_switcher.accounts.remove(id);
                            crate::storage::set(
                                STORAGE_KEY,
                                this.account_switcher.accounts.to_json(),
                            );
                            cx.emit(super::logout::RootEvent::AccountRemoved(id));
                        }
                        cx.notify();
                    })),
            );
        Some(
            deferred(
                anchored()
                    .anchor(gpui::Anchor::TopLeft)
                    .position(at)
                    .snap_to_window_with_margin(px(8.))
                    .child(super::keyboard::outside(
                        frame,
                        super::keyboard::Layer::AccountMenu,
                        cx,
                    )),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }

    /// Escape: the "..." menu first, then the dialogs. True when something closed.
    pub(super) fn account_switcher_escape(&mut self, cx: &mut Context<Self>) -> bool {
        let closed = if self.account_switcher.more.take().is_some()
            || self.account_switcher.add.take().is_some()
        {
            true
        } else {
            std::mem::take(&mut self.account_switcher.manage)
        };
        if closed {
            cx.notify();
        }
        closed
    }

    pub(super) fn account_switcher_open(&self) -> bool {
        self.account_switcher.manage || self.account_switcher.add.is_some()
    }

    /// Capture only: put an account with no token into the list, or show the switch toast.
    #[cfg(feature = "capture")]
    pub(crate) fn script_accounts(&mut self, words: &[&str], cx: &mut Context<Self>) -> String {
        match words {
            ["fake", id, name] => {
                let Ok(id) = id.parse::<u64>() else {
                    return "usage: accounts fake <id> <name>".into();
                };
                let user = User {
                    id,
                    username: (*name).to_owned(),
                    discriminator: "0".into(),
                    ..User::default()
                };
                self.account_switcher.accounts.seen(&user);
                cx.notify();
                format!(
                    "accounts: {} listed",
                    self.account_switcher.accounts.users.len()
                )
            }
            ["expired", id, name] => {
                let Ok(id) = id.parse::<u64>() else {
                    return "usage: accounts expired <id> <name>".into();
                };
                let user = User {
                    id,
                    username: (*name).to_owned(),
                    discriminator: "0".into(),
                    ..User::default()
                };
                self.account_switcher.accounts.seen(&user);
                if let Some(acc) = self
                    .account_switcher
                    .accounts
                    .users
                    .iter_mut()
                    .find(|a| a.id == id)
                {
                    acc.status = 0;
                }
                self.account_switcher.manage = true;
                cx.notify();
                format!("accounts: expired {name} added & manage open")
            }
            ["manage"] => {
                self.account_switcher.manage = true;
                cx.notify();
                "accounts: manage open".into()
            }
            ["toast", name] => {
                self.put_app_toast(format!("Switched to {name}"), true, cx);
                "accounts: toast".into()
            }
            ["switch", id] => match id.parse::<u64>() {
                Ok(id) => {
                    self.switch_account(id, cx);
                    format!(
                        "accounts: would connect with account {id} (the event is emitted; the harness has no shell to receive it)"
                    )
                }
                Err(_) => "usage: accounts switch <id>".into(),
            },
            _ => "usage: accounts fake|manage|toast|switch".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(id: u64, name: &str) -> User {
        User {
            id,
            username: name.into(),
            discriminator: "0".into(),
            ..User::default()
        }
    }

    #[test]
    fn a_new_user_goes_last_and_a_known_one_is_refreshed_in_place() {
        let mut accounts = Accounts::default();
        assert!(accounts.seen(&user(1, "a")).is_empty());
        assert!(accounts.seen(&user(2, "b")).is_empty());
        assert!(accounts.seen(&user(1, "renamed")).is_empty());
        let names: Vec<&str> = accounts.users.iter().map(|a| a.username.as_str()).collect();
        assert_eq!(names, ["renamed", "b"]);
    }

    #[test]
    fn the_sixth_account_is_dropped_with_its_token() {
        let mut accounts = Accounts::default();
        for id in 1..=5 {
            assert!(accounts.seen(&user(id, "x")).is_empty());
        }
        assert_eq!(accounts.seen(&user(6, "x")), [6]);
        assert_eq!(accounts.users.len(), MAX_ACCOUNTS);
    }

    #[test]
    fn the_list_round_trips_through_the_storage_shape() {
        let mut accounts = Accounts::default();
        accounts.seen(&user(10, "one"));
        accounts.seen(&user(20, "two"));
        let back = Accounts::from_json(&accounts.to_json());
        assert_eq!(back.users, accounts.users);
        assert_eq!(accounts.to_json()["users"][0]["id"], "10");
        assert_eq!(accounts.to_json()["users"][0]["tokenStatus"], 2);
    }

    #[test]
    fn removing_an_account_keeps_the_others_in_order() {
        let mut accounts = Accounts::default();
        for id in [1, 2, 3] {
            accounts.seen(&user(id, "x"));
        }
        accounts.remove(2);
        let ids: Vec<u64> = accounts.users.iter().map(|a| a.id).collect();
        assert_eq!(ids, [1, 3]);
    }
}
