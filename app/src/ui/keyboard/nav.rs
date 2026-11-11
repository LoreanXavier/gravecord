//! Server, channel and unread stepping as pure functions over plain data, so the official algorithms can be tested
//! without a window. Sources: module 194482 (`E`, server next and previous), 980372 (`p`, channel next and previous)
//! and 513291 (`m`, unread and mention navigation).

use dc_state::Id;

/// `ME`, the id the official client uses for Home and direct messages.
pub(super) const HOME: Id = 0;

/// Where the window is, for stepping between servers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Place {
    Home,
    Guild(Id),
}

/// Where server stepping lands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ServerTarget {
    Home,
    Guild(Id),
}

/// `E(e)` in 194482. The index starts at -2 for Home or a server that is not in the list, so one step forward from
/// Home is the first server and one step back from Home is the last. The favorites pseudo-server (index -1) is
/// skipped because the client has none.
pub(super) fn server_step(guilds: &[Id], here: Place, delta: i32) -> ServerTarget {
    let len = guilds.len() as i32;
    let start = match here {
        Place::Home => -2,
        Place::Guild(guild) => guilds
            .iter()
            .position(|known| *known == guild)
            .map_or(-2, |index| index as i32),
    };
    let mut at = start + delta;
    if at == -1 {
        at += delta;
    }
    if at == -3 {
        at = len - 1;
    }
    if at >= len || at == -2 {
        return ServerTarget::Home;
    }
    ServerTarget::Guild(guilds[at as usize])
}

/// The channel one step from `current` in `list`, wrapping at both ends (`d[n<0?a-1:n>=a?0:n]` in 980372).
pub(super) fn channel_step(list: &[Id], current: Option<Id>, delta: i32) -> Option<Id> {
    if list.is_empty() {
        return None;
    }
    let len = list.len() as i32;
    let start = current
        .and_then(|channel| list.iter().position(|known| *known == channel))
        .map_or(-1, |index| index as i32);
    let at = start + delta;
    let at = if at < 0 {
        len - 1
    } else if at >= len {
        0
    } else {
        at
    };
    Some(list[at as usize])
}

/// The order the walk visits servers in (`O` in 513291): the current one first, then the rest in rail order with
/// the current one again at the end; for a step back the same list is walked from its tail.
pub(super) fn guild_walk(order: &[Id], current: Id, delta: i32) -> Vec<Id> {
    let mut list: Vec<Id> = order.to_vec();
    if !list.contains(&current) {
        list.insert(0, current);
    }
    let at = list
        .iter()
        .position(|known| *known == current)
        .unwrap_or_default();
    if delta > 0 {
        let mut walk: Vec<Id> = list[at..].to_vec();
        walk.extend_from_slice(&list[..at]);
        walk.push(current);
        walk
    } else {
        list.insert(at, current);
        let mut walk: Vec<Id> = list[at + 1..].to_vec();
        walk.extend_from_slice(&list[..=at]);
        walk
    }
}

/// `m(e, ...)` in 513291: from the open channel, the next channel (or previous for a negative `delta`) whose
/// `channel_hit` is true, looking through servers whose `guild_hit` is true. Returns the server and the channel.
pub(super) fn unread_step(
    order: &[Id],
    channels_of: &dyn Fn(Id) -> Vec<Id>,
    current: (Id, Option<Id>),
    delta: i32,
    guild_hit: &dyn Fn(Id) -> bool,
    channel_hit: &dyn Fn(Id, Id) -> bool,
) -> Option<(Id, Id)> {
    let (here, channel) = current;
    let walk = guild_walk(order, here, delta);
    let mut at = if delta > 0 { 0 } else { walk.len() as i32 - 1 };
    let mut list = channels_of(here);
    let mut index = channel
        .and_then(|channel| list.iter().position(|known| *known == channel))
        .map_or(-1, |index| index as i32)
        + delta;
    loop {
        let guild = *walk.get(usize::try_from(at).ok()?)?;
        if guild_hit(guild) {
            while let Some(candidate) = usize::try_from(index).ok().and_then(|i| list.get(i)) {
                if channel_hit(guild, *candidate) {
                    return Some((guild, *candidate));
                }
                index += delta;
            }
        }
        at += delta;
        let next = *walk.get(usize::try_from(at).ok()?)?;
        list = channels_of(next);
        index = if delta < 0 { list.len() as i32 - 1 } else { 0 };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GUILDS: [Id; 3] = [10, 20, 30];

    #[test]
    fn servers_step_through_home_at_both_ends() {
        assert_eq!(
            server_step(&GUILDS, Place::Home, 1),
            ServerTarget::Guild(10)
        );
        assert_eq!(
            server_step(&GUILDS, Place::Guild(10), 1),
            ServerTarget::Guild(20)
        );
        assert_eq!(
            server_step(&GUILDS, Place::Guild(30), 1),
            ServerTarget::Home
        );
        assert_eq!(
            server_step(&GUILDS, Place::Home, -1),
            ServerTarget::Guild(30)
        );
        assert_eq!(
            server_step(&GUILDS, Place::Guild(20), -1),
            ServerTarget::Guild(10)
        );
        assert_eq!(
            server_step(&GUILDS, Place::Guild(10), -1),
            ServerTarget::Home
        );
    }

    #[test]
    fn an_unknown_server_steps_like_home() {
        assert_eq!(
            server_step(&GUILDS, Place::Guild(99), 1),
            ServerTarget::Guild(10)
        );
        assert_eq!(server_step(&[], Place::Home, 1), ServerTarget::Home);
    }

    #[test]
    fn channels_wrap() {
        let list = [1, 2, 3];
        assert_eq!(channel_step(&list, Some(1), 1), Some(2));
        assert_eq!(channel_step(&list, Some(3), 1), Some(1));
        assert_eq!(channel_step(&list, Some(1), -1), Some(3));
        assert_eq!(channel_step(&list, None, 1), Some(1));
        assert_eq!(channel_step(&list, Some(9), -1), Some(3));
        assert_eq!(channel_step(&[], Some(1), 1), None);
    }

    #[test]
    fn the_unread_walk_starts_and_ends_on_the_current_server() {
        assert_eq!(guild_walk(&[0, 10, 20], 10, 1), vec![10, 20, 0, 10]);
        assert_eq!(guild_walk(&[0, 10, 20], 10, -1), vec![10, 20, 0, 10]);
        assert_eq!(guild_walk(&[0, 10, 20], 99, 1), vec![99, 0, 10, 20, 99]);
    }

    fn channels(guild: Id) -> Vec<Id> {
        match guild {
            0 => vec![1, 2],
            10 => vec![11, 12, 13],
            20 => vec![21, 22],
            _ => Vec::new(),
        }
    }

    #[test]
    fn the_next_unread_channel_is_found_in_the_same_server_first() {
        let hit = |_: Id, channel: Id| channel == 13 || channel == 22;
        let found = unread_step(&[0, 10, 20], &channels, (10, Some(11)), 1, &|_| true, &hit);
        assert_eq!(found, Some((10, 13)));
    }

    #[test]
    fn the_walk_moves_on_to_the_next_server() {
        let hit = |_: Id, channel: Id| channel == 22;
        let found = unread_step(&[0, 10, 20], &channels, (10, Some(12)), 1, &|_| true, &hit);
        assert_eq!(found, Some((20, 22)));
    }

    #[test]
    fn a_muted_server_is_skipped_unless_it_is_the_current_one() {
        let hit = |_: Id, channel: Id| channel == 22 || channel == 2;
        let found = unread_step(
            &[0, 10, 20],
            &channels,
            (10, Some(12)),
            1,
            &|guild| guild != 20,
            &hit,
        );
        assert_eq!(found, Some((0, 2)));
    }

    #[test]
    fn the_walk_back_goes_from_the_tail() {
        let hit = |_: Id, channel: Id| channel == 11 || channel == 21;
        let found = unread_step(&[0, 10, 20], &channels, (10, Some(12)), -1, &|_| true, &hit);
        assert_eq!(found, Some((10, 11)));
        let found = unread_step(&[0, 10, 20], &channels, (10, Some(11)), -1, &|_| true, &hit);
        assert_eq!(found, Some((20, 21)));
    }

    #[test]
    fn nothing_unread_finds_nothing() {
        let found = unread_step(
            &[0, 10, 20],
            &channels,
            (10, Some(11)),
            1,
            &|_| true,
            &|_, _| false,
        );
        assert_eq!(found, None);
    }
}
