//! The webfetch binary end to end, against a small HTTP server in the test (no network).

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::process::{Command, Output};

/// Serve `routes` (path -> (status line, content type, body)) on a free port; unknown paths are 404.
fn serve(routes: Vec<(&'static str, &'static str, &'static str, String)>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let mut s = stream.unwrap();
            let mut first = String::new();
            let mut reader = BufReader::new(s.try_clone().unwrap());
            reader.read_line(&mut first).unwrap();
            let mut line = String::new();
            while reader.read_line(&mut line).unwrap() > 2 {
                line.clear();
            }
            let path = first.split_whitespace().nth(1).unwrap_or("/").to_string();
            let (status, ctype, body) = routes
                .iter()
                .find(|(p, ..)| path == *p || path.starts_with(&format!("{p}?")))
                .map(|(_, s, c, b)| (*s, *c, b.clone()))
                .unwrap_or(("404 Not Found", "text/plain", "not found".to_string()));
            let reply = format!(
                "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = s.write_all(reply.as_bytes());
        }
    });
    base
}

fn webfetch(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_webfetch")).args(args).env_remove("WEBFETCH_SEARXNG").output().unwrap()
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

const PAGE: &str = "<html><head><style>h1{}</style></head><body><nav>MENU</nav><main><h1>Title</h1>\
    <p>Hello <b>world</b>.</p><script>var noise = 1;</script></main><footer>FOOT</footer></body></html>";

#[test]
fn neither_url_nor_search_is_a_usage_error() {
    let o = webfetch(&[]);
    assert_eq!(o.status.code(), Some(2), "{}", stderr(&o));
    assert!(stderr(&o).contains("required"), "{}", stderr(&o));
}

#[test]
fn raw_and_search_together_is_a_usage_error() {
    let o = webfetch(&["--raw", "--search", "x"]);
    assert_eq!(o.status.code(), Some(2), "{}", stderr(&o));
}

#[test]
fn a_page_prints_its_readable_text() {
    let base = serve(vec![("/page", "200 OK", "text/html; charset=utf-8", PAGE.to_string())]);
    let o = webfetch(&[&format!("{base}/page")]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert_eq!(stdout(&o), "Title\nHello world.\n");
}

#[test]
fn an_http_error_exits_1_with_the_status() {
    let base = serve(vec![]);
    let o = webfetch(&[&format!("{base}/missing")]);
    assert_eq!(o.status.code(), Some(1));
    assert!(stderr(&o).contains("HTTP 404"), "{}", stderr(&o));
    assert_eq!(stdout(&o), "");
}

#[test]
fn raw_is_cut_on_a_character_boundary() {
    let body = format!("{}é and more", "a".repeat(49_999));
    let base = serve(vec![("/raw", "200 OK", "text/html; charset=utf-8", body)]);
    let o = webfetch(&["--raw", &format!("{base}/raw")]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert_eq!(stdout(&o), format!("{}\n", "a".repeat(49_999)));
}

#[test]
fn lines_limits_the_output() {
    let body = format!("<body>{}</body>", (1..=5).map(|i| format!("<p>line {i}</p>")).collect::<String>());
    let base = serve(vec![("/many", "200 OK", "text/html", body)]);
    let o = webfetch(&["-l", "2", &format!("{base}/many")]);
    assert_eq!(stdout(&o), "line 1\nline 2\n... (3 more lines)\n");
}

#[test]
fn search_reads_searxng_json() {
    let json = r#"{"results":[{"title":"Rust","url":"https://www.rust-lang.org/","content":"A language."},
                   {"title":"Book","url":"https://doc.rust-lang.org/book/","content":"The book."}]}"#;
    let base = serve(vec![("/search", "200 OK", "application/json", json.to_string())]);
    let o = webfetch(&["--searx", &base, "--search", "rust"]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert_eq!(
        stdout(&o),
        "[1] Rust\n    https://www.rust-lang.org/\n    A language.\n[2] Book\n    https://doc.rust-lang.org/book/\n    The book.\n"
    );
}

#[test]
fn a_search_with_no_results_exits_1() {
    let base = serve(vec![("/search", "200 OK", "application/json", r#"{"results":[]}"#.to_string())]);
    let o = webfetch(&["--searx", &base, "--search", "nothing at all"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(stderr(&o).contains("no results"), "{}", stderr(&o));
}
