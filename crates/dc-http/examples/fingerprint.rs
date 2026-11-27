//! Prints what a TLS and HTTP/2 fingerprint echo service sees of the Discord client's handshake: the Chromium
//! emulation, the pinned user agent and the fixed browser headers only. No token, no account or system data, no
//! cookies. `cargo run --release -p dc-http --example fingerprint [url]` (default `https://tls.peet.ws/api/all`).

use dc_proto::pin;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let url = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "https://tls.peet.ws/api/all".to_owned());
    let client = wreq::Client::builder()
        .emulation(dc_http::chromium::emulation())
        .orig_headers(dc_http::chromium::fetch_order())
        .user_agent(pin::USER_AGENT)
        .build()?;
    let answer = client
        .get(url)
        .header("accept", "*/*")
        .header("accept-encoding", dc_http::chromium::ACCEPT_ENCODING)
        .header("sec-ch-ua", pin::SEC_CH_UA)
        .header("sec-ch-ua-mobile", "?0")
        .header("sec-ch-ua-platform", "\"Windows\"")
        .header("sec-fetch-site", "same-origin")
        .header("sec-fetch-mode", "cors")
        .header("sec-fetch-dest", "empty")
        .header("priority", "u=1, i")
        .send()
        .await?
        .text()
        .await?;
    println!("{answer}");
    Ok(())
}
