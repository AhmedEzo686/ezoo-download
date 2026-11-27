//! A web server that goes wrong on purpose, to try Ezoo Download against it:
//!
//! cargo run -p swiftget-engine --example fault_server -- [what...]
//!   cut      every answer is cut after 200 KB (the first 20)
//!   noranges ranges are ignored
//!   two      more than 2 connections at once: 403
//!   busy     the first 3 requests: 503
//!   slow     1 MB/s
//!   robot    a "not a robot" check
//!
//! It serves a 50 MB file at the address it prints, until Ctrl+C.

#[path = "../tests/support/faults.rs"]
mod faults;

#[tokio::main]
async fn main() {
    let mut f = faults::Faults::default();
    for a in std::env::args().skip(1) {
        match a.as_str() {
            "cut" => {
                f.cut_after = Some(200 * 1024);
                f.cuts = Some(20);
            }
            "noranges" => f.no_ranges = true,
            "two" => f.max_connections = Some(2),
            "busy" => f.busy_first = 3,
            "slow" => f.rate = 1 << 20,
            "robot" => f.robot_check = true,
            other => eprintln!("unknown: {other}"),
        }
    }
    let srv = faults::start(faults::file_data(50 << 20, 7), f).await;
    println!("{}", srv.url("test-50mb.bin"));
    std::future::pending::<()>().await;
}
