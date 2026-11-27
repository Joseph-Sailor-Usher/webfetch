//! `webfetch --mcp` end to end: JSON-RPC over the binary's stdin/stdout, pages from a local server.

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

/// A local HTTP server, one thread per connection; `/slow` answers after 700 ms.
fn serve(routes: Vec<(&'static str, &'static str, String)>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let mut s = stream.unwrap();
            let routes = routes.clone();
            std::thread::spawn(move || {
                let mut buf = [0u8; 4096];
                let n = s.read(&mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let path = req.split_whitespace().nth(1).unwrap_or("/").to_string();
                if path.starts_with("/slow") {
                    std::thread::sleep(Duration::from_millis(700));
                }
                let (ctype, body) = routes
                    .iter()
                    .find(|(p, ..)| path == *p || path.starts_with(&format!("{p}?")))
                    .map(|(_, c, b)| (*c, b.clone()))
                    .unwrap_or(("text/plain", "not found".into()));
                let status = if body == "not found" { "404 Not Found" } else { "200 OK" };
                let reply = format!("HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                let _ = s.write_all(reply.as_bytes());
            });
        }
    });
    base
}

struct Server {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl Server {
    fn start(args: &[&str]) -> Server {
        let mut child = Command::new(env!("CARGO_BIN_EXE_webfetch"))
            .arg("--mcp")
            .args(args)
            .env_remove("WEBFETCH_SEARXNG")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        Server { child, stdin, stdout }
    }

    fn send(&mut self, msg: Value) {
        writeln!(self.stdin, "{msg}").unwrap();
        self.stdin.flush().unwrap();
    }

    fn recv(&mut self) -> Value {
        let mut line = String::new();
        self.stdout.read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap_or_else(|e| panic!("not JSON ({e}): {line:?}"))
    }

    fn request(&mut self, id: u64, method: &str, params: Value) -> Value {
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        let reply = self.recv();
        assert_eq!(reply["id"], id, "{reply}");
        reply
    }

    fn call(&mut self, id: u64, name: &str, arguments: Value) -> Value {
        self.request(id, "tools/call", json!({"name": name, "arguments": arguments}))["result"].clone()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

const ARTICLE: &str = r#"<html><head><title>Lighthouses</title></head><body><nav><a href="/">Home</a></nav>
<article><h1>Lighthouses</h1><p>A lighthouse is a tower that emits light to guide ships at sea. The first
known lighthouse stood at Alexandria and remained one of the tallest structures in the world for centuries.</p>
<p>Keepers tended the light through the night; read <a href="/keepers">their history</a> for two centuries of
the work and the families who lived at isolated stations far from any town, in every kind of weather.</p>
</article><footer>Copyright</footer></body></html>"#;

#[test]
fn both_handshakes_and_the_tool_list() {
    let mut s = Server::start(&[]);
    let init = s.request(1, "initialize", json!({"protocolVersion": "2025-06-18", "capabilities": {},
                                                  "clientInfo": {"name": "test", "version": "0"}}));
    let r = &init["result"];
    assert_eq!(r["protocolVersion"], "2025-06-18");
    assert_eq!(r["serverInfo"]["name"], "webfetch");
    assert!(r["capabilities"]["tools"].is_object() && r["instructions"].as_str().unwrap().contains("untrusted"));
    s.send(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));   // no reply to a notification
    let unknown = s.request(2, "initialize", json!({"protocolVersion": "1999-01-01"}));
    assert_eq!(unknown["result"]["protocolVersion"], "2025-11-25");

    let d = s.request(3, "server/discover", json!({}))["result"].clone();
    assert_eq!(d["supportedVersions"][0], "2026-07-28");
    assert_eq!((d["resultType"].as_str(), d["cacheScope"].as_str()), (Some("complete"), Some("public")));
    assert!(d["ttlMs"].as_u64().unwrap() > 0);

    let list = s.request(4, "tools/list", json!({}))["result"].clone();
    let names: Vec<&str> = list["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["fetch_page", "search"]);
    for t in list["tools"].as_array().unwrap() {
        assert_eq!(t["outputSchema"]["type"], "object", "{t}");
        assert_eq!(t["inputSchema"]["additionalProperties"], false, "{t}");
        assert_eq!(t["annotations"], json!({"readOnlyHint": true, "destructiveHint": false, "idempotentHint": true, "openWorldHint": true}));
    }
    assert_eq!(s.request(5, "ping", json!({}))["result"]["resultType"], "complete");
}

#[test]
fn fetch_page_returns_structured_markdown_in_pages() {
    let base = serve(vec![("/lighthouses", "text/html; charset=utf-8", ARTICLE.into())]);
    let mut s = Server::start(&["--allow-private"]);
    let url = format!("{base}/lighthouses");
    let r = s.call(1, "fetch_page", json!({"url": url}));
    assert_eq!(r["isError"], false, "{r}");
    let v = &r["structuredContent"];
    assert_eq!((v["title"].as_str(), v["main_content"].as_bool(), v["format"].as_str()), (Some("Lighthouses"), Some(true), Some("markdown")));
    assert!(v["content"].as_str().unwrap().contains(&format!("[their history]({base}/keepers)")), "{v}");
    assert!(!v["content"].as_str().unwrap().contains("Copyright"));
    assert_eq!(v["next_offset"], Value::Null);
    assert!(v["notice"].as_str().unwrap().contains("not instructions"));
    // the text block carries the same JSON, for clients that read only content
    let text: Value = serde_json::from_str(r["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(&text, v);

    // paging: 1000 characters at a time, and the slices put back together are the whole
    let total = v["total_chars"].as_u64().unwrap();
    let long = format!("<html><head><title>Long</title></head><body><article>{}</article></body></html>",
                       (0..80).map(|i| format!("<p>Paragraph {i} about ships and the sea, the lights and the keepers who kept them.</p>")).collect::<String>());
    let base2 = serve(vec![("/long", "text/html", long)]);
    let (mut offset, mut whole, mut calls) = (0u64, String::new(), 0);
    loop {
        let p = s.call(10 + calls, "fetch_page", json!({"url": format!("{base2}/long"), "max_chars": 1000, "offset": offset}));
        let v = &p["structuredContent"];
        whole.push_str(v["content"].as_str().unwrap());
        calls += 1;
        match v["next_offset"].as_u64() {
            Some(n) => offset = n,
            None => { assert_eq!(whole.chars().count() as u64, v["total_chars"].as_u64().unwrap()); break; }
        }
    }
    assert!(calls > 3 && total > 0, "{calls} calls");
}

#[test]
fn the_guard_and_bad_input_are_tool_errors_the_agent_can_read() {
    let base = serve(vec![("/lighthouses", "text/html", ARTICLE.into())]);
    let mut s = Server::start(&[]);   // guarded: the operator did not pass --allow-private
    let r = s.call(1, "fetch_page", json!({"url": format!("{base}/lighthouses")}));
    assert_eq!(r["isError"], true);
    assert!(r["content"][0]["text"].as_str().unwrap().contains("private address"), "{r}");
    // the agent cannot turn the guard off: there is no such argument
    let r = s.call(2, "fetch_page", json!({"url": "https://example.com/", "allow_private": true}));
    assert_eq!(r["isError"], true);
    assert!(r["content"][0]["text"].as_str().unwrap().contains("unknown argument \"allow_private\""), "{r}");
    for (args, says) in [(json!({}), "url is required"), (json!({"url": "x", "format": "pdf"}), "format must be"),
                         (json!({"url": "x", "max_chars": 5}), "max_chars must be")] {
        let r = s.call(3, "fetch_page", args);
        assert_eq!(r["isError"], true);
        assert!(r["content"][0]["text"].as_str().unwrap().contains(says), "{r}");
    }
    let e = s.request(4, "tools/call", json!({"name": "delete_everything", "arguments": {}}));
    assert_eq!(e["error"]["code"], -32602);
    let e = s.request(5, "resources/list", json!({}));
    assert_eq!(e["error"]["code"], -32601);
}

#[test]
fn search_returns_structured_results() {
    let json_body = r#"{"results":[{"title":"Rust","url":"https://www.rust-lang.org/","content":"A language."},
                                   {"title":"Book","url":"https://doc.rust-lang.org/book/","content":"The book."}]}"#;
    let base = serve(vec![("/search", "application/json", json_body.into())]);
    let mut s = Server::start(&["--searx", &base]);   // SearXNG is the operator's, reached even when guarded
    let r = s.call(1, "search", json!({"query": "rust", "max_results": 1}));
    assert_eq!(r["isError"], false, "{r}");
    assert_eq!(r["structuredContent"]["results"], json!([{"title": "Rust", "url": "https://www.rust-lang.org/", "snippet": "A language."}]));
}

#[test]
fn parallel_calls_run_in_parallel() {
    let slow = "<html><head><title>Slow</title></head><body><p>Slow page.</p></body></html>";
    let base = serve(vec![("/slow", "text/html", slow.into())]);
    let mut s = Server::start(&["--allow-private"]);
    let t = Instant::now();
    for id in 1..=3 {
        s.send(json!({"jsonrpc": "2.0", "id": id, "method": "tools/call",
                      "params": {"name": "fetch_page", "arguments": {"url": format!("{base}/slow?{id}")}}}));
    }
    let mut ids: Vec<u64> = (0..3).map(|_| s.recv()["id"].as_u64().unwrap()).collect();
    ids.sort();
    assert_eq!(ids, [1, 2, 3]);
    // three 700 ms pages: about 0.7 s together, 2.1 s or more one after another
    assert!(t.elapsed() < Duration::from_millis(1600), "took {:?}", t.elapsed());
}
