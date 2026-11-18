//! The window's root view. It owns the login: which screen shows (the login screen or the main view), where the token
//! lives (the OS credential store), and the network session of the main view. Signing in starts a session without a
//! restart; signing out, a dead token and a gateway close 4004 all end it and bring the login screen back.

use crate::login_art;
use crate::net::{self, Update};
use crate::tray::Action;
use crate::ui::login::{Art, LoginEvent, LoginView};
use crate::ui::{Root, RootEvent};
use dc_auth::credentials::{ACTIVE, Store};
use dc_proto::identity::Identity;
use dc_proto::secrets::Token;
use gpui::{
    AnyElement, Context, Entity, IntoElement, Render, Subscription, Task, Window, div, prelude::*,
};
use tokio::runtime::Handle;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

/// How the app starts.
pub enum Start {
    /// A token was found (the credential store; in capture builds also `secrets.toml`).
    Token(Token),
    Login,
    /// A capture of one login state, with no network: `--login <state>` (capture builds only).
    #[cfg_attr(not(feature = "capture"), allow(dead_code))]
    Preview(String),
}

enum Screen {
    Empty,
    Login(Entity<LoginView>),
    Main(Entity<Root>),
}

pub struct Shell {
    handle: Handle,
    store: Store,
    screen: Screen,
    /// The running session's network task; aborting it closes the gateway (`net::run` shuts it down on drop).
    session: Option<tokio::task::JoinHandle<()>>,
    token: Option<Token>,
    _pump: Option<Task<()>>,
    _art: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
    /// Capture builds never touch the credential store: a run with a test account must not drop the owner's login.
    persist: bool,
    /// The next session replaces another account's: its main view announces the switch.
    switched: bool,
}

impl Shell {
    pub fn new(
        handle: Handle,
        start: Start,
        cached: Option<crate::ui::start_cache::Cached>,
        session: Option<net::Session>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut this = Self {
            handle,
            store: Store::app(),
            screen: Screen::Empty,
            session: None,
            token: None,
            _pump: None,
            _art: None,
            _subscriptions: Vec::new(),
            persist: !cfg!(feature = "capture"),
            switched: false,
        };
        match start {
            Start::Token(token) => this.start_session(token, cached, session, window, cx),
            Start::Login => this.show_login(false, window, cx),
            Start::Preview(state) => {
                this.show_login(true, window, cx);
                if let Screen::Login(login) = &this.screen {
                    login.update(cx, |login, cx| login.preview(&state, cx));
                }
            }
        }
        this
    }

    /// The main view, while there is one (the capture harness drives it).
    #[cfg_attr(not(feature = "capture"), allow(dead_code))]
    pub fn root(&self) -> Option<Entity<Root>> {
        match &self.screen {
            Screen::Main(root) => Some(root.clone()),
            Screen::Empty | Screen::Login(_) => None,
        }
    }

    /// Whether the screen is drawn completely enough to capture.
    #[cfg_attr(not(feature = "capture"), allow(dead_code))]
    pub fn ready(&self, cx: &gpui::App) -> bool {
        match &self.screen {
            Screen::Main(root) => root.read(cx).ready_for_capture(),
            Screen::Login(login) => login.read(cx).has_art(),
            Screen::Empty => false,
        }
    }

    /// Opens a `discord://` or discord.com link in the main view. False while there is no main view yet or it is
    /// still starting: the caller tries again.
    pub fn open_link(&mut self, url: &str, cx: &mut Context<Self>) -> bool {
        match &self.screen {
            Screen::Main(root) if root.read(cx).ready_for_capture() => {
                let root = root.clone();
                root.update(cx, |root, cx| root.open_deep_link(url, cx));
                true
            }
            _ => false,
        }
    }

    /// What reaches the main view from the network task.
    fn route(&mut self, update: Update, window: &mut Window, cx: &mut Context<Self>) {
        // Quit is not a main-view matter: the tray menu and the installer's quit request must work from the login
        // screen and while a session is still starting, where the main view does not exist to act on it.
        if quits_from_any_screen(&update) {
            cx.quit();
            return;
        }
        // Whether the start cache's key (the account read from the login) is READY's account. Only the answer is
        // kept, never the id: a token of another shape would make every start a cold one without this saying so.
        if let Update::Dispatch { name, data } = &update
            && name == "READY"
        {
            let keyed = self
                .token
                .as_ref()
                .and_then(|token| crate::ui::start_cache::token_account(token.expose()));
            let live = data["user"]["id"].as_str().and_then(|id| id.parse().ok());
            crate::start_trace::mark(if keyed.is_some() && keyed == live {
                "cache_key_matches"
            } else {
                "cache_key_differs"
            });
        }
        match (update, &self.screen) {
            (Update::AuthFailed, Screen::Main(root)) => {
                debug!("the stored login is dead");
                root.update(cx, |root, cx| root.leave_for_logout(cx));
                self.end_session(false, window, cx);
            }
            (Update::TokenRenewed(token), Screen::Main(_)) => self.adopt_token(token),
            (update, Screen::Main(root)) => {
                if matches!(&update, Update::NotificationClicked(_)) {
                    crate::notifications::restore_window(window);
                }
                root.update(cx, |root, cx| root.update(update, cx));
            }
            _ => {}
        }
    }

    fn start_session(
        &mut self,
        token: Token,
        preloaded: Option<crate::ui::start_cache::Cached>,
        session: Option<net::Session>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A start with a stored login began its session before the window (`main.rs`); a sign in starts it here.
        let net::Session {
            updates: hook_tx,
            mut update_rx,
            requests: request_tx,
            task,
        } = session.unwrap_or_else(|| net::Session::start(&self.handle, token.clone()));
        let account = crate::ui::start_cache::token_account(token.expose());
        let cached = preloaded
            .filter(|cached| cached.account() == account)
            .or_else(|| crate::ui::start_cache::load(account));
        self.session = Some(task);
        self.token = Some(token);
        let mut cached = cached;
        let pictures = cached
            .as_mut()
            .map(|cached| std::mem::take(&mut cached.pictures))
            .unwrap_or_default();
        let root = cx.new(|cx| Root::new(request_tx, hook_tx, cx));
        root.update(cx, |root, cx| {
            // The cached pictures start decoding first, on the background threads, while the rest of the cache is
            // applied here: the first frame has them decoded.
            root.add_start_pictures(pictures);
            root.decode_start_pictures(window, cx);
            if let Some(cached) = cached {
                root.apply_cache(cached, cx);
            }
        });
        if std::mem::take(&mut self.switched) {
            root.update(cx, |root, _| root.announce_switch());
        }
        crate::ui::bind_focus(&root, window, cx);
        crate::ui::memory_trim::start(&root, cx);
        self._subscriptions = vec![
            cx.subscribe_in(&root, window, |this, _, event, window, cx| match event {
                RootEvent::AccountSeen(id) => this.keep_account_token(*id),
                RootEvent::AccountRemoved(id) => this.forget_account_token(*id),
                RootEvent::SwitchAccount(id) => this.switch_account(*id, window, cx),
                RootEvent::LogOut => {
                    if !crate::inert::skip("log out") {
                        crate::ui::start_cache::forget();
                        this.end_session(true, window, cx);
                    }
                }
            }),
        ];
        self.screen = Screen::Main(root);
        self._pump = Some(cx.spawn_in(window, async move |this, cx| {
            while let Some(update) = update_rx.recv().await {
                if this
                    .update_in(cx, |this, window, cx| this.route(update, window, cx))
                    .is_err()
                {
                    return;
                }
            }
        }));
        cx.notify();
    }

    /// Ends the session and shows the login screen. `server`: the user asked to log out, so the token is retired at
    /// Discord too; a token Discord already refused is only dropped here.
    fn end_session(&mut self, server: bool, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(session) = self.session.take() {
            session.abort();
        }
        self._pump = None;
        let token = self.token.take();
        if self.persist {
            let store = self.store.clone();
            self.handle.spawn_blocking(move || {
                if let Err(error) = store.forget(ACTIVE) {
                    warn!("the stored login could not be removed: {error:?}");
                }
            });
        }
        if server && let Some(token) = token {
            self.handle.spawn(async move {
                if let Ok(api) = dc_auth::api::AuthApi::pinned(Identity::new()) {
                    let _ = api.logout(&token).await;
                }
            });
        }
        self.show_login(false, window, cx);
    }

    /// `offline` (a `--login <state>` capture) starts no controller: nothing reaches the network, and the state shown
    /// is the one the preview set.
    fn show_login(&mut self, offline: bool, window: &mut Window, cx: &mut Context<Self>) {
        crate::ui::set_theme(dc_tokens::Theme::Darker);
        // The controller spawns its tasks on the current runtime.
        let guard = self.handle.enter();
        let started = (!offline).then(|| dc_auth::spawn(dc_auth::Config::pinned()));
        drop(guard);
        let (commands, events) = match started {
            Some(Ok(pair)) => pair,
            other => {
                if let Some(Err(error)) = other {
                    warn!("the login controller did not start: {error}");
                }
                let (commands, _) = mpsc::unbounded_channel();
                let (_, events) = mpsc::unbounded_channel();
                (commands, events)
            }
        };
        let (avatar_request_tx, mut avatar_request_rx) =
            mpsc::unbounded_channel::<(String, String)>();
        let (avatar_tx, avatar_rx) = mpsc::unbounded_channel::<Vec<u8>>();
        let client = dc_http::Client::new(Token::new(""), Identity::new())
            .ok()
            .map(std::sync::Arc::new);
        if let Some(client) = client.clone() {
            self.handle.spawn(async move {
                while let Some((user, hash)) = avatar_request_rx.recv().await {
                    if let Some(bytes) = login_art::avatar(&client, &user, &hash).await {
                        let _ = avatar_tx.send(bytes);
                    }
                }
            });
        }
        let login =
            cx.new(|cx| LoginView::new(commands, events, Some((avatar_request_tx, avatar_rx)), cx));
        login.update(cx, |login, cx| login.focus_first(window, cx));
        self._subscriptions =
            vec![
                cx.subscribe_in(&login, window, |this, _, event, window, cx| match event {
                    LoginEvent::Success(token) => this.signed_in(token.clone(), window, cx),
                }),
            ];
        // The artwork comes from the CDN once, then from disk.
        let (art_tx, mut art_rx) = mpsc::unbounded_channel();
        if let Some(client) = client {
            self.handle.spawn(async move {
                let _ = art_tx.send(login_art::load(&client).await);
            });
        }
        let view = login.clone();
        self._art = Some(cx.spawn(async move |_, cx| {
            if let Some(bytes) = art_rx.recv().await {
                let art = Art::from_bytes(bytes);
                view.update(cx, |login, cx| login.set_art(art, cx));
            }
        }));
        self.screen = Screen::Login(login);
        self._pump = None;
        cx.notify();
    }

    /// The signed in account's token goes into its own credential entry, so Switch Accounts can come back to it.
    fn keep_account_token(&self, id: u64) {
        let (true, Some(token)) = (self.persist, self.token.clone()) else {
            return;
        };
        let store = self.store.clone();
        let entry = dc_auth::credentials::account_entry(&id.to_string());
        self.handle.spawn_blocking(move || {
            if let Err(error) = store.save(&entry, &token) {
                warn!("the account's login could not be kept: {error:?}");
            }
        });
    }

    fn forget_account_token(&self, id: u64) {
        if !self.persist {
            return;
        }
        let store = self.store.clone();
        let entry = dc_auth::credentials::account_entry(&id.to_string());
        self.handle.spawn_blocking(move || {
            if let Err(error) = store.forget(&entry) {
                warn!("the account's login could not be removed: {error:?}");
            }
        });
    }

    /// Switch Accounts: the session of the account in use ends without retiring its token, and a session opens with
    /// the chosen account's stored one. A harness run only reports what it would connect to.
    fn switch_account(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        if !self.persist || crate::inert::skip("switch account") {
            info!("would connect with account {id}");
            return;
        }
        let entry = dc_auth::credentials::account_entry(&id.to_string());
        let Ok(Some(token)) = self.store.load(&entry) else {
            warn!("account {id} has no stored login, staying signed in");
            if let Screen::Main(root) = &self.screen {
                root.update(cx, |root, cx| root.switch_failed(cx));
            }
            return;
        };
        if let Some(session) = self.session.take() {
            session.abort();
        }
        self._pump = None;
        crate::ui::start_cache::forget();
        self.switched = true;
        self.signed_in(token, window, cx);
    }

    /// The account's token was renewed (a password change): the stored login and the one kept for this run switch
    /// to it, so the next start does not find a dead token. The value is never logged.
    fn adopt_token(&mut self, token: Token) {
        if self.persist {
            let store = self.store.clone();
            let saved = token.clone();
            self.handle.spawn_blocking(move || {
                if let Err(error) = store.save(ACTIVE, &saved) {
                    warn!("the renewed login could not be stored: {error:?}");
                }
            });
        }
        self.token = Some(token);
    }

    fn signed_in(&mut self, token: Token, window: &mut Window, cx: &mut Context<Self>) {
        if self.persist {
            let store = self.store.clone();
            let saved = token.clone();
            self.handle.spawn_blocking(move || {
                if let Err(error) = store.save(ACTIVE, &saved) {
                    warn!("the login could not be stored, it lasts for this run only: {error:?}");
                }
            });
        }
        self.start_session(token, None, None, window, cx);
    }
}

impl Render for Shell {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let screen: AnyElement = match &self.screen {
            Screen::Empty => div().into_any_element(),
            Screen::Login(login) => login.clone().into_any_element(),
            Screen::Main(root) => root.clone().into_any_element(),
        };
        div().size_full().child(screen)
    }
}

/// Updates that end the app whatever screen is showing.
fn quits_from_any_screen(update: &Update) -> bool {
    matches!(update, Update::Tray(Action::Quit))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tray_quit_is_handled_before_the_screen_is_consulted() {
        assert!(quits_from_any_screen(&Update::Tray(Action::Quit)));
        assert!(!quits_from_any_screen(&Update::Tray(Action::Mute)));
        assert!(!quits_from_any_screen(&Update::AuthFailed));
    }
}
