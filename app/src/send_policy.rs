//! How a sent message is retried, when it counts as failed, and what the failure says.
//!
//! * Module 381941 `ZG`: the message queue's request options are a 60 second timeout, 3 retries and a `Backoff`.
//! * Module 620632 (`I`): a response with one of the gateway-error statuses (`E`), or a transport error that is not an
//!   abort (a timeout is one), is sent again while `retries` lasts, after `backoff.fail()`; every other answer ends the
//!   request. Module 158390 is the `Backoff` (min 500 ms, max ten times that, jittered).
//! * Module 870801 (`createResponseHandler`): a 429 with `Retry-After` puts the message back in the queue for that long,
//!   so it stays in the sending state; the slowmode rate limit (code 20016) is an ordinary failure.
//! * Module 138399 (`_sendMessage`): what a failed send does besides `MESSAGE_SEND_FAILED`, by status and error code,
//!   and the table of notices Clyde shows (`eK`, `sendClydeError`).

use serde_json::Value;
use std::time::Duration;

/// `retries: 3` of module 381941: four attempts in all.
pub const RETRIES: u32 = 3;
/// `timeout` of module 381941.
pub const TIMEOUT: Duration = Duration::from_secs(60);
/// `E` in module 620632.
const RETRIED_STATUSES: [u16; 8] = [502, 504, 507, 598, 599, 522, 523, 524];

/// Whether the HTTP client sends the request again for this status.
pub fn status_is_retried(status: u16) -> bool {
    RETRIED_STATUSES.contains(&status)
}

/// `Backoff` of module 158390 with `new Backoff()`: min 500 ms, max 5000 ms, jitter on.
#[derive(Debug, Clone)]
pub struct Backoff {
    max: f64,
    current: f64,
    fails: u32,
}

impl Default for Backoff {
    fn default() -> Self {
        let min = 500.;
        Self {
            max: min * 10.,
            current: min,
            fails: 0,
        }
    }
}

impl Backoff {
    /// `fail()`: the next wait. `random` is the jitter factor in `0..1`; a server `retry_after` raises the wait.
    pub fn fail(&mut self, random: f64, retry_after: Option<Duration>) -> Duration {
        self.fails += 1;
        let step = 2. * self.current * random;
        self.current = (self.current + step).min(self.max);
        let wait = Duration::from_millis(self.current as u64);
        retry_after.filter(|after| *after > wait).unwrap_or(wait)
    }

    pub fn fails(&self) -> u32 {
        self.fails
    }
}

/// What the server or the transport said about a send that did not go through.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Failure {
    /// The HTTP status; `None` when no response came (a transport error, a timeout).
    pub status: Option<u16>,
    pub code: Option<u64>,
    pub message: Option<String>,
    /// `retry_after` of the body, in seconds.
    pub retry_after: Option<f64>,
}

impl Failure {
    /// From the body of an error response.
    pub fn from_response(status: u16, body: &Value) -> Self {
        Self {
            status: Some(status),
            code: body["code"].as_u64(),
            message: body["message"].as_str().map(str::to_owned),
            retry_after: body["retry_after"].as_f64(),
        }
    }

    /// A refusal that never reached the network (an inert run): the message fails at once, no retry.
    pub fn refused() -> Self {
        Self::default()
    }
}

pub const SLOWMODE_RATE_LIMITED: u64 = 20016;
pub const EXPLICIT_CONTENT: u64 = 20009;
pub const POGGERMODE_TEMPORARILY_DISABLED: u64 = 170008;
pub const AUTOMOD_MESSAGE_BLOCKED: u64 = 200000;
pub const AUTOMOD_TITLE_BLOCKED: u64 = 200001;
pub const AUTOMOD_INVALID_RUST_SERVICE_RESPONSE: u64 = 200002;

/// What a failed send does besides turning the message red (`_sendMessage` of module 138399).
#[derive(Clone, Debug, PartialEq)]
pub enum Followup {
    /// `SLOWMODE_SET_COOLDOWN` with the body's `retry_after`.
    Slowmode {
        seconds: f64,
    },
    /// `MESSAGE_SEND_FAILED_AUTOMOD`: the message is flagged as private and the community's notice shows.
    Automod,
    /// `POGGERMODE_TEMPORARILY_DISABLED`.
    PowermodeDisabled,
    /// The failure reason is `EXPLICIT_CONTENT` and Clyde says so (`sendExplicitMediaClydeError`).
    ExplicitContent,
    /// `sendClydeError(channel, code)`.
    Clyde(u64),
    Nothing,
}

/// `suppress_clyde` is the poll, forward and content-inventory sends (`R`, `el`, `w`), which never get a notice.
pub fn followup(failure: &Failure, suppress_clyde: bool) -> Followup {
    let Some(status) = failure.status else {
        return Followup::Nothing;
    };
    if !(400..500).contains(&status) {
        return Followup::Nothing;
    }
    match failure.code {
        Some(SLOWMODE_RATE_LIMITED) => match failure.retry_after {
            Some(seconds) if seconds > 0. => Followup::Slowmode { seconds },
            _ => Followup::Nothing,
        },
        Some(
            AUTOMOD_MESSAGE_BLOCKED | AUTOMOD_TITLE_BLOCKED | AUTOMOD_INVALID_RUST_SERVICE_RESPONSE,
        ) => Followup::Automod,
        Some(POGGERMODE_TEMPORARILY_DISABLED) => Followup::PowermodeDisabled,
        Some(EXPLICIT_CONTENT) => Followup::ExplicitContent,
        _ if suppress_clyde => Followup::Nothing,
        code => Followup::Clyde(code.unwrap_or(0)),
    }
}

/// `DM_COULD_NOT_BE_DELIVERED` (module 652215) as the help center article address.
const DELIVERY_HELP: &str = "https://support.discord.com/hc/en-us/articles/360060145013";
/// `ew.YK` of module 292348: the most a message's files may weigh together, in MB.
const MAX_TOTAL_ATTACHMENT_MB: u64 = 500;
const HARMFUL_LINKS_HELP: &str = "https://dis.gd/phish";

/// What the notice is about, as `messageGetter` of module 138399 reads the channel.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ChannelFacts {
    /// `rateLimitPerUser`.
    pub slowmode_seconds: u64,
    pub is_dm: bool,
    pub is_forum_like: bool,
    /// A direct message with exactly one recipient, and that recipient is a bot.
    pub dm_with_bot: bool,
}

/// A notice's logging name and text.
pub struct Notice {
    pub name: String,
    pub text: String,
}

fn seconds_text(seconds: u64) -> String {
    // `IWntYg`: a cardinal plural of "second".
    format!(
        "Slowmode is enabled. Members can send one message every {seconds} second{}.",
        if seconds == 1 { "" } else { "s" }
    )
}

fn undelivered() -> String {
    // `SkGL7l`.
    format!(
        "Your message could not be delivered.\n\nYou can see the full list of reasons here: {DELIVERY_HELP}"
    )
}

/// `eK[code]`, else the default (`SkGL7l`, named `SEND_FAILED (code)`).
pub fn clyde_notice(code: u64, channel: ChannelFacts) -> Notice {
    let (name, text) = match code {
        SLOWMODE_RATE_LIMITED => (
            "SLOWMODE_RATE_LIMITED",
            seconds_text(channel.slowmode_seconds),
        ),
        // 50278: a bot DM keeps the plain text; anything else asks for a friend request first (`llTkqr`).
        50278 => (
            "INVALID_MESSAGE_SEND_NO_MUTUAL_GUILDS",
            if channel.is_dm && channel.dm_with_bot {
                undelivered()
            } else {
                format!(
                    "Your message could not be delivered. Send a friend request first and start the conversation once they've accepted.\n\nHere is the full list of reasons why your message could not be delivered: {DELIVERY_HELP}"
                )
            },
        ),
        50007 => ("INVALID_MESSAGE_SEND_USER", undelivered()),
        160006 => (
            "TOO_MANY_THREADS",
            if channel.is_forum_like {
                "Your message could not be sent because this server has reached the maximum number of posts. Free up space by archiving a post.".to_owned()
            } else {
                "Your message could not be sent because this server has reached the maximum number of threads. Free up space by archiving a thread.".to_owned()
            },
        ),
        160007 => (
            "TOO_MANY_ANNOUNCEMENT_THREADS",
            "Your message could not be sent because this server has reached the maximum number of announcement threads. Free up space by archiving a thread.".to_owned(),
        ),
        240000 => (
            "HARMFUL_LINK_MESSAGE_BLOCKED",
            format!(
                "Your message could not be delivered because this message contains a link blocked by Discord. You can learn more here: {HARMFUL_LINKS_HELP}."
            ),
        ),
        240001 => (
            "HARMFUL_URL_BLOCKED",
            "Your message could not be delivered because this message contains a link blocked by Discord.".to_owned(),
        ),
        40002 => (
            "BOT_REQUIRES_EMAIL_VERIFICATION",
            "You need to verify your email or phone before you can send messages here.".to_owned(),
        ),
        400005 => (
            "GUILD_MESSAGE_UPDATE_RATE_LIMIT_EXCEEDED",
            "Server is too busy, please try again later.".to_owned(),
        ),
        40003 => (
            "BOT_DM_RATE_LIMITED",
            "You are sending too many new direct messages. Hold off for a few minutes before trying to message this person again.".to_owned(),
        ),
        40004 => (
            "BOT_DM_SEND_MESSAGE_TEMPORARILY_DISABLED",
            "Sending messages to this channel has been temporarily disabled. Try again in a bit.".to_owned(),
        ),
        50251 => (
            "BOT_DM_SEND_MESSAGE_INVALID_FOR_GAME_FRIEND",
            "This user does not allow messages from Game Friends in Discord, try sending them a message in-game instead!".to_owned(),
        ),
        50252 => (
            "BOT_DM_SEND_MESSAGE_INVALID_OFFLINE_PROVISIONAL_ACCOUNT",
            "This user cannot receive messages while offline.".to_owned(),
        ),
        30053 => (
            "TOTAL_ATTACHMENT_SIZE_TOO_LARGE",
            format!(
                "Your attachment(s) are too large. Maximum total size is {MAX_TOTAL_ATTACHMENT_MB} MB."
            ),
        ),
        50146 => (
            "CLOUD_UPLOAD_NOT_FOUND",
            "We couldn't find your file upload. Try removing the attachment and adding it again.".to_owned(),
        ),
        50013 => (
            "INVALID_PERMISSIONS",
            "You don't have permission to do that here.".to_owned(),
        ),
        other => {
            return Notice {
                name: format!("SEND_FAILED ({other})"),
                text: undelivered(),
            };
        }
    };
    Notice {
        name: name.to_owned(),
        text,
    }
}

/// `sendExplicitMediaClydeError`: the text by kind of channel.
pub fn explicit_media_notice(is_dm: bool, is_group_dm: bool) -> Notice {
    let (name, text) = if is_dm {
        (
            "BOT_DM_EXPLICIT_CONTENT",
            "Your message could not be sent. Discord detected that it may contain sensitive content, which is not allowed in this conversation.",
        )
    } else if is_group_dm {
        (
            "BOT_GDM_EXPLICIT_CONTENT",
            "Your message could not be sent. Discord detected that it may contain sensitive content, which is not allowed in this conversation.",
        )
    } else {
        (
            "BOT_GUILD_EXPLICIT_CONTENT",
            "Your message could not be sent. Discord detected that it may contain sensitive content, which is not allowed in this server.",
        )
    };
    Notice {
        name: name.to_owned(),
        text: text.to_owned(),
    }
}

/// The outcome of a send, once its retries are used up.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    Sent(Value),
    Failed(Failure),
    /// The upload stopped with nothing to tell (the cancel button, or an error without a code): the row goes and
    /// no failed message stays (design/golden/windows/upload/after-failure).
    Dropped,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn only_gateway_errors_are_retried() {
        for status in [502, 504, 507, 598, 599, 522, 523, 524] {
            assert!(status_is_retried(status), "{status}");
        }
        for status in [400, 401, 403, 404, 429, 500, 501, 503] {
            assert!(!status_is_retried(status), "{status}");
        }
        assert_eq!(RETRIES, 3);
        assert_eq!(TIMEOUT, Duration::from_secs(60));
    }

    #[test]
    fn the_backoff_starts_between_half_a_second_and_a_second_and_a_half_and_stops_at_five() {
        let mut backoff = Backoff::default();
        // No jitter at all: the wait stays at the minimum.
        assert_eq!(backoff.fail(0., None), Duration::from_millis(500));
        // Full jitter: current + 2 * current.
        let mut backoff = Backoff::default();
        assert_eq!(backoff.fail(1., None), Duration::from_millis(1500));
        assert_eq!(backoff.fail(1., None), Duration::from_millis(4500));
        assert_eq!(backoff.fail(1., None), Duration::from_millis(5000));
        assert_eq!(backoff.fails(), 3);
        // A server's retry-after longer than the wait wins.
        let mut backoff = Backoff::default();
        assert_eq!(
            backoff.fail(0., Some(Duration::from_secs(9))),
            Duration::from_secs(9)
        );
    }

    #[test]
    fn a_failure_reads_the_status_and_the_body() {
        let failure = Failure::from_response(
            429,
            &json!({"code": 20016, "message": "slow", "retry_after": 4.5}),
        );
        assert_eq!(failure.status, Some(429));
        assert_eq!(failure.code, Some(20016));
        assert_eq!(failure.retry_after, Some(4.5));
        assert_eq!(failure.message.as_deref(), Some("slow"));
    }

    fn client_error(code: u64) -> Failure {
        Failure {
            status: Some(403),
            code: Some(code),
            message: None,
            retry_after: None,
        }
    }

    #[test]
    fn the_followup_follows_the_branches_of_the_send() {
        assert_eq!(followup(&Failure::refused(), false), Followup::Nothing);
        let server = Failure {
            status: Some(500),
            code: Some(50013),
            ..Failure::default()
        };
        assert_eq!(followup(&server, false), Followup::Nothing);
        let slow = Failure {
            status: Some(429),
            code: Some(20016),
            retry_after: Some(5.),
            ..Failure::default()
        };
        assert_eq!(followup(&slow, false), Followup::Slowmode { seconds: 5. });
        let no_wait = Failure {
            retry_after: None,
            ..slow
        };
        assert_eq!(followup(&no_wait, false), Followup::Nothing);
        assert_eq!(followup(&client_error(200000), false), Followup::Automod);
        assert_eq!(followup(&client_error(200001), false), Followup::Automod);
        assert_eq!(
            followup(&client_error(170008), false),
            Followup::PowermodeDisabled
        );
        assert_eq!(
            followup(&client_error(20009), false),
            Followup::ExplicitContent
        );
        assert_eq!(
            followup(&client_error(50013), false),
            Followup::Clyde(50013)
        );
        assert_eq!(followup(&client_error(50013), true), Followup::Nothing);
        // No code in the body: `sendClydeError(channel, undefined)` takes its default 0.
        let bare = Failure {
            status: Some(400),
            ..Failure::default()
        };
        assert_eq!(followup(&bare, false), Followup::Clyde(0));
    }

    #[test]
    fn the_notice_text_follows_the_code() {
        let channel = ChannelFacts {
            slowmode_seconds: 1,
            ..ChannelFacts::default()
        };
        let slow = clyde_notice(20016, channel);
        assert_eq!(slow.name, "SLOWMODE_RATE_LIMITED");
        assert_eq!(
            slow.text,
            "Slowmode is enabled. Members can send one message every 1 second."
        );
        let many = ChannelFacts {
            slowmode_seconds: 30,
            ..channel
        };
        assert!(
            clyde_notice(20016, many)
                .text
                .ends_with("every 30 seconds.")
        );
        assert_eq!(
            clyde_notice(50013, channel).text,
            "You don't have permission to do that here."
        );
        assert_eq!(
            clyde_notice(30053, channel).text,
            "Your attachment(s) are too large. Maximum total size is 500 MB."
        );
        assert_eq!(
            clyde_notice(40002, channel).text,
            "You need to verify your email or phone before you can send messages here."
        );
        assert_eq!(
            clyde_notice(50146, channel).text,
            "We couldn't find your file upload. Try removing the attachment and adding it again."
        );
        assert_eq!(
            clyde_notice(240001, channel).text,
            "Your message could not be delivered because this message contains a link blocked by Discord."
        );
        assert!(
            clyde_notice(240000, channel)
                .text
                .ends_with("You can learn more here: https://dis.gd/phish.")
        );
    }

    #[test]
    fn a_direct_message_to_a_stranger_asks_for_a_friend_request_first() {
        let stranger = ChannelFacts {
            is_dm: true,
            ..ChannelFacts::default()
        };
        assert!(
            clyde_notice(50278, stranger)
                .text
                .starts_with("Your message could not be delivered. Send a friend request first")
        );
        let bot = ChannelFacts {
            is_dm: true,
            dm_with_bot: true,
            ..ChannelFacts::default()
        };
        assert_eq!(clyde_notice(50278, bot).text, clyde_notice(50007, bot).text);
    }

    #[test]
    fn a_code_the_table_lacks_gets_the_default_text_and_its_own_name() {
        let notice = clyde_notice(12345, ChannelFacts::default());
        assert_eq!(notice.name, "SEND_FAILED (12345)");
        assert_eq!(
            notice.text,
            "Your message could not be delivered.\n\nYou can see the full list of reasons here: https://support.discord.com/hc/en-us/articles/360060145013"
        );
        assert_eq!(
            clyde_notice(0, ChannelFacts::default()).name,
            "SEND_FAILED (0)"
        );
    }

    #[test]
    fn threads_and_posts_have_their_own_limit_text() {
        let forum = ChannelFacts {
            is_forum_like: true,
            ..ChannelFacts::default()
        };
        assert!(
            clyde_notice(160006, forum)
                .text
                .contains("maximum number of posts")
        );
        assert!(
            clyde_notice(160006, ChannelFacts::default())
                .text
                .contains("maximum number of threads")
        );
    }

    #[test]
    fn explicit_media_text_follows_the_kind_of_channel() {
        assert!(
            explicit_media_notice(true, false)
                .text
                .ends_with("this conversation.")
        );
        assert_eq!(
            explicit_media_notice(false, true).name,
            "BOT_GDM_EXPLICIT_CONTENT"
        );
        assert!(
            explicit_media_notice(false, false)
                .text
                .ends_with("this server.")
        );
    }
}
