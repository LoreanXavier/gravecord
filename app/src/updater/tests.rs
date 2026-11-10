use super::*;
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};

fn version(text: &str) -> Version {
    Version::parse(text).unwrap()
}

fn digest(data: &[u8]) -> [u8; 32] {
    Sha256::digest(data).into()
}

fn hex(hash: &[u8; 32]) -> String {
    hash.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn feed_json(version: &str, url: &str, data: &[u8]) -> String {
    format!(
        r#"{{"version":"{version}","url":"{url}","size":{},"sha256":"{}"}}"#,
        data.len(),
        hex(&digest(data))
    )
}

/// A scratch folder, deleted when dropped.
struct Scratch(PathBuf);
impl Scratch {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("gc-updater-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

// ---- versions ----

#[test]
fn versions_compare_as_semantic_versions() {
    assert!(version("0.1.0") > version("0.0.9"));
    assert!(version("1.0.0") > version("0.99.99"));
    assert!(version("0.10.0") > version("0.9.0"));
    assert!(version("1.0.0") > version("1.0.0-rc.1"));
    assert!(version("1.0.0-rc.2") > version("1.0.0-rc.1"));
    assert!(version("1.0.0-rc.10") > version("1.0.0-rc.9"));
    assert!(version("1.0.0-rc.1") > version("1.0.0-alpha"));
    assert_eq!(version("v1.2.3"), version("1.2.3"));
    assert_eq!(version("1.2.3").to_string(), "1.2.3");
    assert_eq!(version("1.2.3-beta.1").to_string(), "1.2.3-beta.1");
}

#[test]
fn malformed_versions_are_rejected() {
    for bad in [
        "",
        "1",
        "1.2",
        "1.2.3.4",
        "a.b.c",
        "1.2.x",
        "1.2.3-",
        "1.2.3-a..b",
        "-1.2.3",
        "1.2.3-é",
    ] {
        assert!(Version::parse(bad).is_err(), "{bad:?} should not parse");
    }
}

#[test]
fn only_a_newer_version_is_an_update_and_a_downgrade_is_refused() {
    let current = version("0.2.0");
    assert!(is_update(&current, &version("0.2.1")));
    assert!(!is_update(&current, &version("0.2.0")));
    assert!(!is_update(&current, &version("0.1.9")));
    assert!(!is_update(&current, &version("0.2.0-rc.1")));
}

#[test]
fn this_build_has_a_version_and_the_default_feed_names_the_repository() {
    let _ = current_version();
    assert_eq!(
        DEFAULT_FEED,
        "https://github.com/LoreanXavier/gravecord/releases/latest/download/update.json"
    );
    assert!(Policy::STRICT.feed_url(DEFAULT_FEED).is_ok());
}

// ---- policy ----

#[test]
fn the_feed_address_must_be_https_with_a_host_and_no_credentials() {
    let strict = Policy::STRICT;
    assert!(strict.feed_url("https://example.com/update.json").is_ok());
    assert!(strict.feed_url("http://example.com/update.json").is_err());
    assert!(strict.feed_url("http://127.0.0.1:9/update.json").is_err());
    assert!(strict.feed_url("file:///C:/update.json").is_err());
    assert!(strict.feed_url("ftp://example.com/update.json").is_err());
    assert!(strict.feed_url("not a url").is_err());
    assert!(
        strict
            .feed_url("https://user:pw@example.com/update.json")
            .is_err()
    );
    let loopback = Policy::loopback_for_tests();
    assert!(loopback.feed_url("http://127.0.0.1:9/update.json").is_ok());
    assert!(loopback.feed_url("http://example.com/update.json").is_err());
}

#[test]
fn redirects_stay_on_the_feed_host_and_github_may_use_its_download_host() {
    let strict = Policy::STRICT;
    let feed = Url::parse("https://example.com/a/update.json").unwrap();
    let go = |from: &Url, to: &str| redirect_allowed(from, &Url::parse(to).unwrap(), strict);
    assert!(go(&feed, "https://example.com/b/setup.exe"));
    assert!(go(&feed, "https://EXAMPLE.com/b/setup.exe"));
    assert!(!go(&feed, "https://evil.example.org/setup.exe"));
    assert!(!go(&feed, "https://example.com.evil.org/setup.exe"));
    assert!(!go(&feed, "http://example.com/setup.exe"));
    assert!(!go(&feed, "https://objects.githubusercontent.com/x"));
    let github = Url::parse(DEFAULT_FEED).unwrap();
    assert!(go(
        &github,
        "https://github.com/LoreanXavier/gravecord/releases/download/v1/update.json"
    ));
    assert!(go(
        &github,
        "https://release-assets.githubusercontent.com/x"
    ));
    assert!(!go(&github, "https://githubusercontent.com.evil.org/x"));
    assert!(!go(&github, "https://example.com/x"));
}

// ---- the feed ----

fn feed() -> Url {
    Url::parse("https://example.com/releases/latest/download/update.json").unwrap()
}

#[test]
fn a_good_feed_parses_and_a_relative_setup_address_resolves_against_the_feed() {
    let data = b"setup bytes";
    let json = feed_json("0.3.0", "gravecord-setup.exe", data);
    let manifest = Manifest::parse(json.as_bytes(), &feed(), Policy::STRICT).unwrap();
    assert_eq!(manifest.version, version("0.3.0"));
    assert_eq!(
        manifest.url.as_str(),
        "https://example.com/releases/latest/download/gravecord-setup.exe"
    );
    assert_eq!(manifest.size, data.len() as u64);
    assert_eq!(manifest.sha256, digest(data));
    let absolute = feed_json("0.3.0", "https://example.com/dl/setup.exe", data);
    assert!(Manifest::parse(absolute.as_bytes(), &feed(), Policy::STRICT).is_ok());
}

#[test]
fn bad_feeds_are_rejected() {
    let good_hash = hex(&digest(b"x"));
    let parse = |json: &str| Manifest::parse(json.as_bytes(), &feed(), Policy::STRICT);
    let with = |version: &str, url: &str, size: &str, hash: &str| {
        format!(r#"{{"version":"{version}","url":"{url}","size":{size},"sha256":"{hash}"}}"#)
    };
    assert!(parse(&with("1.0.0", "setup.exe", "10", &good_hash)).is_ok());
    // Not JSON, wrong shapes, missing fields (the hash is mandatory).
    assert!(parse("").is_err());
    assert!(parse("not json").is_err());
    assert!(parse("[]").is_err());
    assert!(parse(r#"{"version":"1.0.0","url":"setup.exe","size":10}"#).is_err());
    assert!(parse(r#"{"version":"1.0.0","url":"setup.exe","sha256":"00"}"#).is_err());
    // Bad versions, sizes and hashes.
    assert!(parse(&with("one", "setup.exe", "10", &good_hash)).is_err());
    assert!(parse(&with("1.0.0", "setup.exe", "0", &good_hash)).is_err());
    assert!(parse(&with("1.0.0", "setup.exe", "-5", &good_hash)).is_err());
    assert!(parse(&with("1.0.0", "setup.exe", "99999999999", &good_hash)).is_err());
    assert!(parse(&with("1.0.0", "setup.exe", "10", "abc")).is_err());
    assert!(parse(&with("1.0.0", "setup.exe", "10", &"g".repeat(64))).is_err());
    assert!(parse(&with("1.0.0", "setup.exe", "10", "")).is_err());
    // The setup file must be https and on the feed's host.
    assert!(
        parse(&with(
            "1.0.0",
            "http://example.com/setup.exe",
            "10",
            &good_hash
        ))
        .is_err()
    );
    assert!(
        parse(&with(
            "1.0.0",
            "https://other.example.org/setup.exe",
            "10",
            &good_hash
        ))
        .is_err()
    );
    assert!(
        parse(&with(
            "1.0.0",
            "https://example.com:8443/setup.exe",
            "10",
            &good_hash
        ))
        .is_err()
    );
    assert!(
        parse(&with(
            "1.0.0",
            "https://u:p@example.com/setup.exe",
            "10",
            &good_hash
        ))
        .is_err()
    );
    assert!(parse(&with("1.0.0", "file:///C:/setup.exe", "10", &good_hash)).is_err());
    assert!(
        parse(&with(
            "1.0.0",
            "//other.example.org/setup.exe",
            "10",
            &good_hash
        ))
        .is_err()
    );
}

// ---- the file ----

#[test]
fn verification_accepts_the_right_file_and_refuses_a_wrong_size_or_hash() {
    let dir = Scratch::new("verify");
    let path = dir.0.join("setup.exe");
    std::fs::write(&path, b"the real setup").unwrap();
    let hash = digest(b"the real setup");
    assert!(verify_file(&path, 14, &hash).is_ok());
    assert!(verify_file(&path, 13, &hash).is_err());
    assert!(verify_file(&path, 14, &digest(b"another file")).is_err());
    assert!(verify_file(&dir.0.join("missing.exe"), 14, &hash).is_err());
    // Same size, one byte changed.
    std::fs::write(&path, b"the real setuq").unwrap();
    assert!(verify_file(&path, 14, &hash).is_err());
}

// ---- applying ----

fn ready_in(dir: &Path, data: &[u8]) -> Ready {
    let path = dir.join("gravecord-setup-9.9.9.exe");
    std::fs::write(&path, data).unwrap();
    Ready {
        manifest: Manifest {
            version: version("9.9.9"),
            url: Url::parse("https://example.com/setup.exe").unwrap(),
            size: data.len() as u64,
            sha256: digest(data),
        },
        path,
    }
}

#[test]
fn the_setup_runs_silently_in_place_and_restarts_the_app() {
    let args = setup_args(Path::new(r"C:\Users\x\AppData\Local\Programs\gravecord"));
    let args: Vec<String> = args
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        args,
        [
            "--silent",
            "--launch",
            "--dir",
            r"C:\Users\x\AppData\Local\Programs\gravecord"
        ]
    );
}

#[test]
fn a_capture_run_never_runs_the_setup() {
    let dir = Scratch::new("capture");
    let ready = ready_in(&dir.0, b"fake setup");
    let mut ran = false;
    let result = apply_with(&ready, &dir.0, true, |_, _| {
        ran = true;
        Ok(())
    });
    assert!(result.is_err());
    assert!(!ran);
}

#[test]
fn a_file_changed_after_download_is_not_run() {
    let dir = Scratch::new("tamper");
    let ready = ready_in(&dir.0, b"fake setup");
    std::fs::write(&ready.path, b"fake setuq").unwrap();
    let mut ran = false;
    let result = apply_with(&ready, &dir.0, false, |_, _| {
        ran = true;
        Ok(())
    });
    assert!(result.is_err());
    assert!(!ran);
}

#[test]
fn a_verified_file_is_run_with_the_installers_arguments() {
    let dir = Scratch::new("run");
    let ready = ready_in(&dir.0, b"fake setup");
    let mut seen = None;
    apply_with(&ready, &dir.0, false, |path, args| {
        seen = Some((path.to_owned(), args.to_vec()));
        Ok(())
    })
    .unwrap();
    let (path, args) = seen.unwrap();
    assert_eq!(path, ready.path);
    assert_eq!(args, setup_args(&dir.0));
}

#[test]
fn only_an_install_is_updated() {
    let dir = Scratch::new("installed");
    let exe = dir.0.join("gravecord.exe");
    assert_eq!(installed_dir(&exe), None);
    std::fs::write(dir.0.join("install.manifest"), "version 1\n").unwrap();
    assert_eq!(installed_dir(&exe), Some(dir.0.clone()));
}

#[test]
fn sweeping_keeps_only_the_wanted_files() {
    let dir = Scratch::new("sweep");
    for name in ["keep.exe", "keep.exe.part", "old.exe", "other.part"] {
        std::fs::write(dir.0.join(name), b"x").unwrap();
    }
    sweep(
        &dir.0,
        &[dir.0.join("keep.exe"), dir.0.join("keep.exe.part")],
    );
    assert!(dir.0.join("keep.exe").exists() && dir.0.join("keep.exe.part").exists());
    assert!(!dir.0.join("old.exe").exists() && !dir.0.join("other.part").exists());
}

// ---- a local server ----

enum Route {
    /// Answers 200 with everything, whatever range was asked.
    Body(Vec<u8>),
    /// Serves the body and honours `Range: bytes=N-`.
    Ranged(Vec<u8>),
    Redirect(String),
}

struct Server {
    port: u16,
    stop: Arc<AtomicBool>,
    log: Arc<Mutex<Vec<String>>>,
}

impl Server {
    /// Binds 127.0.0.1 on a free port. The routes not listed answer 404.
    fn start(routes: HashMap<&'static str, Route>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let stop = Arc::new(AtomicBool::new(false));
        let log = Arc::new(Mutex::new(Vec::new()));
        let (stopped, logged) = (stop.clone(), log.clone());
        std::thread::spawn(move || {
            while !stopped.load(AtomicOrdering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let _ = stream.set_nonblocking(false);
                        handle(stream, &routes, &logged);
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(5)),
                }
            }
        });
        Self { port, stop, log }
    }

    fn url(&self, path: &str) -> Url {
        Url::parse(&format!("http://127.0.0.1:{}{path}", self.port)).unwrap()
    }

    fn requests(&self) -> Vec<String> {
        self.log.lock().unwrap().clone()
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, AtomicOrdering::Relaxed);
    }
}

fn handle(
    mut stream: std::net::TcpStream,
    routes: &HashMap<&'static str, Route>,
    log: &Mutex<Vec<String>>,
) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut first = String::new();
    if reader.read_line(&mut first).is_err() {
        return;
    }
    let mut range = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).is_err() || line.trim().is_empty() {
            break;
        }
        if let Some(value) = line.to_ascii_lowercase().strip_prefix("range: bytes=") {
            range = value.trim().trim_end_matches('-').parse::<usize>().ok();
        }
    }
    let path = first.split_whitespace().nth(1).unwrap_or("/").to_owned();
    log.lock().unwrap().push(match range {
        Some(from) => format!("{path} range={from}"),
        None => path.clone(),
    });
    let head = |status: &str, extra: &str, length: usize| {
        format!("HTTP/1.1 {status}\r\n{extra}Content-Length: {length}\r\nConnection: close\r\n\r\n")
    };
    match routes.get(path.as_str()) {
        None => {
            let _ = stream.write_all(head("404 Not Found", "", 0).as_bytes());
        }
        Some(Route::Redirect(to)) => {
            let extra = format!("Location: {to}\r\n");
            let _ = stream.write_all(head("302 Found", &extra, 0).as_bytes());
        }
        Some(Route::Body(body)) => {
            let _ = stream.write_all(head("200 OK", "", body.len()).as_bytes());
            let _ = stream.write_all(body);
        }
        Some(Route::Ranged(body)) => match range {
            Some(from) if from < body.len() => {
                let extra = format!(
                    "Content-Range: bytes {from}-{}/{}\r\n",
                    body.len() - 1,
                    body.len()
                );
                let _ = stream
                    .write_all(head("206 Partial Content", &extra, body.len() - from).as_bytes());
                let _ = stream.write_all(&body[from..]);
            }
            Some(_) => {
                let _ = stream.write_all(head("416 Range Not Satisfiable", "", 0).as_bytes());
            }
            None => {
                let _ = stream.write_all(head("200 OK", "", body.len()).as_bytes());
                let _ = stream.write_all(body);
            }
        },
    }
    let _ = stream.flush();
}

fn client_for(feed: &Url) -> reqwest::Client {
    build_client(feed, Policy::loopback_for_tests()).unwrap()
}

async fn offered(client: &reqwest::Client, feed: &Url) -> Manifest {
    match check(
        client,
        feed,
        Policy::loopback_for_tests(),
        &version("0.4.0"),
    )
    .await
    .unwrap()
    {
        Check::Available(manifest) => manifest,
        Check::NoUpdate => panic!("an update is offered"),
    }
}

#[tokio::test]
async fn a_missing_feed_is_no_update_and_not_an_error() {
    let server = Server::start(HashMap::new());
    let feed = server.url("/update.json");
    let outcome = check(
        &client_for(&feed),
        &feed,
        Policy::loopback_for_tests(),
        &version("0.0.0"),
    )
    .await;
    assert_eq!(outcome.unwrap(), Check::NoUpdate);
}

#[tokio::test]
async fn an_unreachable_feed_is_an_error_the_caller_only_logs() {
    // A refused connection: nothing listens on the port.
    let port = {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    };
    let feed = Url::parse(&format!("http://127.0.0.1:{port}/update.json")).unwrap();
    let outcome = check(
        &client_for(&feed),
        &feed,
        Policy::loopback_for_tests(),
        &version("0.0.0"),
    )
    .await;
    assert!(outcome.is_err());
}

#[tokio::test]
async fn the_feed_offers_a_newer_version_and_ignores_an_older_one() {
    let data = b"fake setup file".to_vec();
    let json = feed_json("0.5.0", "gravecord-setup.exe", &data);
    let server = Server::start(HashMap::from([(
        "/update.json",
        Route::Body(json.into_bytes()),
    )]));
    let feed = server.url("/update.json");
    let client = client_for(&feed);
    let policy = Policy::loopback_for_tests();
    assert_eq!(offered(&client, &feed).await.version, version("0.5.0"));
    assert_eq!(
        check(&client, &feed, policy, &version("0.5.0"))
            .await
            .unwrap(),
        Check::NoUpdate
    );
    assert_eq!(
        check(&client, &feed, policy, &version("0.6.0"))
            .await
            .unwrap(),
        Check::NoUpdate
    );
}

#[tokio::test]
async fn a_bad_feed_from_the_server_is_rejected() {
    let server = Server::start(HashMap::from([(
        "/update.json",
        Route::Body(b"{\"version\":\"9.0.0\"}".to_vec()),
    )]));
    let feed = server.url("/update.json");
    let outcome = check(
        &client_for(&feed),
        &feed,
        Policy::loopback_for_tests(),
        &version("0.0.0"),
    )
    .await;
    assert!(outcome.is_err());
}

#[tokio::test]
async fn download_and_verification_end_to_end_without_running_anything() {
    let data: Vec<u8> = (0..200_000u32).map(|n| (n % 251) as u8).collect();
    let json = feed_json("0.5.0", "gravecord-setup.exe", &data);
    let server = Server::start(HashMap::from([
        ("/update.json", Route::Body(json.into_bytes())),
        ("/gravecord-setup.exe", Route::Ranged(data.clone())),
    ]));
    let feed = server.url("/update.json");
    let client = client_for(&feed);
    let manifest = offered(&client, &feed).await;
    let dir = Scratch::new("download");
    let last = Mutex::new((0, 0));
    let path = download(&client, &manifest, &dir.0, &|done, total| {
        *last.lock().unwrap() = (done, total);
    })
    .await
    .unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), data);
    assert_eq!(
        *last.lock().unwrap(),
        (data.len() as u64, data.len() as u64)
    );
    assert!(path.ends_with("gravecord-setup-0.5.0.exe"));
    assert!(!path.with_extension("exe.part").exists());
    verify_file(&path, manifest.size, &manifest.sha256).unwrap();
    // A second call finds the verified file and does not ask the server again.
    let before = server.requests().len();
    download(&client, &manifest, &dir.0, &|_, _| {})
        .await
        .unwrap();
    assert_eq!(server.requests().len(), before);
    // The file only ever reaches a runner through `apply_with`, which here records instead of running.
    let ready = Ready { manifest, path };
    let mut ran = Vec::new();
    apply_with(&ready, &dir.0, false, |path, args| {
        ran.push((path.to_owned(), args.len()));
        Ok(())
    })
    .unwrap();
    assert_eq!(ran.len(), 1);
}

#[tokio::test]
async fn an_interrupted_download_resumes_with_a_range_request() {
    let data: Vec<u8> = (0..100_000u32).map(|n| (n % 241) as u8).collect();
    let json = feed_json("0.5.0", "gravecord-setup.exe", &data);
    let server = Server::start(HashMap::from([
        ("/update.json", Route::Body(json.into_bytes())),
        ("/gravecord-setup.exe", Route::Ranged(data.clone())),
    ]));
    let feed = server.url("/update.json");
    let client = client_for(&feed);
    let manifest = offered(&client, &feed).await;
    let dir = Scratch::new("resume");
    std::fs::write(
        dir.0.join("gravecord-setup-0.5.0.exe.part"),
        &data[..40_000],
    )
    .unwrap();
    let path = download(&client, &manifest, &dir.0, &|_, _| {})
        .await
        .unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), data);
    assert!(
        server
            .requests()
            .contains(&"/gravecord-setup.exe range=40000".to_owned())
    );
}

#[tokio::test]
async fn a_server_that_ignores_the_range_restarts_the_file() {
    let data: Vec<u8> = (0..50_000u32).map(|n| (n % 239) as u8).collect();
    let json = feed_json("0.5.0", "gravecord-setup.exe", &data);
    let server = Server::start(HashMap::from([
        ("/update.json", Route::Body(json.into_bytes())),
        ("/gravecord-setup.exe", Route::Body(data.clone())),
    ]));
    let feed = server.url("/update.json");
    let client = client_for(&feed);
    let manifest = offered(&client, &feed).await;
    let dir = Scratch::new("norange");
    std::fs::write(
        dir.0.join("gravecord-setup-0.5.0.exe.part"),
        &data[..10_000],
    )
    .unwrap();
    let path = download(&client, &manifest, &dir.0, &|_, _| {})
        .await
        .unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), data);
}

#[tokio::test]
async fn a_file_that_does_not_match_the_feed_is_discarded() {
    let announced: Vec<u8> = vec![7; 30_000];
    let mut served = announced.clone();
    served[15_000] = 8;
    let json = feed_json("0.5.0", "gravecord-setup.exe", &announced);
    let server = Server::start(HashMap::from([
        ("/update.json", Route::Body(json.into_bytes())),
        ("/gravecord-setup.exe", Route::Ranged(served)),
    ]));
    let feed = server.url("/update.json");
    let client = client_for(&feed);
    let manifest = offered(&client, &feed).await;
    let dir = Scratch::new("corrupt");
    assert!(
        download(&client, &manifest, &dir.0, &|_, _| {})
            .await
            .is_err()
    );
    let left: Vec<_> = std::fs::read_dir(&dir.0).unwrap().flatten().collect();
    assert!(left.is_empty(), "nothing may be left behind: {left:?}");
}

#[tokio::test]
async fn a_file_larger_than_announced_is_discarded() {
    let announced = vec![1u8; 10_000];
    let json = feed_json("0.5.0", "gravecord-setup.exe", &announced);
    let server = Server::start(HashMap::from([
        ("/update.json", Route::Body(json.into_bytes())),
        ("/gravecord-setup.exe", Route::Ranged(vec![1u8; 20_000])),
    ]));
    let feed = server.url("/update.json");
    let client = client_for(&feed);
    let manifest = offered(&client, &feed).await;
    let dir = Scratch::new("larger");
    assert!(
        download(&client, &manifest, &dir.0, &|_, _| {})
            .await
            .is_err()
    );
    assert_eq!(std::fs::read_dir(&dir.0).unwrap().count(), 0);
}

#[tokio::test]
async fn a_redirect_to_another_host_is_refused() {
    let data = b"fake setup".to_vec();
    let json = feed_json("0.5.0", "gravecord-setup.exe", &data);
    let other = Server::start(HashMap::from([(
        "/gravecord-setup.exe",
        Route::Body(data.clone()),
    )]));
    // `localhost` is not the feed's host (`127.0.0.1`), even though both are this machine.
    let target = format!("http://localhost:{}/gravecord-setup.exe", other.port);
    let server = Server::start(HashMap::from([
        ("/update.json", Route::Body(json.into_bytes())),
        ("/gravecord-setup.exe", Route::Redirect(target)),
    ]));
    let feed = server.url("/update.json");
    let client = client_for(&feed);
    let manifest = offered(&client, &feed).await;
    let dir = Scratch::new("redirect");
    assert!(
        download(&client, &manifest, &dir.0, &|_, _| {})
            .await
            .is_err()
    );
    assert!(
        other.requests().is_empty(),
        "the other host must never be asked"
    );
}

#[tokio::test]
async fn a_redirect_on_the_same_host_is_followed() {
    let data = b"fake setup".to_vec();
    let json = feed_json("0.5.0", "gravecord-setup.exe", &data);
    let server = Server::start(HashMap::from([
        ("/update.json", Route::Body(json.into_bytes())),
        (
            "/gravecord-setup.exe",
            Route::Redirect("/cdn/setup.bin".to_owned()),
        ),
        ("/cdn/setup.bin", Route::Body(data.clone())),
    ]));
    let feed = server.url("/update.json");
    let client = client_for(&feed);
    let manifest = offered(&client, &feed).await;
    let dir = Scratch::new("samehost");
    let path = download(&client, &manifest, &dir.0, &|_, _| {})
        .await
        .unwrap();
    assert_eq!(std::fs::read(path).unwrap(), data);
}
