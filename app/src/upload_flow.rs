//! The cloud upload of a message's files with the progress its placeholder row shows, and the cancel of that row
//! (module 48325): for each file `POST /channels/{id}/attachments` asks for a signed address, then the bytes are
//! PUT there while `XMLHttpRequest.upload.onprogress` reports (throttled to 50 ms: `createResumeAwareProgressFn`).
//! The message that follows names each upload (`OW` in module 292348).
//!
//! An upload that stops with an error code of the server ends as a failed message; one that stops without a code
//! (the signed address refused the bytes, the connection broke) or that the cancel button stopped leaves nothing
//! behind: the placeholder row goes and the composer stays empty (design/golden/windows/upload/after-failure).

use crate::net::{Upload, attachment_entry};
use crate::send_policy::Failure;
use dc_http::Client;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tokio::sync::Notify;

/// `createResumeAwareProgressFn` reports at most this often.
const PROGRESS_EVERY: Duration = Duration::from_millis(50);

/// Called with the index of the file, the bytes sent and the file's size.
pub(crate) type Report = Arc<dyn Fn(usize, u64, u64) + Send + Sync>;

/// How an upload stopped without finishing.
#[derive(Debug, PartialEq)]
pub(crate) enum End {
    /// The cancel button of the placeholder row.
    Cancelled,
    /// The server's answer, with its error code when it gave one.
    Failed(Failure),
}

impl End {
    /// Whether the user sees anything of this end: a coded failure turns the message red, the rest is silent.
    #[cfg(test)]
    pub(crate) fn is_silent(&self) -> bool {
        match self {
            End::Cancelled => true,
            End::Failed(failure) => failure.code.is_none(),
        }
    }
}

fn cancels() -> &'static Mutex<HashMap<String, Arc<Notify>>> {
    static CANCELS: OnceLock<Mutex<HashMap<String, Arc<Notify>>>> = OnceLock::new();
    CANCELS.get_or_init(Mutex::default)
}

/// The signal the cancel button of message `nonce` sends.
pub(crate) fn register(nonce: &str) -> Arc<Notify> {
    let signal = Arc::new(Notify::new());
    cancels()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(nonce.to_owned(), signal.clone());
    signal
}

pub(crate) fn forget(nonce: &str) {
    cancels()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .remove(nonce);
}

/// Stops the upload of message `nonce` if one is running.
pub(crate) fn cancel(nonce: &str) {
    if let Some(signal) = cancels()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(nonce)
    {
        signal.notify_one();
    }
}

fn failure_of(error: &dc_http::Error) -> Failure {
    match error {
        dc_http::Error::Status { status, body, .. } => {
            Failure::from_response(status.as_u16(), body)
        }
        _ => Failure::default(),
    }
}

/// The attachments entries of the message, after every file went up; `cancel` ends it early.
pub(crate) async fn upload_files(
    client: &Client,
    channel: u64,
    files: &[Upload],
    report: Report,
    cancel: Arc<Notify>,
) -> Result<Vec<Value>, End> {
    tokio::select! {
        done = run(client, channel, files, report) => done,
        () = cancel.notified() => Err(End::Cancelled),
    }
}

async fn run(
    client: &Client,
    channel: u64,
    files: &[Upload],
    report: Report,
) -> Result<Vec<Value>, End> {
    let mut attachments = Vec::new();
    for (index, file) in files.iter().enumerate() {
        let bytes = std::fs::read(&file.path).map_err(|_| End::Failed(Failure::default()))?;
        let filename = file.filename();
        let request = json!({"files": [{
            "filename": filename,
            "file_size": bytes.len(),
            "id": (index + 1).to_string(),
            "is_clip": false,
        }]});
        let granted = client
            .post(&format!("/channels/{channel}/attachments"), &request)
            .await
            .map_err(|error| End::Failed(failure_of(&error)))?;
        let slot = &granted["attachments"][0];
        let (Some(url), Some(uploaded)) = (
            slot["upload_url"].as_str(),
            slot["upload_filename"].as_str(),
        ) else {
            return Err(End::Failed(Failure::default()));
        };
        let last = Mutex::new(None::<Instant>);
        let report = report.clone();
        client
            .put_upload_with_progress(
                url,
                bytes,
                Arc::new(move |sent, total| {
                    let mut last = last.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                    let due = last.is_none_or(|at| at.elapsed() >= PROGRESS_EVERY);
                    if due || sent == total {
                        *last = Some(Instant::now());
                        report(index, sent, total);
                    }
                }),
            )
            .await
            .map_err(|error| End::Failed(failure_of(&error)))?;
        attachments.push(attachment_entry(
            index,
            &filename,
            uploaded,
            file.description.as_deref(),
        ));
    }
    Ok(attachments)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dc_proto::identity::Identity;
    use dc_proto::secrets::Token;
    use std::path::PathBuf;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// What the local server saw of one request.
    #[derive(Debug, Clone)]
    struct Seen {
        method: String,
        path: String,
        body: Vec<u8>,
    }

    /// How the local server answers the PUT.
    #[derive(Clone, Copy)]
    enum Put {
        Ok,
        Fails,
        Never,
    }

    /// A server for the two requests of an upload: the grant (`POST .../attachments`, with `grant_status`) and the
    /// PUT of the bytes. Every request it reads goes into the list.
    async fn serve(
        grant_status: u16,
        put: Put,
    ) -> (String, Arc<Mutex<Vec<Seen>>>, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let record = seen.clone();
        let base = origin.clone();
        let task = tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    return;
                };
                let record = record.clone();
                let base = base.clone();
                tokio::spawn(async move {
                    let mut bytes = Vec::new();
                    let mut buffer = [0_u8; 16 * 1024];
                    let (head_end, length) = loop {
                        let read = stream.read(&mut buffer).await.unwrap_or(0);
                        if read == 0 {
                            return;
                        }
                        bytes.extend_from_slice(&buffer[..read]);
                        if let Some(at) = bytes.windows(4).position(|window| window == b"\r\n\r\n")
                        {
                            let head = String::from_utf8_lossy(&bytes[..at]).to_lowercase();
                            let length = head
                                .lines()
                                .find_map(|line| line.strip_prefix("content-length: "))
                                .and_then(|value| value.trim().parse::<usize>().ok())
                                .unwrap_or(0);
                            break (at + 4, length);
                        }
                    };
                    while bytes.len() < head_end + length {
                        let read = stream.read(&mut buffer).await.unwrap_or(0);
                        if read == 0 {
                            return;
                        }
                        bytes.extend_from_slice(&buffer[..read]);
                    }
                    let head = String::from_utf8_lossy(&bytes[..head_end]).into_owned();
                    let mut line = head.lines().next().unwrap_or("").split(' ');
                    let (method, path) = (
                        line.next().unwrap_or("").to_owned(),
                        line.next().unwrap_or("").to_owned(),
                    );
                    record.lock().unwrap().push(Seen {
                        method: method.clone(),
                        path: path.clone(),
                        body: bytes[head_end..].to_vec(),
                    });
                    let (status, body) = if method == "POST" {
                        if grant_status == 200 {
                            (
                                200,
                                json!({"attachments": [{
                                    "id": 1,
                                    "upload_url": format!("{base}/put"),
                                    "upload_filename": "up/abc/notes.txt",
                                }]})
                                .to_string(),
                            )
                        } else {
                            (
                                grant_status,
                                json!({"code": 50013, "message": "no"}).to_string(),
                            )
                        }
                    } else {
                        match put {
                            Put::Ok => (200, String::new()),
                            Put::Fails => (500, String::new()),
                            Put::Never => {
                                tokio::time::sleep(Duration::from_secs(30)).await;
                                return;
                            }
                        }
                    };
                    let reason = if status == 200 { "OK" } else { "Error" };
                    let answer = format!(
                        "HTTP/1.1 {status} {reason}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = stream.write_all(answer.as_bytes()).await;
                });
            }
        });
        (origin, seen, task)
    }

    fn client(origin: &str) -> Client {
        Client::new(Token::new("fixture"), Identity::new())
            .unwrap()
            .with_api_base(origin.to_owned())
    }

    fn fixture(name: &str, bytes: &[u8]) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("gravecord-upload-{}-{name}", std::process::id()));
        std::fs::write(&path, bytes).unwrap();
        path
    }

    fn upload(path: PathBuf, name: &str, description: Option<&str>) -> Upload {
        Upload {
            path,
            name: name.to_owned(),
            description: description.map(str::to_owned),
        }
    }

    #[tokio::test]
    async fn an_upload_asks_for_an_address_then_puts_the_bytes_and_names_the_file() {
        let (origin, seen, server) = serve(200, Put::Ok).await;
        let path = fixture("ok.txt", &[9; 150_000]);
        let steps = Arc::new(Mutex::new(Vec::new()));
        let record = steps.clone();
        let entries = upload_files(
            &client(&origin),
            77,
            &[upload(path.clone(), "SPOILER_notes.txt", Some("a note"))],
            Arc::new(move |index, sent, total| record.lock().unwrap().push((index, sent, total))),
            Arc::new(Notify::new()),
        )
        .await
        .unwrap();
        let seen = seen.lock().unwrap().clone();
        assert_eq!(seen.len(), 2);
        assert_eq!(
            (seen[0].method.as_str(), seen[0].path.as_str()),
            ("POST", "/channels/77/attachments")
        );
        let asked: Value = serde_json::from_slice(&seen[0].body).unwrap();
        assert_eq!(
            asked,
            json!({"files": [{"filename": "SPOILER_notes.txt", "file_size": 150_000, "id": "1", "is_clip": false}]})
        );
        assert_eq!(
            (seen[1].method.as_str(), seen[1].path.as_str()),
            ("PUT", "/put")
        );
        assert_eq!(seen[1].body.len(), 150_000);
        assert_eq!(
            entries,
            vec![json!({
                "id": "0",
                "filename": "SPOILER_notes.txt",
                "uploaded_filename": "up/abc/notes.txt",
                "description": "a note",
            })]
        );
        let steps = steps.lock().unwrap();
        assert_eq!(*steps.last().unwrap(), (0, 150_000, 150_000));
        assert!(steps.windows(2).all(|pair| pair[0].1 <= pair[1].1));
        server.abort();
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn a_refusal_with_a_code_is_seen_and_one_without_is_silent() {
        let (origin, _, server) = serve(403, Put::Ok).await;
        let path = fixture("coded.txt", b"hello");
        let ended = upload_files(
            &client(&origin),
            5,
            &[upload(path.clone(), "coded.txt", None)],
            Arc::new(|_, _, _| {}),
            Arc::new(Notify::new()),
        )
        .await
        .unwrap_err();
        match &ended {
            End::Failed(failure) => {
                assert_eq!((failure.status, failure.code), (Some(403), Some(50013)));
            }
            other => panic!("{other:?}"),
        }
        assert!(!ended.is_silent());
        server.abort();

        let (origin, _, server) = serve(200, Put::Fails).await;
        let ended = upload_files(
            &client(&origin),
            5,
            &[upload(path.clone(), "coded.txt", None)],
            Arc::new(|_, _, _| {}),
            Arc::new(Notify::new()),
        )
        .await
        .unwrap_err();
        assert!(ended.is_silent(), "{ended:?}");
        server.abort();
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn the_cancel_button_stops_a_running_upload() {
        let (origin, _, server) = serve(200, Put::Never).await;
        let path = fixture("cancel.txt", b"hello");
        let nonce = "cancel-test-nonce";
        let signal = register(nonce);
        let canceller = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(300)).await;
            cancel(nonce);
        });
        let ended = upload_files(
            &client(&origin),
            5,
            &[upload(path.clone(), "cancel.txt", None)],
            Arc::new(|_, _, _| {}),
            signal,
        )
        .await
        .unwrap_err();
        assert_eq!(ended, End::Cancelled);
        assert!(ended.is_silent());
        forget(nonce);
        canceller.await.unwrap();
        server.abort();
        let _ = std::fs::remove_file(path);
    }
}
