//! Files on an FTP server: a small server in the test answers as real ones
//! do (login, SIZE, MDTM, REST, EPSV / PASV, RETR).

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use swiftget_engine::ftp::{self, FtpJob};
use swiftget_engine::http::{build_client_with, Progress, ProxyConfig, Shared};
use swiftget_engine::limiter::SpeedLimiter;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

struct Server {
    data: Vec<u8>,
    user: &'static str,
    pass: &'static str,
    /// no EPSV (older servers): PASV only
    pasv_only: bool,
    retrs: AtomicU32,
}

async fn serve(s: Arc<Server>) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((sock, _)) = listener.accept().await {
            let s = s.clone();
            tokio::spawn(async move {
                let (r, mut w) = sock.into_split();
                let mut r = BufReader::new(r);
                let _ = w.write_all(b"220-Welcome\r\n220 ready\r\n").await;
                let mut user = String::new();
                let mut logged = false;
                let mut rest = 0usize;
                let mut data: Option<TcpListener> = None;
                loop {
                    let mut line = String::new();
                    if r.read_line(&mut line).await.unwrap_or(0) == 0 {
                        return;
                    }
                    let line = line.trim_end();
                    let (cmd, arg) = line.split_once(' ').unwrap_or((line, ""));
                    let answer: String = match cmd.to_ascii_uppercase().as_str() {
                        "USER" => {
                            user = arg.to_string();
                            "331 password please".into()
                        }
                        "PASS" => {
                            logged = user == s.user && arg == s.pass;
                            if logged { "230 in".into() } else { "530 Login incorrect".into() }
                        }
                        "AUTH" => "502 no".into(),
                        _ if !logged => "530 log in first".into(),
                        "TYPE" => "200 binary".into(),
                        "SIZE" if arg == "pub/f.iso" => format!("213 {}", s.data.len()),
                        "SIZE" => "550 no such file".into(),
                        "MDTM" => "213 20260102030405".into(),
                        "REST" => {
                            rest = arg.parse().unwrap_or(0);
                            format!("350 restarting at {rest}")
                        }
                        "EPSV" if s.pasv_only => "500 unknown".into(),
                        "EPSV" | "PASV" => {
                            let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
                            let p = l.local_addr().unwrap().port();
                            data = Some(l);
                            if cmd.eq_ignore_ascii_case("EPSV") {
                                format!("229 Entering Extended Passive Mode (|||{p}|)")
                            } else {
                                // an address that is not the server's: the client keeps to the server
                                format!("227 Entering Passive Mode (10,9,8,7,{},{})", p / 256, p % 256)
                            }
                        }
                        "RETR" => {
                            let Some(l) = data.take() else {
                                let _ = w.write_all(b"425 no data connection\r\n").await;
                                continue;
                            };
                            s.retrs.fetch_add(1, Ordering::Relaxed);
                            let _ = w.write_all(b"150 here it comes\r\n").await;
                            let (mut d, _) = l.accept().await.unwrap();
                            let from = rest.min(s.data.len());
                            rest = 0;
                            let mut gone = false;
                            for c in s.data[from..].chunks(32 * 1024) {
                                if d.write_all(c).await.is_err() {
                                    gone = true;
                                    break;
                                }
                                tokio::time::sleep(Duration::from_millis(1)).await;
                            }
                            drop(d);
                            if gone { "426 transfer aborted".into() } else { "226 done".into() }
                        }
                        "QUIT" => {
                            let _ = w.write_all(b"221 bye\r\n").await;
                            return;
                        }
                        _ => "502 not here".into(),
                    };
                    if w.write_all(format!("{answer}\r\n").as_bytes()).await.is_err() {
                        return;
                    }
                }
            });
        }
    });
    port
}

fn server(pasv_only: bool) -> Arc<Server> {
    Arc::new(Server {
        data: (0..3 * 1024 * 1024 + 11).map(|i| (i * 7 % 253) as u8).collect(),
        user: "me",
        pass: "secret",
        pasv_only,
        retrs: AtomicU32::new(0),
    })
}

fn shared() -> Arc<Shared> {
    Arc::new(Shared::new(build_client_with(&ProxyConfig::default()).unwrap(), SpeedLimiter::new(0)))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_file_comes_in_parts_and_goes_on_after_a_pause() {
    for pasv_only in [false, true] {
        let s = server(pasv_only);
        let port = serve(s.clone()).await;
        let url = format!("ftp://127.0.0.1:{port}/pub/f.iso");
        let auth = Some(("me".to_string(), "secret".to_string()));
        let look = ftp::look(&url, auth.as_ref()).await.unwrap();
        assert_eq!(look.size, Some(s.data.len() as u64));
        assert!(look.rest);
        assert!(look.modified.is_some());
        let dir = std::env::temp_dir().join(format!("ezoo-ftp-{port}"));
        std::fs::create_dir_all(&dir).unwrap();
        let part = dir.join("f.part");
        let job = FtpJob {
            url: url.clone(),
            auth,
            path: look.path,
            total_size: look.size,
            rest: true,
            part_path: part.clone(),
            connections: 4,
            min_split: 64 * 1024,
            retries: 2,
            limiter: Arc::new(SpeedLimiter::new(0)),
        };
        let segs = Arc::new(Mutex::new(Vec::new()));
        let progress = Arc::new(Progress::default());
        let cancel = CancellationToken::new();
        let (c2, p2, quarter) = (cancel.clone(), progress.clone(), s.data.len() as u64 / 4);
        tokio::spawn(async move {
            while p2.bytes.load(Ordering::Relaxed) < quarter {
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
            c2.cancel();
        });
        assert!(ftp::run(shared(), job.clone(), segs.clone(), progress, cancel).await.is_err());
        ftp::run(shared(), job, segs, Arc::new(Progress::default()), CancellationToken::new()).await.unwrap();
        assert!(std::fs::read(&part).unwrap() == s.data);
        assert!(s.retrs.load(Ordering::Relaxed) >= 4, "in parts");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_wrong_login_and_a_missing_file_are_told() {
    let s = server(false);
    let port = serve(s).await;
    let e = ftp::look(&format!("ftp://127.0.0.1:{port}/pub/f.iso"), Some(&("me".into(), "wrong".into()))).await.unwrap_err();
    assert!(format!("{e:#}").contains("user name or password"), "{e:#}");
    let e = ftp::look(&format!("ftp://me:secret@127.0.0.1:{port}/pub/none.iso"), None).await.unwrap_err();
    assert!(format!("{e:#}").contains("not found"), "{e:#}");
}

/// Through the list: an address with its login in it is kept without it
/// (the login is kept apart), and the file comes whole.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_ftp_download_from_the_list() {
    let s = server(false);
    let port = serve(s.clone()).await;
    let root = std::env::temp_dir().join(format!("ezoo-ftp-list-{port}"));
    let _ = std::fs::remove_dir_all(&root);
    let dir = root.join("out");
    let mgr = swiftget_engine::Manager::new(tokio::runtime::Handle::current(), root.join("app")).unwrap();
    let id = mgr.add(swiftget_engine::NewDownload {
        url: format!("ftp://me:secret@127.0.0.1:{port}/pub/f.iso"),
        save_dir: Some(dir.clone()),
        start: true,
        ..Default::default()
    });
    let mut v = mgr.get(id).unwrap();
    assert!(!v.url.contains("secret"), "{}", v.url);
    for _ in 0..1500 {
        v = mgr.get(id).unwrap();
        if matches!(v.status, swiftget_engine::Status::Completed | swiftget_engine::Status::Failed) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(v.status, swiftget_engine::Status::Completed, "{:?}", v.error);
    assert_eq!(v.file_name, "f.iso");
    assert!(std::fs::read(&v.save_path).unwrap() == s.data);
    let _ = std::fs::remove_dir_all(&root);
}
