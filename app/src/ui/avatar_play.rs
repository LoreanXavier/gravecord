//! Animated avatars: when one plays and which file it asks for.
//!
//! Module 45837 (`useAnimatedAvatar`): `shouldAnimate = focused && (hovered || (!reducedMotion && !animateOnHover &&
//! gifAutoPlay))`, `gifAutoPlay` being the setting `animate_gifs`. Module 486020 (`getUserAvatarURL`) adds
//! `animated=true` only to a hash starting with `a_`, and asks for the drawn size times the pixel ratio rounded up to the
//! next step of module 776231's size ladder (`cdn::side_for`). The message author's `animateOnHover` is on, and its hover is the message row's
//! (module 549587); the profile popout and full profile are drawn as hovered.
use dc_state::User;
use gpui::{AnyElement, Context, IntoElement, div, prelude::*, px};

/// Module 45837, argument for argument.
pub(super) fn should_animate(
    focused: bool,
    hovered: bool,
    reduced_motion: bool,
    animate_on_hover: bool,
    auto_play: bool,
) -> bool {
    focused && (hovered || (!reduced_motion && !animate_on_hover && auto_play))
}

pub(super) fn is_animated(hash: &str) -> bool {
    hash.starts_with("a_")
}

/// The file side module 486020 asks for: `kr(drawn * devicePixelRatio)`, the next step of module 776231's ladder.
pub(super) fn file_side(drawn: f32, scale: f32) -> u32 {
    super::cdn::side_for(drawn, scale)
}

pub(super) fn animated_url(base: &str, side: u32) -> String {
    format!("{base}.webp?size={side}&animated=true")
}

impl super::Root {
    /// Asks for `url` on the next frame (see `prepare_live_avatars`).
    pub(super) fn want_image(&self, key: String, url: String, cx: &mut Context<Self>) {
        if !self.requested.contains(&key) {
            self.avatar_wants.borrow_mut().push((key, url));
            cx.notify();
        }
    }

    /// Whether the pointer is over the surface with this key (see `avatar_hover`).
    pub(super) fn avatar_hovered(&self, key: &str) -> bool {
        self.avatar_over.as_deref() == Some(key)
    }

    /// The hover handler of a surface whose avatar plays while the pointer is over it.
    pub(super) fn avatar_hover(
        &self,
        key: String,
        cx: &mut Context<Self>,
    ) -> impl Fn(&bool, &mut gpui::Window, &mut gpui::App) + 'static {
        cx.listener(move |this, inside: &bool, _, cx| {
            let was = this.avatar_over.clone();
            if *inside {
                this.avatar_over = Some(key.clone());
            } else if this.avatar_over.as_deref() == Some(key.as_str()) {
                this.avatar_over = None;
            }
            if was != this.avatar_over {
                cx.notify();
            }
        })
    }

    /// The animated file of a user's avatar in a server (their server avatar first), as a cache key and URL.
    fn live_avatar_source(&self, user: &User, side: u32) -> Option<(String, String)> {
        let guild_hash = self.guild.and_then(|guild| {
            self.stores
                .members
                .get(guild, user.id)?
                .avatar
                .clone()
                .map(|hash| (guild, hash))
        });
        let (base, hash) = match guild_hash {
            Some((guild, hash)) => (
                format!(
                    "https://cdn.discordapp.com/guilds/{guild}/users/{}/avatars/{hash}",
                    user.id
                ),
                hash,
            ),
            None => {
                let hash = user.avatar.clone()?;
                (
                    format!("https://cdn.discordapp.com/avatars/{}/{hash}", user.id),
                    hash,
                )
            }
        };
        is_animated(&hash).then(|| {
            (
                format!("avatar-live:{base}:{side}"),
                animated_url(&base, side),
            )
        })
    }

    /// The animated file of `user`'s avatar when it should be playing now and has arrived: its cache key and picture.
    /// While the gate is open and the file has not arrived, it is asked for on the next frame; while the gate is shut
    /// nothing is asked for and nothing is decoded.
    pub(super) fn live_image(
        &self,
        user: &User,
        drawn: f32,
        hovered: bool,
        cx: &mut Context<Self>,
    ) -> Option<(String, std::sync::Arc<gpui::Image>)> {
        let side = file_side(drawn, self.ui_scale.get());
        let (key, url) = self.live_avatar_source(user, side)?;
        if !should_animate(
            super::motion::window_active(),
            // Not in the pinned client: AlwaysAnimate's `icons` (`plugin_hooks/always_animate.rs`).
            hovered || super::plugin_hooks::always_animate(super::plugin_hooks::AnimatePart::Icons),
            super::motion::reduced(),
            true,
            self.pref_on("animate_gifs", true),
        ) {
            return None;
        }
        let Some(image) = self.images.get(&key).cloned() else {
            if !self.requested.contains(&key) {
                self.avatar_wants.borrow_mut().push((key, url));
                cx.notify();
            }
            return None;
        };
        Some((key, image))
    }

    /// The playing picture of an avatar drawn `size` points wide, or `None` when it is still (the caller draws the
    /// still one).
    pub(super) fn live_avatar(
        &self,
        user: &User,
        size: f32,
        hovered: bool,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let (key, image) = self.live_image(user, size, hovered, cx)?;
        let side = (size * self.ui_scale.get()).round() as u32;
        Some(
            div()
                .relative()
                .size(px(size))
                .child(self.gif_art_pixels(&key, image, (side, side), size / 2., None, cx))
                .into_any_element(),
        )
    }

    /// The server icon on the rail while it plays, and `None` while the still one is drawn. Module 396655 (the rail item):
    /// the icon is asked for with `canAnimate = hovered && animatable`, `animatable` being the window's focus and
    /// `hovered` the pointer being over that rail item; the setting for animated GIFs and reduced motion do not enter. The
    /// file is `getGuildIconURL({size: 2 * 40, lossless})` (module 486020): `.webp?size=<80 * ratio to a power of two>&
    /// quality=lossless&animated=true` for a hash that starts with `a_`.
    pub(super) fn live_guild_icon(
        &self,
        guild: &dc_state::Guild,
        hovered: bool,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let hash = guild.icon.as_deref().filter(|hash| is_animated(hash))?;
        // Not in the pinned client: AlwaysAnimate's `icons` plays it all the time (`plugin_hooks/always_animate.rs`).
        let always = super::plugin_hooks::always_animate(super::plugin_hooks::AnimatePart::Icons);
        if !always && (!hovered || !super::motion::window_active()) {
            return None;
        }
        let scale = self.ui_scale.get();
        let side = file_side(80., scale);
        let key = format!("guild-live:{}:{hash}:{side}", guild.id);
        let Some(image) = self.images.get(&key).cloned() else {
            if !self.requested.contains(&key) {
                let url = format!(
                    "https://cdn.discordapp.com/icons/{}/{hash}.webp?size={side}&quality=lossless&animated=true",
                    guild.id
                );
                self.avatar_wants.borrow_mut().push((key, url));
                cx.notify();
            }
            return None;
        };
        let pixels = (40. * scale).round() as u32;
        Some(
            div()
                .relative()
                .size(px(40.))
                .child(self.gif_art_pixels(&key, image, (pixels, pixels), 12., None, cx))
                .into_any_element(),
        )
    }

    /// The cached users with an animated avatar, for the harness.
    #[cfg(feature = "capture")]
    pub(crate) fn avatar_report(&self) -> String {
        let mut lines: Vec<String> = self
            .stores
            .users
            .values()
            .filter(|user| user.avatar.as_deref().is_some_and(is_animated))
            .map(|user| {
                format!(
                    "{}={}:{}",
                    user.id,
                    user.display_name(),
                    user.avatar.as_deref().unwrap_or_default()
                )
            })
            .collect();
        lines.sort();
        let guild: Vec<String> = self
            .guild
            .map(|guild| {
                self.stores
                    .members
                    .of(guild)
                    .iter()
                    .filter(|member| member.avatar.as_deref().is_some_and(is_animated))
                    .map(|member| format!("{}", member.user.id))
                    .collect()
            })
            .unwrap_or_default();
        let emoji: Vec<String> = self
            .channel
            .and_then(|channel| self.stores.messages.get(&channel))
            .into_iter()
            .flatten()
            .flat_map(|message| {
                message
                    .content
                    .match_indices("<a:")
                    .map(|(at, _)| {
                        let tail = &message.content[at..];
                        let id: String = tail
                            .rsplit_once(':')
                            .map(|_| {
                                tail.split(':')
                                    .nth(2)
                                    .unwrap_or_default()
                                    .chars()
                                    .take_while(char::is_ascii_digit)
                                    .collect()
                            })
                            .unwrap_or_default();
                        let loaded = self.images.contains_key(&format!("custom-animated:{id}"));
                        format!("{id}:loaded={loaded}")
                    })
                    .collect::<Vec<_>>()
            })
            .collect();
        format!(
            "animated_emoji=[{}] active={} reduced={} live_images={} live_requested={} tiles=[{}] hovered_member={:?} users={} animated={} [{}] server_avatars={}",
            emoji.join(","),
            super::motion::window_active(),
            super::motion::reduced(),
            self.images
                .keys()
                .filter(|key| key.starts_with("avatar-live:"))
                .count(),
            self.requested
                .iter()
                .filter(|key| key.starts_with("avatar-live:"))
                .count(),
            self.gif_tiles.borrow().len(),
            self.hovered_nameplate,
            self.stores.users.len(),
            lines.len(),
            lines.join(", "),
            guild.len()
        )
    }

    /// Scrolls the message list to the `nth` message of a user with an animated avatar; the row index, or none.
    #[cfg(feature = "capture")]
    pub(crate) fn avatar_goto(&mut self, nth: usize) -> Option<usize> {
        let channel = self.channel?;
        let list = self.stores.messages.get(&channel)?;
        let found: Vec<usize> = self
            .rows
            .iter()
            .enumerate()
            .filter_map(|(row, kind)| match kind {
                super::Row::Message { index, head: true } => {
                    let author = &list.get(*index)?.author;
                    author
                        .avatar
                        .as_deref()
                        .is_some_and(is_animated)
                        .then_some(row)
                }
                _ => None,
            })
            .collect();
        let row = *found.get(nth)?;
        self.list.scroll_to(gpui::ListOffset {
            item_ix: row,
            offset_in_item: px(0.),
        });
        Some(row)
    }

    /// Scrolls the member panel to the `nth` member with an animated avatar; the pixel offset and the name, or none.
    #[cfg(feature = "capture")]
    pub(crate) fn avatar_member(&mut self, name: &str) -> Option<String> {
        use dc_state::member_list::Row as MemberRow;
        let list = self
            .guild
            .and_then(|guild| self.stores.member_lists.get(&guild))?;
        let mut y: f32 = 0.;
        let mut found = Vec::new();
        for row in &list.rows {
            let height = match row {
                MemberRow::Member { user, .. } if self.has_presence_line(user, self.guild) => 44.,
                _ => 42.,
            };
            if let MemberRow::Member { user, .. } = row
                && user.avatar.as_deref().is_some_and(is_animated)
            {
                found.push((y, user.display_name().to_owned()));
            }
            y += height;
        }
        let (y, name) = found
            .iter()
            .find(|(_, found)| found.contains(name))?
            .clone();
        // The pointer cannot be held on a row of a list that reorders itself; the row is marked hovered instead.
        self.hovered_nameplate = self
            .stores
            .users
            .values()
            .find(|user| user.display_name() == name)
            .map(|user| user.id);
        let scroll = &self.member_window.scroll;
        let at = scroll.offset();
        scroll.set_offset(gpui::point(at.x, px(-(y - 120.).max(0.))));
        Some(format!("{name} at {y} of {} animated", found.len()))
    }

    /// Asks for the animated files `live_avatar` found missing.
    pub(super) fn prepare_live_avatars(&mut self) {
        let wants: Vec<_> = self.avatar_wants.borrow_mut().drain(..).collect();
        for (key, url) in wants {
            self.request_image(key, url);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_gate_is_module_45837() {
        // (focused, hovered, reduced, animate_on_hover, auto_play) -> plays
        let table = [
            (true, true, true, true, false, true),
            (true, true, false, false, false, true),
            (false, true, false, false, true, false),
            (true, false, false, false, true, true),
            (true, false, false, true, true, false),
            (true, false, true, false, true, false),
            (true, false, false, false, false, false),
            (false, false, false, false, true, false),
        ];
        for (focused, hovered, reduced, on_hover, auto, plays) in table {
            assert_eq!(
                should_animate(focused, hovered, reduced, on_hover, auto),
                plays,
                "{focused} {hovered} {reduced} {on_hover} {auto}"
            );
        }
    }

    #[test]
    fn only_a_hashes_get_the_animated_file_and_sizes_are_powers_of_two() {
        assert!(is_animated("a_1f"));
        assert!(!is_animated("1f"));
        assert_eq!(file_side(40., 1.), 40);
        assert_eq!(file_side(40., 1.5), 60);
        assert_eq!(file_side(32., 1.), 32);
        assert_eq!(file_side(8., 1.), 16);
        assert_eq!(file_side(120., 1.25), 160);
        assert_eq!(file_side(5000., 2.), 4096);
        assert_eq!(
            animated_url("https://cdn.discordapp.com/avatars/1/a_f", 64),
            "https://cdn.discordapp.com/avatars/1/a_f.webp?size=64&animated=true"
        );
    }
}
