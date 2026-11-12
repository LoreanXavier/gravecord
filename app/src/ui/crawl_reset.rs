//! Capture only: `reset` takes the window back to one known place (the Home friends page) without relying on the
//! keyboard, so that a crawl or a script can start every screen from the same state however the last one ended.

use super::Root;
use super::channel_settings::Section;
use super::server_settings::Page;
use gpui::{Context, Window};

impl Root {
    /// Closes every popout, menu, dialog and full-window page, drops edits and replies, goes Home and gives the
    /// window's own handle focus. Returns what was open, for the log.
    pub(crate) fn script_reset(&mut self, window: &mut Window, cx: &mut Context<Self>) -> String {
        let mut closed: Vec<String> = Vec::new();
        for _ in 0..8 {
            let open = self.open_layers();
            if open.is_empty() {
                break;
            }
            for layer in open {
                closed.push(format!("{layer:?}"));
                self.close_layer(layer, cx);
            }
        }
        self.polls.close_modals();
        macro_rules! drop_all {
            ($($field:ident),* $(,)?) => {$(
                if self.$field.is_some() {
                    closed.push(stringify!($field).to_owned());
                    self.$field = None;
                }
            )*};
        }
        drop_all!(
            settings,
            server_settings,
            channel_settings,
            profile_editor,
            quick_switcher,
            lightbox,
            full_profile,
            guild_events,
            create_channel,
            add_server,
            group_dm,
            group_edit,
            group_leave,
            invite,
            forward,
            report,
            view_reactions,
            custom_status,
            custom_reminder,
            modify_attachment,
            search,
            search_filters,
            new_thread,
            member_view,
            add_friend,
            friend_nickname,
            confirm_remove_friend,
            remediation,
            camera_error,
            go_live,
            activity_shelf,
            screening,
            file_expanded,
            confirm_delete,
            confirm_pin,
            confirm_unpin,
            leave_guild,
            destructive,
            leaving,
            poll,
            editing,
            replying,
            security_actions,
            too_long,
            snooze_menu,
            menu,
            submenu,
        );
        // Composer residue of the last screen: text, a half-filled command, an attached sticker or files.
        self.cancel_pending(cx);
        let _ = self.sticker_ui.take_staged();
        self.pending_files.clear();
        self.logout_confirm = false;
        self.notify_open = false;
        self.plus_menu = false;
        self.select_home(None);
        window.focus(&self.focus, cx);
        cx.notify();
        format!("reset: closed [{}]", closed.join(", "))
    }

    /// `server-settings <page>`: Server Settings of the server in view (the fixture server when none is) on a page named
    /// as the variant, in lower case: `emoji`, `roles`, `automod`.
    pub(crate) fn script_server_settings(&mut self, name: &str, cx: &mut Context<Self>) -> String {
        const PAGES: [Page; 23] = [
            Page::Profile,
            Page::Tag,
            Page::Engagement,
            Page::BoostPerks,
            Page::Emoji,
            Page::Stickers,
            Page::Soundboard,
            Page::Members,
            Page::Roles,
            Page::Invites,
            Page::Access,
            Page::Integrations,
            Page::AppDirectory,
            Page::SafetySetup,
            Page::AuditLog,
            Page::Bans,
            Page::AutoMod,
            Page::Community,
            Page::Onboarding,
            Page::Insights,
            Page::Welcome,
            Page::Landing,
            Page::Template,
        ];
        let Some(page) = PAGES
            .iter()
            .copied()
            .find(|page| format!("{page:?}").eq_ignore_ascii_case(name))
        else {
            return format!("server-settings: no page {name}");
        };
        let guild = self.guild.unwrap_or(1_568_692_564_637_524_109);
        self.open_server_settings_at(guild, page, cx);
        cx.notify();
        format!("server-settings: {page:?}")
    }

    /// `channel-settings <section>`: Channel Settings of the open text channel on `overview`, `permissions`, `invites` or
    /// `integrations`.
    pub(crate) fn script_channel_settings(
        &mut self,
        name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> String {
        let Some(channel) = self.channel else {
            return "channel-settings: no channel open".to_owned();
        };
        self.open_channel_settings(channel, window, cx);
        let section = match name {
            "permissions" => Section::Permissions,
            "invites" => Section::Invites,
            "integrations" => Section::Integrations,
            _ => Section::Overview,
        };
        if let Some(open) = self.channel_settings.as_mut() {
            open.section = section;
        }
        cx.notify();
        format!("channel-settings: {section:?}")
    }
}

impl Root {
    /// The open layers as one string, for the crawler to tell that a right press opened a menu.
    pub(crate) fn crawl_layers(&self) -> String {
        format!("{:?}", self.open_layers())
    }
}
