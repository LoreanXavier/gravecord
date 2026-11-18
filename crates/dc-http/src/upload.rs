//! The file upload of a message with the progress the placeholder row shows: `XMLHttpRequest.upload.onprogress`
//! of module 48325 (`createResumeAwareProgressFn`, throttled to 50 ms) as a body that reports the bytes it has
//! handed to the connection.

use super::{Client, Error};
use bytes::Bytes;
use http_body::{Body, Frame, SizeHint};
use std::convert::Infallible;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

/// What one read of the body hands the connection.
const CHUNK: usize = 64 * 1024;

/// Called with `(sent, total)` after each chunk.
pub type Progress = Arc<dyn Fn(u64, u64) + Send + Sync>;

struct ProgressBody {
    data: Bytes,
    sent: usize,
    report: Progress,
}

impl Body for ProgressBody {
    type Data = Bytes;
    type Error = Infallible;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        _: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Infallible>>> {
        let total = self.data.len();
        if self.sent >= total {
            return Poll::Ready(None);
        }
        let end = (self.sent + CHUNK).min(total);
        let chunk = self.data.slice(self.sent..end);
        self.sent = end;
        (self.report)(end as u64, total as u64);
        Poll::Ready(Some(Ok(Frame::data(chunk))))
    }

    fn is_end_stream(&self) -> bool {
        self.sent >= self.data.len()
    }

    fn size_hint(&self) -> SizeHint {
        SizeHint::with_exact((self.data.len() - self.sent) as u64)
    }
}

impl Client {
    /// [`Client::put_upload`] that reports how much of the file has gone out.
    pub async fn put_upload_with_progress(
        &self,
        url: &str,
        bytes: Vec<u8>,
        report: Progress,
    ) -> Result<(), Error> {
        let body = reqwest::Body::wrap(ProgressBody {
            data: Bytes::from(bytes),
            sent: 0,
            report,
        });
        self.http()
            .put(url)
            .header("content-type", "application/octet-stream")
            .body(body)
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dc_proto::identity::Identity;
    use dc_proto::secrets::Token;
    use std::sync::Mutex;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// A one-shot HTTP server: reads a whole PUT and answers 200.
    async fn serve_one() -> (String, tokio::task::JoinHandle<(String, usize)>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut seen = Vec::new();
            let mut buffer = [0_u8; 16 * 1024];
            let (head_end, length) = loop {
                let read = stream.read(&mut buffer).await.unwrap();
                seen.extend_from_slice(&buffer[..read]);
                if let Some(at) = seen.windows(4).position(|window| window == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&seen[..at]).to_lowercase();
                    let length = head
                        .lines()
                        .find_map(|line| line.strip_prefix("content-length: "))
                        .and_then(|value| value.trim().parse::<usize>().ok())
                        .unwrap_or(0);
                    break (at + 4, length);
                }
            };
            while seen.len() < head_end + length {
                let read = stream.read(&mut buffer).await.unwrap();
                seen.extend_from_slice(&buffer[..read]);
            }
            stream
                .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 0\r\nconnection: close\r\n\r\n")
                .await
                .unwrap();
            (
                String::from_utf8_lossy(&seen[..head_end]).into_owned(),
                length,
            )
        });
        (format!("http://{address}/upload"), task)
    }

    #[tokio::test]
    async fn the_whole_file_goes_out_in_steps_that_end_at_the_total() {
        let (url, server) = serve_one().await;
        let client = Client::new(Token::new("fixture"), Identity::new()).unwrap();
        let steps = Arc::new(Mutex::new(Vec::new()));
        let record = steps.clone();
        let size = 3 * CHUNK + 100;
        client
            .put_upload_with_progress(
                &url,
                vec![7; size],
                Arc::new(move |sent, total| record.lock().unwrap().push((sent, total))),
            )
            .await
            .unwrap();
        let (head, received) = server.await.unwrap();
        assert_eq!(received, size);
        assert!(head.to_lowercase().contains("content-length: "));
        assert!(!head.to_lowercase().contains("authorization"));
        let steps = steps.lock().unwrap();
        assert_eq!(steps.len(), 4);
        assert!(steps.windows(2).all(|pair| pair[0].0 < pair[1].0));
        assert_eq!(*steps.last().unwrap(), (size as u64, size as u64));
    }
}
