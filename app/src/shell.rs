//! The window's root view. It owns the login: which screen shows (the login screen or the main view), where the token
//! lives (the OS credential store), and the network session of the main view. Signing in starts a session without a
//! restart; signing out, a dead token and a gateway close 4004 all end it and bring the login screen back.

use crate::login_art;
use crate::net::{self, Update};
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
use tracing::{debug, warn};

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
}

impl Shell {
    pub fn new(handle: Handle, start: Start, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let placeholder = cx.new(|cx| {
            let (commands, _) = mpsc::unbounded_channel();
            let (_, events) = mpsc::unbounded_channel();
            LoginView::new(commands, events, None, cx)
        });
        let mut this = Self {
            handle,
            store: Store::app(),
            screen: Screen::Login(placeholder),
            session: None,
            token: None,
            _pump: None,
            _art: None,
            _subscriptions: Vec::new(),
            persist: !cfg!(feature = "capture"),
        };
        match start {
            Start::Token(token) => this.start_session(token, window, cx),
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
            Screen::Login(_) => None,
        }
    }

    /// Whether the screen is drawn completely enough to capture.
    #[cfg_attr(not(feature = "capture"), allow(dead_code))]
    pub fn ready(&self, cx: &gpui::App) -> bool {
        match &self.screen {
            Screen::Main(root) => root.read(cx).ready,
            Screen::Login(login) => login.read(cx).has_art(),
        }
    }

    /// What reaches the main view from the network task.
    fn route(&mut self, update: Update, window: &mut Window, cx: &mut Context<Self>) {
        match (update, &self.screen) {
            (Update::AuthFailed, Screen::Main(root)) => {
                debug!("the stored login is dead");
                root.update(cx, |root, cx| root.leave_for_logout(cx));
                self.end_session(false, window, cx);
            }
            (update, Screen::Main(root)) => {
                root.update(cx, |root, cx| root.update(update, cx));
            }
            _ => {}
        }
    }

    fn start_session(&mut self, token: Token, window: &mut Window, cx: &mut Context<Self>) {
        let (update_tx, mut update_rx) = mpsc::unbounded_channel();
        let (request_tx, request_rx) = mpsc::unbounded_channel();
        let hook_tx = update_tx.clone();
        self.session = Some(
            self.handle
                .spawn(net::run(token.clone(), update_tx, request_rx)),
        );
        self.token = Some(token);
        let root = cx.new(|cx| Root::new(request_tx, hook_tx, cx));
        crate::ui::bind_focus(&root, window, cx);
        self._subscriptions = vec![
            cx.subscribe_in(&root, window, |this, _, event, window, cx| match event {
                RootEvent::LogOut => this.end_session(true, window, cx),
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
        self.start_session(token, window, cx);
    }
}

impl Render for Shell {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let screen: AnyElement = match &self.screen {
            Screen::Login(login) => login.clone().into_any_element(),
            Screen::Main(root) => root.clone().into_any_element(),
        };
        div().size_full().child(screen)
    }
}
