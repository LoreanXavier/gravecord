//! What the call view's tiles show: which tiles exist, in which order, and how a decoded picture gets onto one.
//!
//! The rules are the official `ChannelRTCStore` (module 568598 `participantByIndex` and its sort key `w`) and the
//! video tile of module 83982: with "Show Non-Video Participants" off only tiles with video stay (`FILTERED`), a user
//! whose camera is switched off locally (`localVideoDisabled`, our own "Show My Own Camera") is not drawn as video,
//! and tiles are sorted by a key that puts streams first, then cameras, then Go Live users, then the rest.
//!
//! Pictures: [`Slots`] holds one gpui image per source and the sequence number it was made from. The conversion
//! (`Picture::to_rgba` and the swap to BGRA, which is what gpui's atlas takes) runs on a worker; a finished image
//! replaces the old one, and the old one is handed back so the caller can release it from the atlas with
//! `App::drop_image`. That keeps memory flat however long a call lasts.

use dc_voice::VideoSource;
use gpui::RenderImage;
use std::collections::HashMap;
use std::sync::Arc;

/// One source of pictures: a user's camera, or a stream (owned by the user).
pub(super) type Key = (u64, VideoSource);

// ---- image slots ----

struct Slot<T> {
    image: Option<T>,
    /// The picture sequence number `image` was made from.
    shown: u64,
    /// A conversion of a newer picture is running.
    pending: bool,
}

/// The image of every source, with the lifetime rules in one place so they can be tested without a window.
pub(super) struct Slots<T> {
    slots: HashMap<Key, Slot<T>>,
}

impl<T> Default for Slots<T> {
    fn default() -> Self {
        Self {
            slots: HashMap::new(),
        }
    }
}

impl<T> Slots<T> {
    /// A picture with sequence number `latest` exists: whether a conversion should start now. At most one conversion
    /// runs per source, so a slow worker never queues work.
    pub fn wanted(&mut self, key: Key, latest: u64) -> bool {
        let slot = self.slots.entry(key).or_insert(Slot {
            image: None,
            shown: 0,
            pending: false,
        });
        if slot.pending || latest <= slot.shown {
            return false;
        }
        slot.pending = true;
        true
    }

    /// The conversion of picture `seq` is done (`None` when the picture was invalid). The image it replaces comes
    /// back, to be released.
    pub fn finish(&mut self, key: Key, seq: u64, image: Option<T>) -> Option<T> {
        let slot = self.slots.get_mut(&key)?;
        slot.pending = false;
        let image = image?;
        slot.shown = seq;
        slot.image.replace(image)
    }

    pub fn image(&self, key: &Key) -> Option<&T> {
        self.slots.get(key)?.image.as_ref()
    }

    /// Drops every source `keep` rejects; the images that were held come back.
    pub fn retain(&mut self, keep: impl Fn(&Key) -> bool) -> Vec<T> {
        let gone: Vec<Key> = self
            .slots
            .keys()
            .filter(|key| !keep(key))
            .copied()
            .collect();
        gone.into_iter()
            .filter_map(|key| self.slots.remove(&key)?.image)
            .collect()
    }

    pub fn clear(&mut self) -> Vec<T> {
        self.retain(|_| false)
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.slots.len()
    }
}

/// A gpui image of a picture: `None` for an invalid one. gpui's atlas wants BGRA.
pub(super) fn render_image(picture: &dc_media::Picture) -> Option<Arc<RenderImage>> {
    let mut rgba = picture.to_rgba()?;
    for pixel in rgba.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }
    let buffer = image::RgbaImage::from_raw(picture.width, picture.height, rgba)?;
    Some(Arc::new(RenderImage::new(vec![image::Frame::new(buffer)])))
}

// ---- which tiles ----

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum TileKind {
    /// A person: their camera when it is on, else their avatar.
    User,
    /// A Go Live stream of the person.
    Stream,
}

/// What the official participant carries that the selection and the order read.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Candidate {
    pub user: u64,
    pub kind: TileKind,
    pub name: String,
    /// The user's voice state says `self_video`.
    pub video: bool,
    /// The user's voice state says `self_stream`.
    pub going_live: bool,
    /// Our own camera is hidden from us (`isLocalVideoDisabled`).
    pub local_video_disabled: bool,
    /// A stream tile whose stream we send or watch (`streamId` is set).
    pub connected: bool,
}

impl Candidate {
    /// `w(e)`: `\x02` stream of a user with camera, `\x03` other stream; users: `\x03` with camera, `\x04` going live,
    /// `\x05` otherwise; then the lower-cased name padded to 32 with `!`, then the id.
    pub fn sort_key(&self) -> String {
        let lead = match self.kind {
            TileKind::Stream if self.video => '\x02',
            TileKind::Stream => '\x03',
            TileKind::User if self.video => '\x03',
            TileKind::User if self.going_live => '\x04',
            TileKind::User => '\x05',
        };
        let mut name = self.name.to_lowercase();
        while name.chars().count() < 32 {
            name.push('!');
        }
        let tail = if self.kind == TileKind::Stream {
            "\x03"
        } else {
            ""
        };
        format!("{lead}{name}{}{tail}", self.user)
    }

    /// `FILTERED`: what stays while non-video participants are hidden.
    fn has_video(&self) -> bool {
        match self.kind {
            TileKind::User => self.video && !self.local_video_disabled,
            TileKind::Stream => self.connected,
        }
    }
}

/// The tiles of the grid, sorted. `show_non_video` is "Show Non-Video Participants" (`!getVoiceParticipantsHidden`).
pub(super) fn select(mut all: Vec<Candidate>, show_non_video: bool) -> Vec<Candidate> {
    all.sort_by_cached_key(Candidate::sort_key);
    if !show_non_video {
        all.retain(Candidate::has_video);
    }
    all
}

pub(super) fn hides_participants(all: &[Candidate], show_non_video: bool) -> bool {
    !show_non_video && all.iter().any(|candidate| !candidate.has_video())
}

/// Whether the video of a user tile is drawn (module 83982: `!localVideoDisabled && hasVideo`), else the avatar.
pub(super) fn shows_video(candidate: &Candidate) -> bool {
    candidate.kind == TileKind::User && candidate.video && !candidate.local_video_disabled
}

// ---- focus ----

/// The tile the grid is zoomed on (`selectedParticipant`), or none for the grid.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Focus {
    pub tile: Option<(u64, TileKind)>,
}

impl Focus {
    /// A click on a tile: focus it, or back to the grid when it already is the focused one.
    pub fn click(&mut self, user: u64, kind: TileKind) {
        let tile = Some((user, kind));
        self.tile = if self.tile == tile { None } else { tile };
    }

    pub fn grid(&mut self) {
        self.tile = None;
    }

    /// The focused tile left the call (or its stream ended): the view goes back to the grid, as the official store
    /// does when the selected participant disappears.
    pub fn keep_if(&mut self, exists: impl Fn(u64, TileKind) -> bool) {
        if let Some((user, kind)) = self.tile
            && !exists(user, kind)
        {
            self.tile = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(id: u64, name: &str, video: bool) -> Candidate {
        Candidate {
            user: id,
            kind: TileKind::User,
            name: name.to_owned(),
            video,
            going_live: false,
            local_video_disabled: false,
            connected: false,
        }
    }

    fn stream(id: u64, name: &str, connected: bool) -> Candidate {
        Candidate {
            kind: TileKind::Stream,
            connected,
            // The owner has a camera on: the stream sorts first (``).
            ..user(id, name, true)
        }
    }

    #[test]
    fn hidden_notice_requires_a_participant_removed_by_the_filter() {
        assert!(!hides_participants(&[], false));
        let people = vec![user(1, "one", false)];
        assert!(!hides_participants(&people, true));
        assert_eq!(select(people.clone(), true).len(), 1);
        assert!(hides_participants(&people, false));
        assert!(select(people, false).is_empty());

        let cameras = vec![user(2, "two", true), stream(2, "two", true)];
        assert!(!hides_participants(&cameras, false));
        assert_eq!(select(cameras, false).len(), 2);
        let hidden_camera = vec![Candidate {
            local_video_disabled: true,
            ..user(2, "two", true)
        }];
        assert!(hides_participants(&hidden_camera, false));
        assert!(select(hidden_camera, false).is_empty());
    }

    #[test]
    fn a_slot_converts_one_picture_at_a_time_and_hands_back_the_old_image() {
        let mut slots: Slots<&'static str> = Slots::default();
        let key = (1, VideoSource::Camera);
        assert!(
            slots.wanted(key, 1),
            "the first picture starts a conversion"
        );
        assert!(!slots.wanted(key, 2), "a second one waits while one runs");
        assert_eq!(slots.finish(key, 1, Some("a")), None);
        assert_eq!(slots.image(&key), Some(&"a"));
        assert!(
            !slots.wanted(key, 1),
            "a picture already shown is not converted again"
        );
        assert!(slots.wanted(key, 5));
        assert_eq!(
            slots.finish(key, 5, Some("b")),
            Some("a"),
            "the old image is released"
        );
        assert_eq!(slots.image(&key), Some(&"b"));
    }

    #[test]
    fn an_invalid_picture_keeps_the_old_image_and_frees_the_slot() {
        let mut slots: Slots<u8> = Slots::default();
        let key = (1, VideoSource::Camera);
        assert!(slots.wanted(key, 1));
        slots.finish(key, 1, Some(1));
        assert!(slots.wanted(key, 2));
        assert_eq!(slots.finish(key, 2, None), None);
        assert_eq!(slots.image(&key), Some(&1));
        assert!(slots.wanted(key, 3), "the next picture is tried");
    }

    #[test]
    fn memory_stays_flat_over_many_pictures() {
        let mut slots: Slots<u32> = Slots::default();
        let key = (1, VideoSource::Stream(1));
        let mut released = 0;
        for seq in 1..=1000 {
            if slots.wanted(key, seq) && slots.finish(key, seq, Some(seq as u32)).is_some() {
                released += 1;
            }
        }
        assert_eq!(slots.len(), 1);
        assert_eq!(released, 999, "every replaced image went back for release");
    }

    #[test]
    fn leaving_sources_release_their_images() {
        let mut slots: Slots<u8> = Slots::default();
        for user in 1..=3 {
            let key = (user, VideoSource::Camera);
            slots.wanted(key, 1);
            slots.finish(key, 1, Some(user as u8));
        }
        let gone = slots.retain(|(user, _)| *user == 2);
        assert_eq!(gone.len(), 2);
        assert_eq!(slots.len(), 1);
        assert_eq!(slots.clear(), vec![2]);
        assert_eq!(slots.len(), 0);
    }

    #[test]
    fn a_picture_becomes_a_bgra_image() {
        let picture = dc_media::Picture::rgba(2, 1, vec![1, 2, 3, 255, 10, 20, 30, 255]).unwrap();
        let image = render_image(&picture).unwrap();
        assert_eq!(image.as_bytes(0).unwrap(), [3, 2, 1, 255, 30, 20, 10, 255]);
        let bad = dc_media::Picture {
            width: 4,
            height: 4,
            layout: dc_media::Layout::Rgba,
            data: vec![0; 3],
        };
        assert!(render_image(&bad).is_none());
    }

    #[test]
    fn streams_sort_before_cameras_before_the_rest() {
        let mut live = user(3, "Live", false);
        live.going_live = true;
        let tiles = select(
            vec![
                user(1, "zed", false),
                live,
                user(2, "amy", true),
                stream(3, "Live", true),
            ],
            true,
        );
        let order: Vec<(u64, TileKind)> = tiles.iter().map(|t| (t.user, t.kind)).collect();
        assert_eq!(
            order,
            vec![
                (3, TileKind::Stream),
                (2, TileKind::User),
                (3, TileKind::User),
                (1, TileKind::User)
            ]
        );
    }

    #[test]
    fn hiding_non_video_keeps_cameras_and_connected_streams_only() {
        let all = vec![
            user(1, "a", false),
            user(2, "b", true),
            stream(3, "c", true),
            stream(4, "d", false),
        ];
        let kept = select(all.clone(), false);
        let ids: Vec<u64> = kept.iter().map(|t| t.user).collect();
        assert_eq!(ids, vec![3, 2], "a stream nobody watches is not video");
        assert_eq!(select(all, true).len(), 4);
        assert!(select(vec![user(1, "a", false)], false).is_empty());
    }

    #[test]
    fn a_hidden_own_camera_is_neither_video_nor_kept_in_the_filtered_grid() {
        let mut own = user(1, "me", true);
        assert!(shows_video(&own));
        own.local_video_disabled = true;
        assert!(!shows_video(&own), "the tile falls back to the avatar");
        assert!(select(vec![own.clone()], false).is_empty());
        assert_eq!(select(vec![own], true).len(), 1);
    }

    #[test]
    fn the_sort_key_pads_and_lower_cases_the_name() {
        let key = user(7, "Ab", false).sort_key();
        assert_eq!(key, format!("\x05ab{}7", "!".repeat(30)));
        assert!(stream(7, "Ab", true).sort_key().ends_with("7\x03"));
    }

    #[test]
    fn focus_toggles_on_the_same_tile_and_falls_back_when_it_leaves() {
        let mut focus = Focus::default();
        focus.click(1, TileKind::Stream);
        assert_eq!(focus.tile, Some((1, TileKind::Stream)));
        focus.click(2, TileKind::User);
        assert_eq!(focus.tile, Some((2, TileKind::User)));
        focus.click(2, TileKind::User);
        assert_eq!(focus.tile, None, "a second click returns to the grid");
        focus.click(1, TileKind::Stream);
        focus.keep_if(|user, kind| !(user == 1 && kind == TileKind::Stream));
        assert_eq!(focus.tile, None);
        focus.click(1, TileKind::User);
        focus.grid();
        assert_eq!(focus.tile, None);
    }
}
