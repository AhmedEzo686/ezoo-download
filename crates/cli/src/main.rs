//! `ezoo` — drive the running Ezoo Download from a terminal or from an AI
//! assistant (MCP).
//!
//!     ezoo add https://example.com/file.iso
//!     ezoo list
//!     ezoo pause 12          ezoo pause --all
//!     ezoo resume 12         ezoo resume --all
//!     ezoo remove 12 [--delete-file]
//!     ezoo status
//!     ezoo mcp               (JSON-RPC over stdin/stdout)
//!
//! It talks to the program's local API, signing every request with the key
//! the program puts in its extension folder (token.json), so only programs
//! running as this user can use it.

use anyhow::{anyhow, bail, Context, Result};
use std::io::{BufRead, Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::time::Duration;

fn data_dir() -> PathBuf {
    swiftget_engine::default_data_dir()
}

fn port() -> u16 {
    let s = swiftget_engine::Settings::load(&data_dir().join("settings.json"));
    if s.api_port >= 1024 {
        s.api_port
    } else {
        9614
    }
}

/// The key the program gave its add-ons (and every program of this user
/// reads it there), plus the port to ask.
fn token() -> Result<(String, u16)> {
    for dir in ["extension-firefox", "extension"] {
        let path = data_dir().join(dir).join("token.json");
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        let v: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
        if let Some(key) = v["token"].as_str().filter(|k| !k.is_empty()) {
            let p = v["port"].as_u64().filter(|p| *p >= 1024).map(|p| p as u16).unwrap_or_else(port);
            return Ok((key.to_string(), p));
        }
    }
    bail!("Ezoo Download is not running: start it once (the key is written next to its add-ins)")
}

fn call(method: &str, path: &str, body: Option<serde_json::Value>) -> Result<serde_json::Value> {
    let (key, port) = token()?;
    let time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
        .to_string();
    let nonce = swiftget_engine::secret::random_nonce();
    let sig = swiftget_engine::secret::request_signature(&key, method, path, &time, &nonce);
    let body = body.map(|b| b.to_string()).unwrap_or_default();

    let mut stream = TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_millis(800))
        .with_context(|| format!("Ezoo Download is not answering on port {port}"))?;
    stream.set_read_timeout(Some(Duration::from_secs(15)))?;
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nX-SwiftGet: 1\r\n\
         X-Ezoo-Time: {time}\r\nX-Ezoo-Nonce: {nonce}\r\nX-Ezoo-Sig: {sig}\r\n\
         Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes())?;
    let mut raw = String::new();
    stream.read_to_string(&mut raw)?;
    let (head, body) = raw.split_once("\r\n\r\n").ok_or_else(|| anyhow!("unexpected answer from the program"))?;
    let status = head.lines().next().unwrap_or("").to_string();
    if !status.contains(" 200") {
        bail!("the program refused it: {status} {body}");
    }
    serde_json::from_str(body).context("unexpected JSON from the program")
}

fn cmd_add(url: &str) -> Result<()> {
    let r = call("POST", "/add", Some(serde_json::json!({ "url": url, "quiet": true })))?;
    if r["ok"].as_bool() != Some(true) {
        bail!("the program did not take the link: {r}");
    }
    println!("added: {url}");
    Ok(())
}

fn cmd_list(json: bool) -> Result<()> {
    let r = call("GET", "/v1/list", None)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&r["downloads"])?);
        return Ok(());
    }
    let empty = Vec::new();
    let items = r["downloads"].as_array().unwrap_or(&empty);
    if items.is_empty() {
        println!("no downloads");
        return Ok(());
    }
    println!("{:>5}  {:<18} {:>9}  {}", "id", "state", "size", "name");
    for d in items {
        let size = d["size"].as_u64().unwrap_or(0);
        let size = if size >= 1 << 30 {
            format!("{:.1} GB", size as f64 / (1u64 << 30) as f64)
        } else if size >= 1 << 20 {
            format!("{:.1} MB", size as f64 / (1u64 << 20) as f64)
        } else if size >= 1 << 10 {
            format!("{:.0} KB", size as f64 / 1024.0)
        } else if size > 0 {
            format!("{size} B")
        } else {
            "?".into()
        };
        println!(
            "{:>5}  {:<18} {:>9}  {}",
            d["id"],
            d["status"].as_str().unwrap_or("?"),
            size,
            d["name"].as_str().unwrap_or("?")
        );
    }
    Ok(())
}

fn cmd_status(json: bool) -> Result<()> {
    let r = call("GET", "/v1/status", None)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&r)?);
    } else {
        println!(
            "Ezoo Download {}: {} downloads ({} active)",
            r["version"].as_str().unwrap_or("?"),
            r["downloads"].as_u64().unwrap_or(0),
            r["active"].as_u64().unwrap_or(0)
        );
    }
    Ok(())
}

fn cmd_id_action(path: &str, id: Option<u64>, all: bool) -> Result<()> {
    let body = if all {
        serde_json::json!({ "all": true })
    } else if let Some(id) = id {
        serde_json::json!({ "id": id })
    } else {
        bail!("give an id, or --all");
    };
    call("POST", path, Some(body))?;
    println!("ok");
    Ok(())
}

fn cmd_remove(id: u64, delete_file: bool) -> Result<()> {
    call("POST", "/v1/remove", Some(serde_json::json!({ "id": id, "deleteFile": delete_file })))?;
    println!("removed {id}");
    Ok(())
}

fn cmd_debrid(args: &[String]) -> Result<()> {
    match args.first().map(String::as_str) {
        None | Some("status") => {
            let r = call("GET", "/v1/debrid", None)?;
            let empty = Vec::new();
            let accounts: Vec<&str> = r["accounts"]
                .as_array()
                .unwrap_or(&empty)
                .iter()
                .filter_map(|x| x.as_str())
                .collect();
            let provider = r["provider"].as_str().unwrap_or("");
            if r["configured"].as_bool() == Some(true) {
                println!("{} is the provider in use", provider);
            } else if provider.is_empty() {
                println!("no provider in use");
            } else {
                println!("{provider} is in use, but has no token");
            }
            println!("accounts: {}", if accounts.is_empty() { "none".to_string() } else { accounts.join(", ") });
            println!("providers: {}", r["providers"]);
            Ok(())
        }
        Some("set") => {
            let provider = args.get(1).context("usage: ezoo debrid set realdebrid <token>")?;
            let token = args.get(2).context("give the token")?;
            let r = call(
                "POST",
                "/v1/debrid",
                Some(serde_json::json!({ "provider": provider, "token": token })),
            )?;
            println!("connected: {}", r["user"].as_str().unwrap_or("account"));
            Ok(())
        }
        Some("off") => {
            // with a provider name: that one; without: the one in use
            let provider = args.get(1).map(String::as_str).unwrap_or("");
            call("POST", "/v1/debrid", Some(serde_json::json!({ "provider": provider, "token": "" })))?;
            println!("debrid account removed");
            Ok(())
        }
        other => bail!("unknown debrid command: {}", other.unwrap_or("")),
    }
}

// ---------------------------------------------------------------- MCP

fn mcp_tools() -> serde_json::Value {
    serde_json::json!([
        {
            "name": "add_download",
            "description": "Hand a link (or a magnet) to Ezoo Download and start it.",
            "inputSchema": {
                "type": "object",
                "properties": { "url": { "type": "string", "description": "the link to download" } },
                "required": ["url"]
            }
        },
        {
            "name": "list_downloads",
            "description": "The downloads of Ezoo Download with their state and size.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "pause_download",
            "description": "Pause one download (by id) or all of them (all = true).",
            "inputSchema": {
                "type": "object",
                "properties": { "id": { "type": "integer" }, "all": { "type": "boolean" } }
            }
        },
        {
            "name": "resume_download",
            "description": "Continue one download (by id) or all of them (all = true).",
            "inputSchema": {
                "type": "object",
                "properties": { "id": { "type": "integer" }, "all": { "type": "boolean" } }
            }
        },
        {
            "name": "remove_download",
            "description": "Remove a download from the list; delete_file also removes the file on disk.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "integer" },
                    "delete_file": { "type": "boolean" }
                },
                "required": ["id"]
            }
        }
    ])
}

fn mcp_call(name: &str, args: &serde_json::Value) -> Result<String> {
    match name {
        "add_download" => {
            let url = args["url"].as_str().context("url is missing")?;
            call("POST", "/add", Some(serde_json::json!({ "url": url, "quiet": true })))?;
            Ok(format!("added {url}"))
        }
        "list_downloads" => {
            let r = call("GET", "/v1/list", None)?;
            Ok(serde_json::to_string_pretty(&r["downloads"])?)
        }
        "pause_download" => {
            let body = if args["all"].as_bool() == Some(true) {
                serde_json::json!({ "all": true })
            } else {
                serde_json::json!({ "id": args["id"].as_u64().context("id is missing")? })
            };
            call("POST", "/v1/pause", Some(body))?;
            Ok("paused".into())
        }
        "resume_download" => {
            let body = if args["all"].as_bool() == Some(true) {
                serde_json::json!({ "all": true })
            } else {
                serde_json::json!({ "id": args["id"].as_u64().context("id is missing")? })
            };
            call("POST", "/v1/resume", Some(body))?;
            Ok("resumed".into())
        }
        "remove_download" => {
            let id = args["id"].as_u64().context("id is missing")?;
            let delete = args["delete_file"].as_bool().unwrap_or(false);
            call("POST", "/v1/remove", Some(serde_json::json!({ "id": id, "deleteFile": delete })))?;
            Ok(format!("removed {id}"))
        }
        other => bail!("unknown tool: {other}"),
    }
}

fn mcp() -> Result<()> {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let Ok(msg) = serde_json::from_str::<serde_json::Value>(&line) else { continue };
        let id = msg.get("id").cloned();
        let method = msg["method"].as_str().unwrap_or("");
        // notifications have no id and take no answer
        if id.is_none() {
            continue;
        }
        let answer = match method {
            "initialize" => Ok(serde_json::json!({
                "protocolVersion": "2024-11-05",
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "ezoo", "version": env!("CARGO_PKG_VERSION") }
            })),
            "tools/list" => Ok(serde_json::json!({ "tools": mcp_tools() })),
            "tools/call" => {
                let name = msg["params"]["name"].as_str().unwrap_or("");
                let args = &msg["params"]["arguments"];
                match mcp_call(name, args) {
                    Ok(text) => Ok(serde_json::json!({
                        "content": [ { "type": "text", "text": text } ],
                        "isError": false
                    })),
                    Err(e) => Ok(serde_json::json!({
                        "content": [ { "type": "text", "text": format!("{e:#}") } ],
                        "isError": true
                    })),
                }
            }
            "ping" => Ok(serde_json::json!({})),
            other => Err(anyhow!("unknown method: {other}")),
        };
        let response = match answer {
            Ok(result) => serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            Err(e) => serde_json::json!({
                "jsonrpc": "2.0", "id": id,
                "error": { "code": -32601, "message": format!("{e:#}") }
            }),
        };
        writeln!(stdout, "{response}")?;
        stdout.flush()?;
    }
    Ok(())
}

fn usage() -> ! {
    eprintln!(
        "ezoo — Ezoo Download from the terminal

  ezoo add <url>
  ezoo list [--json]
  ezoo pause <id> | --all
  ezoo resume <id> | --all
  ezoo remove <id> [--delete-file]
  ezoo status [--json]
  ezoo debrid status | set realdebrid <token> | off
  ezoo mcp            (for Claude, Cursor, VS Code: MCP over stdin/stdout)"
    );
    std::process::exit(2);
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(cmd) = args.first().map(String::as_str) else { usage() };
    let rest = &args[1..];
    match cmd {
        "add" => {
            let url = rest.iter().find(|a| !a.starts_with("--")).context("give a link")?;
            cmd_add(url)
        }
        "list" => cmd_list(rest.iter().any(|a| a == "--json")),
        "status" => cmd_status(rest.iter().any(|a| a == "--json")),
        "debrid" => cmd_debrid(rest),
        "pause" => {
            let all = rest.iter().any(|a| a == "--all");
            let id = rest.iter().find_map(|a| a.parse::<u64>().ok());
            cmd_id_action("/v1/pause", id, all)
        }
        "resume" => {
            let all = rest.iter().any(|a| a == "--all");
            let id = rest.iter().find_map(|a| a.parse::<u64>().ok());
            cmd_id_action("/v1/resume", id, all)
        }
        "remove" => {
            let id = rest.iter().find_map(|a| a.parse::<u64>().ok()).context("give an id")?;
            cmd_remove(id, rest.iter().any(|a| a == "--delete-file"))
        }
        "mcp" => mcp(),
        "-h" | "--help" | "help" => {
            usage();
        }
        other => {
            eprintln!("unknown command: {other}");
            usage();
        }
    }
}
