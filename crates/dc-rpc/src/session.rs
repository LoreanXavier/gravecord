//! One client connection: the frame loop of `RPCServer:IPC` (module 998921), the handshake checks
//! (module 313731 `checkRpcVersion`, module 546983 `j7`, module 90924 `uM`) and the request flow of the generic
//! server (module 223769 `handleRequest`).

use crate::activity::{self, Outcome};
use crate::commands::COMMANDS;
use crate::frame::{Decoder, Opcode, encode};
use crate::{Event, RpcError, Shared, close_codes, codes, validate};
use serde_json::{Value, json};
use std::sync::atomic::Ordering;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::watch;

/// The scope every IPC client has (`RPC_LOCAL_SCOPE`); the only commands it may run are those whose scope lists it.
const LOCAL_SCOPE_COMMANDS: &[&str] = &["SET_ACTIVITY"];

/// A close frame the server sends before it hangs up.
struct Close {
    code: i64,
    message: String,
}

impl Close {
    fn new(code: i64, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

/// What the handshake payload says about the version: JavaScript's `+value`, which `checkRpcVersion` compares
/// (`null`, above 1 or below 1 is refused; `NaN` compares false both ways and passes).
fn version_number(value: Option<&Value>) -> f64 {
    match value {
        None => f64::NAN,
        Some(Value::Number(number)) => number.as_f64().unwrap_or(f64::NAN),
        Some(Value::String(text)) => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                0.0
            } else {
                trimmed.parse().unwrap_or(f64::NAN)
            }
        }
        Some(Value::Bool(flag)) => f64::from(u8::from(*flag)),
        Some(Value::Null) => 0.0,
        Some(_) => f64::NAN,
    }
}

fn show_number(number: f64) -> String {
    if number.fract() == 0.0 && number.is_finite() {
        format!("{}", number as i64)
    } else {
        number.to_string()
    }
}

struct Session<'a> {
    shared: &'a Shared,
    id: u64,
    did_handshake: bool,
    application: Option<Value>,
}

impl Session<'_> {
    fn dispatch(&self, nonce: Value, cmd: &str, evt: Value, data: Value) -> Vec<u8> {
        encode(
            Opcode::Frame,
            &json!({"cmd": cmd, "data": data, "evt": evt, "nonce": nonce}),
        )
    }

    fn error(&self, nonce: Value, cmd: Value, error: RpcError) -> Vec<u8> {
        // A missing `cmd` takes the default of `error()`, `DISPATCH`.
        let cmd = if cmd.is_null() {
            json!("DISPATCH")
        } else {
            cmd
        };
        encode(
            Opcode::Frame,
            &json!({
                "cmd": cmd,
                "data": {"code": error.code, "message": error.message},
                "evt": "ERROR",
                "nonce": nonce,
            }),
        )
    }

    /// One decoded frame.
    async fn frame(&mut self, opcode: Opcode, payload: Value) -> Result<Vec<Vec<u8>>, Close> {
        match opcode {
            Opcode::Ping => Ok(vec![encode(Opcode::Pong, &payload)]),
            Opcode::Pong => Ok(Vec::new()),
            Opcode::Close => Err(Close::new(close_codes::NORMAL, "client disconnect")),
            Opcode::Handshake => self.handshake(&payload).await,
            Opcode::Frame => {
                if !self.did_handshake {
                    return Err(Close::new(close_codes::UNSUPPORTED, "did not handshake"));
                }
                Ok(self.request(payload).await)
            }
        }
    }

    async fn handshake(&mut self, payload: &Value) -> Result<Vec<Vec<u8>>, Close> {
        if self.did_handshake {
            return Err(Close::new(
                close_codes::UNSUPPORTED,
                "already did handshake",
            ));
        }
        let client_id = match payload.get("client_id") {
            Some(Value::String(text)) => text.clone(),
            Some(Value::Number(number)) => number.to_string(),
            _ => String::new(),
        };
        let version = version_number(payload.get("v"));
        // `null == e || e > 1 || e < 1`: raised inside `read`, so the socket closes as unsupported.
        if !(1.0..=1.0).contains(&version) && !version.is_nan() {
            return Err(Close::new(
                close_codes::UNSUPPORTED,
                format!("Invalid Version: {}", show_number(version)),
            ));
        }
        self.did_handshake = true;
        // `j7`: an IPC client has no origin, so a client id is required.
        if client_id.is_empty() {
            return Err(Close::new(
                close_codes::INVALID_CLIENTID,
                "No Client ID Specified",
            ));
        }
        self.shared.throttle.acquire(&client_id).await;
        let Some(application) = self
            .shared
            .cache
            .application(self.shared.dir.as_ref(), &client_id)
            .await
        else {
            return Err(Close::new(
                close_codes::INVALID_CLIENTID,
                "Invalid Client ID",
            ));
        };
        let Some(user) = self.shared.dir.current_user() else {
            return Err(Close::new(close_codes::NORMAL, "User logged out"));
        };
        self.application = Some(application.clone());
        self.shared.emit(Event::Connected {
            socket: self.id,
            application: application.clone(),
        });
        // `handleConnect`: `{v, config, user}` as the `READY` dispatch.
        let ready = self.dispatch(
            Value::Null,
            "DISPATCH",
            json!("READY"),
            json!({
                "v": 1,
                "config": {
                    "cdn_host": "cdn.discordapp.com",
                    "api_endpoint": "//discord.com/api",
                    "environment": "production",
                },
                "user": user,
            }),
        );
        Ok(vec![ready])
    }

    /// `handleRequest`: nonce, command, scope, validation, then the handler.
    async fn request(&mut self, request: Value) -> Vec<Vec<u8>> {
        let nonce = request.get("nonce").cloned().unwrap_or(Value::Null);
        let cmd = request.get("cmd").cloned().unwrap_or(Value::Null);
        let fail =
            |this: &Self, error: RpcError| vec![this.error(nonce.clone(), cmd.clone(), error)];
        if nonce.is_null() || nonce.as_str() == Some("") {
            return fail(
                self,
                RpcError::new(codes::INVALID_PAYLOAD, "Payload requires a nonce"),
            );
        }
        let name = cmd.as_str().unwrap_or("");
        if !COMMANDS.contains(&name) {
            let shown = match &cmd {
                Value::String(text) => text.clone(),
                Value::Null => "undefined".to_owned(),
                other => other.to_string(),
            };
            return fail(
                self,
                RpcError::new(codes::INVALID_COMMAND, format!("Invalid command: {shown}")),
            );
        }
        if !LOCAL_SCOPE_COMMANDS.contains(&name) {
            return fail(
                self,
                RpcError::new(
                    codes::INVALID_PERMISSIONS,
                    "Not authenticated or invalid scope",
                ),
            );
        }
        let args = request.get("args");
        if let Err(message) = validate::set_activity(args) {
            return fail(self, RpcError::new(codes::INVALID_PAYLOAD, message));
        }
        let args = args.cloned().unwrap_or_else(|| json!({}));
        let application = self.application.clone().unwrap_or(Value::Null);
        let now = (self.shared.clock)();
        match activity::set_activity(
            self.shared.dir.as_ref(),
            &self.shared.cache,
            &application,
            &args,
            now,
        )
        .await
        {
            Err(error) => fail(self, error),
            Ok(Outcome::Cleared { pid }) => {
                self.shared.emit(Event::Activity {
                    socket: self.id,
                    pid: Some(pid),
                    application_id: application["id"].as_str().map(str::to_owned),
                    activity: None,
                    party_privacy: None,
                });
                vec![self.dispatch(nonce, name, Value::Null, Value::Null)]
            }
            Ok(Outcome::Set {
                pid,
                activity,
                party_privacy,
            }) => {
                self.shared.emit(Event::Activity {
                    socket: self.id,
                    pid: Some(pid),
                    application_id: application["id"].as_str().map(str::to_owned),
                    activity: Some(activity.clone()),
                    party_privacy,
                });
                vec![self.dispatch(nonce, name, Value::Null, activity)]
            }
        }
    }
}

/// Serves one client until it leaves, is refused or the server stops. Emits `Disconnected` at the end.
pub async fn serve<S>(stream: S, shared: &Shared, mut stop: watch::Receiver<bool>)
where
    S: AsyncRead + AsyncWrite + Send + Unpin,
{
    let id = shared.next_socket.fetch_add(1, Ordering::SeqCst);
    let active = shared.active.fetch_add(1, Ordering::SeqCst) + 1;
    let (mut reader, mut writer) = tokio::io::split(stream);
    if active > shared.config.max_connections {
        let frame = encode(
            Opcode::Close,
            &json!({"code": close_codes::ABNORMAL, "message": "Server at capacity"}),
        );
        let _ = writer.write_all(&frame).await;
        let _ = writer.shutdown().await;
        shared.active.fetch_sub(1, Ordering::SeqCst);
        return;
    }
    let mut session = Session {
        shared,
        id,
        did_handshake: false,
        application: None,
    };
    let mut decoder = Decoder::default();
    let mut buffer = vec![0u8; 16 * 1024];
    let handshake_timeout = tokio::time::sleep(shared.config.handshake_timeout);
    tokio::pin!(handshake_timeout);
    let mut waiting_for_handshake = true;
    let farewell = 'connection: loop {
        tokio::select! {
            read = reader.read(&mut buffer) => {
                let count = match read {
                    Ok(0) | Err(_) => break 'connection None,
                    Ok(count) => count,
                };
                if let Err(error) = decoder.push(&buffer[..count]) {
                    break 'connection Some(Close::new(close_codes::UNSUPPORTED, error.0));
                }
                loop {
                    let (opcode, payload) = match decoder.next_frame() {
                        Ok(Some(frame)) => frame,
                        Ok(None) => break,
                        Err(error) => {
                            break 'connection Some(Close::new(close_codes::UNSUPPORTED, error.0));
                        }
                    };
                    if opcode == Opcode::Handshake {
                        waiting_for_handshake = false;
                    }
                    match session.frame(opcode, payload).await {
                        Ok(frames) => {
                            for frame in frames {
                                if writer.write_all(&frame).await.is_err() {
                                    break 'connection None;
                                }
                            }
                        }
                        Err(close) => break 'connection Some(close),
                    }
                }
            }
            () = &mut handshake_timeout, if waiting_for_handshake => {
                break 'connection Some(Close::new(close_codes::ABNORMAL, "Handshake timeout"));
            }
            changed = stop.changed() => {
                if changed.is_err() || *stop.borrow() {
                    break 'connection Some(Close::new(close_codes::NORMAL, "User logged out"));
                }
            }
        }
    };
    if let Some(close) = farewell {
        let frame = encode(
            Opcode::Close,
            &json!({"code": close.code, "message": close.message}),
        );
        let _ = writer.write_all(&frame).await;
        let _ = writer.shutdown().await;
    }
    shared.active.fetch_sub(1, Ordering::SeqCst);
    shared.emit(Event::Disconnected { socket: id });
}

/// The pause the handshake limit gives a client that never sends one (`10 s` in the official server).
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
