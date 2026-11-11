//! Text to speech for Speak Message, where the official client uses the browser's `speechSynthesis`: a thread
//! owning the system voice (WinRT on Windows through the `tts` crate), told what to say or to stop.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::time::Duration;

enum Command {
    Speak(String, f32),
    Stop,
}

pub struct Speech {
    commands: Sender<Command>,
    speaking: Arc<AtomicBool>,
}

impl Speech {
    pub fn new() -> Self {
        let (commands, received) = mpsc::channel::<Command>();
        let speaking = Arc::new(AtomicBool::new(false));
        let flag = speaking.clone();
        std::thread::Builder::new()
            .name("speech".into())
            .spawn(move || {
                let mut voice = match tts::Tts::default() {
                    Ok(voice) => voice,
                    Err(error) => {
                        tracing::warn!("text to speech unavailable: {error}");
                        return;
                    }
                };
                loop {
                    match received.recv_timeout(Duration::from_millis(200)) {
                        Ok(Command::Speak(text, rate)) => {
                            // `TTSStore.speechRate`: 1 is the voice's normal pace; the engine's own range bounds it.
                            let rate = if rate > 0. { rate } else { 1. };
                            let _ = voice.set_rate(
                                (voice.normal_rate() * rate)
                                    .clamp(voice.min_rate(), voice.max_rate()),
                            );
                            // `interrupt`: a new message replaces the one being read.
                            if voice.speak(text, true).is_ok() {
                                flag.store(true, Ordering::SeqCst);
                            }
                        }
                        Ok(Command::Stop) => {
                            let _ = voice.stop();
                            flag.store(false, Ordering::SeqCst);
                        }
                        Err(RecvTimeoutError::Timeout) => {
                            if flag.load(Ordering::SeqCst) && !voice.is_speaking().unwrap_or(false)
                            {
                                flag.store(false, Ordering::SeqCst);
                            }
                        }
                        Err(RecvTimeoutError::Disconnected) => return,
                    }
                }
            })
            .ok();
        Self { commands, speaking }
    }

    pub fn speak(&self, text: String, rate: f32) {
        self.speaking.store(true, Ordering::SeqCst);
        let _ = self.commands.send(Command::Speak(text, rate));
    }

    pub fn stop(&self) {
        let _ = self.commands.send(Command::Stop);
    }

    pub fn is_speaking(&self) -> bool {
        self.speaking.load(Ordering::SeqCst)
    }
}

/// `v_` in module 596339: spoilers read as "Spoiler", custom emoji as "Emoji name", then
/// `{username} said {body}`, or `{username} replied to {replyUsername} {body}` for a reply.
pub fn utterance(
    username: &str,
    content: &str,
    reply_to: Option<&str>,
    resolve_user: impl Fn(&str) -> Option<String>,
) -> String {
    let mut body = String::new();
    let mut rest = content;
    // `||spoiler||` becomes the word.
    while let Some(start) = rest.find("||") {
        let Some(end) = rest[start + 2..].find("||") else {
            break;
        };
        body.push_str(&rest[..start]);
        body.push_str("Spoiler");
        rest = &rest[start + 2 + end + 2..];
    }
    body.push_str(rest);
    // `<@id>` / `<@!id>` become names, `<a:name:id>` / `<:name:id>` "Emoji name".
    let mut out = String::new();
    let mut text = body.as_str();
    while let Some(start) = text.find('<') {
        out.push_str(&text[..start]);
        let tail = &text[start..];
        let Some(end) = tail.find('>') else {
            out.push_str(tail);
            text = "";
            break;
        };
        let inner = &tail[1..end];
        if let Some(id) = inner
            .strip_prefix("@!")
            .or_else(|| inner.strip_prefix('@'))
            .filter(|id| id.chars().all(|c| c.is_ascii_digit()))
        {
            out.push_str(&resolve_user(id).unwrap_or_else(|| "unknown-user".to_owned()));
        } else if let Some(emoji) = inner.strip_prefix("a:").or_else(|| inner.strip_prefix(':')) {
            let name = emoji.split(':').next().unwrap_or_default();
            out.push_str(&format!("Emoji {name}"));
        } else {
            out.push_str(&tail[..=end]);
        }
        text = &tail[end + 1..];
    }
    out.push_str(text);
    match reply_to {
        Some(reply) => format!("{username} replied to {reply} {out}"),
        None => format!("{username} said {out}"),
    }
}

#[cfg(test)]
mod tests {
    use super::utterance;

    #[test]
    fn reads_like_the_official_client() {
        let resolve = |id: &str| (id == "5").then(|| "lorean".to_owned());
        assert_eq!(
            utterance("onugeccen", "hi ||secret|| <@5> <:pog:12>", None, resolve),
            "onugeccen said hi Spoiler lorean Emoji pog"
        );
        assert_eq!(
            utterance("a", "yes", Some("b"), |_| None),
            "a replied to b yes"
        );
    }
}
