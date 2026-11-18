//! The Video and Go Live widgets (module 415173 `iS` and `e2`) as plain rules.
//!
//! - Video: the cameras of the voice channel (`getVideoParticipants`) in tiles of at most 192 px, at least 120, 4 px
//!   apart, at most 8 (`i` of module 163432 with `cropSelfVideo`); our own camera always takes the last place. The
//!   tiles fill the widget's height when it is horizontal (its default), its width when vertical. Drawn while there is
//!   a camera and the overlay is unlocked or the widget pinned; unlocked with none, the empty state.
//! - Go Live: the streams of the voice channel other than our own (`getStreamParticipants`), ended, failed or paused
//!   ones left out; locked, only the ones we watch; unlocked, every one unless "View all streams" is off
//!   (`meta.showAllStreams`). Watched ones first where the widget is anchored to the top (or left, horizontal), last
//!   where it hangs from the bottom (or right), then by username. Each tile spans the widget's width (vertical, its
//!   default) or height (horizontal) at 16:9, `padding` apart.

/// `i` of module 163432 as the Video widget calls it.
pub const TILE_WIDTH: f32 = 192.;
pub const TILE_MIN_WIDTH: f32 = 120.;
pub const TILE_MARGIN: f32 = 4.;
pub const TILE_LIMIT: usize = 8;
/// `ez` of module 415173.
pub const GO_LIVE_RATIO: f32 = 9. / 16.;

/// The tiles the Video widget shows along `available` px: the ids in order and each tile's length.
///
/// `others` are the other cameras in the store's order, `me` our own when it is on.
pub fn video_tiles(available: f32, others: &[u64], me: Option<u64>) -> (Vec<u64>, f32) {
    let total = others.len() + usize::from(me.is_some());
    let fit = ((available - TILE_MARGIN) / (TILE_MIN_WIDTH + TILE_MARGIN)).floor();
    let count = (fit.max(0.) as usize).min(TILE_LIMIT).min(total);
    let width = if count == 0 {
        0.
    } else {
        ((available - TILE_MARGIN) / count as f32 - TILE_MARGIN).min(TILE_WIDTH)
    };
    let mut shown: Vec<u64> = others.iter().copied().take(count).collect();
    if let Some(me) = me {
        if !shown.is_empty() && shown.len() >= count {
            let last = shown.len() - 1;
            shown[last] = me;
        } else {
            shown.push(me);
        }
    }
    (shown, width)
}

/// The layout effect of `iS` when Horizontal Layout flips: a widget whose stored size no longer suits its orientation
/// (taller than wide while horizontal, wider than tall while vertical) is resized by `iE` with its content's sides
/// swapped: horizontal at least 192 wide and at most 260 high, vertical the other way round, each at most 75% of the
/// window. `None`: the size stays.
pub fn video_reoriented(
    horizontal: bool,
    size: (f32, f32),
    content: (f32, f32),
    window: (f32, f32),
) -> Option<(f32, f32)> {
    if !(horizontal && size.1 > size.0 || !horizontal && size.0 > size.1) {
        return None;
    }
    let offsets = 2. * crate::drag::BORDER + 2. * crate::drag::PADDING;
    let (width, height) = (content.1, content.0);
    let (width, height) = if horizontal {
        (width.max(192.), height.min(240. + offsets))
    } else {
        (width.min(240. + offsets), height.max(192.))
    };
    Some((
        width.clamp(0., 0.75 * window.0),
        height.clamp(0., 0.75 * window.1),
    ))
}

/// One stream of the channel as the Go Live widget sees it.
#[derive(Clone, Debug, PartialEq)]
pub struct Stream {
    pub owner: u64,
    pub username: String,
    /// We watch it (`getAllActiveStreamsForChannel`).
    pub watched: bool,
    /// Its state is ended, failed or paused (`ew`).
    pub gone: bool,
}

/// `e2`'s participants: who is listed, in order.
pub fn go_live_streams(
    streams: &[Stream],
    me: u64,
    locked: bool,
    show_all: bool,
    first_watched: bool,
    last_watched: bool,
) -> Vec<Stream> {
    let all = !locked && show_all;
    let mut listed: Vec<Stream> = streams
        .iter()
        .filter(|stream| stream.owner != me && !stream.gone && (all || stream.watched))
        .cloned()
        .collect();
    listed.sort_by(|a, b| {
        let rank = |stream: &Stream| {
            if first_watched {
                u8::from(!stream.watched)
            } else if last_watched {
                u8::from(stream.watched)
            } else {
                0
            }
        };
        rank(a)
            .cmp(&rank(b))
            .then_with(|| a.username.cmp(&b.username))
    });
    listed
}

/// `$` of `e2`: nothing to draw (no channel, or locked with nothing watched, or unlocked with nobody streaming).
pub fn go_live_empty(in_channel: bool, watched: usize, listed: usize, locked: bool) -> bool {
    !in_channel || (watched == 0 && locked) || (listed == 0 && !locked)
}

/// A Go Live tile's size in a widget whose content is `content` px: the full width (vertical) or height
/// (horizontal) at 16:9 (`eZ`).
pub fn go_live_tile(content: (f32, f32), horizontal: bool) -> (f32, f32) {
    if horizontal {
        (content.1 / GO_LIVE_RATIO, content.1)
    } else {
        (content.0, content.0 * GO_LIVE_RATIO)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_video_tiles_fit_the_widget_and_keep_our_camera_last() {
        // The default widget: 408 wide less 2 * (8 padding + 2 border) = 388.
        let (shown, width) = video_tiles(388., &[2, 3], Some(1));
        assert_eq!(shown, [2, 3, 1]);
        assert_eq!(width, (388. - 4.) / 3. - 4.);
        // Room for two: our camera takes the last place.
        let (shown, width) = video_tiles(260., &[2, 3, 4], Some(1));
        assert_eq!(shown, [2, 1]);
        assert_eq!(width, 124.);
        // Wide: at most 192 each, at most eight.
        let (shown, width) = video_tiles(4000., &[2, 3, 4, 5, 6, 7, 8, 9, 10], None);
        assert_eq!(shown.len(), 8);
        assert_eq!(width, 192.);
        assert_eq!(video_tiles(388., &[], None), (Vec::new(), 0.));
        assert_eq!(video_tiles(388., &[], Some(1)).0, [1]);
    }

    #[test]
    fn turning_the_video_widget_swaps_its_sides() {
        // The default 408 by 128 widget turned vertical: 108 wide, 388 high.
        assert_eq!(
            video_reoriented(false, (408., 128.), (388., 108.), (1280., 720.)),
            Some((108., 388.))
        );
        // Back to horizontal: at least 192 wide, at most 260 high.
        assert_eq!(
            video_reoriented(true, (108., 388.), (88., 368.), (1280., 720.)),
            Some((368., 88.))
        );
        // A size that already suits the orientation stays; a big one is kept inside 75% of the window.
        assert_eq!(
            video_reoriented(true, (408., 128.), (388., 108.), (1280., 720.)),
            None
        );
        assert_eq!(
            video_reoriented(false, (1200., 300.), (1180., 280.), (1280., 720.)),
            Some((260., 540.))
        );
    }

    #[test]
    fn the_go_live_list_follows_the_lock_and_the_anchor() {
        let stream = |owner, name: &str, watched| Stream {
            owner,
            username: name.to_owned(),
            watched,
            gone: false,
        };
        let streams = [
            stream(1, "me", false),
            stream(2, "carol", false),
            stream(3, "bob", true),
            Stream {
                gone: true,
                ..stream(4, "dave", true)
            },
        ];
        let owners = |list: Vec<Stream>| list.iter().map(|s| s.owner).collect::<Vec<_>>();
        // Locked: only the watched ones.
        assert_eq!(
            owners(go_live_streams(&streams, 1, true, true, true, false)),
            [3]
        );
        // Unlocked: everyone else, the watched first under a top anchor, last under a bottom one.
        assert_eq!(
            owners(go_live_streams(&streams, 1, false, true, true, false)),
            [3, 2]
        );
        assert_eq!(
            owners(go_live_streams(&streams, 1, false, true, false, true)),
            [2, 3]
        );
        assert_eq!(
            owners(go_live_streams(&streams, 1, false, true, false, false)),
            [3, 2]
        );
        // "Only show watched streams".
        assert_eq!(
            owners(go_live_streams(&streams, 1, false, false, true, false)),
            [3]
        );
        assert!(go_live_empty(false, 1, 1, false));
        assert!(go_live_empty(true, 0, 2, true));
        assert!(!go_live_empty(true, 1, 1, true));
        assert!(go_live_empty(true, 0, 0, false));
        assert_eq!(go_live_tile((240., 300.), false), (240., 135.));
        assert_eq!(go_live_tile((500., 90.), true), (160., 90.));
    }
}
