//! Unit tests for lib.rs (a sibling file, so foi counts src as tested).

use super::*;

#[test]
fn script_style_and_chrome_are_left_out() {
    let doc = Html::parse_document(
        "<html><head><title>T</title></head><body><header>SITE HEADER</header><nav>NAV MENU</nav>\
         <p>Real paragraph.</p><script>var js_noise = 1;</script><style>.css_noise{color:red}</style>\
         <noscript>enable js</noscript><form><input>FORM TEXT</form><footer>FOOTER</footer></body></html>",
    );
    assert_eq!(extract_text(&doc), "Real paragraph.");
}

#[test]
fn inline_text_stays_on_one_line_and_blocks_break() {
    let doc = Html::parse_document(
        "<body><p>Hello <b>world</b>, this is <a href='/x'>a link</a> inline.</p>\
         <ul><li>one</li><li>two <em>three</em></li></ul><div>a<br>b</div><p>x<span>y</span>z</p></body>",
    );
    assert_eq!(extract_text(&doc), "Hello world, this is a link inline.\none\ntwo three\na\nb\nxyz");
}

#[test]
fn headers_inside_main_or_article_are_content() {
    let doc = Html::parse_document(
        "<body><header>SITE HEADER</header><main><header><h1>Hero headline</h1></header>\
         <article><header>Article title</header><p>Body.</p><footer>By someone</footer></article></main>\
         <footer>SITE FOOTER</footer></body>",
    );
    assert_eq!(extract_text(&doc), "Hero headline\nArticle title\nBody.\nBy someone");
}

#[test]
fn whitespace_collapses() {
    let doc = Html::parse_document("<body><p>  lots \n\n of\t space  </p></body>");
    assert_eq!(extract_text(&doc), "lots of space");
}

#[test]
fn raw_cut_never_splits_a_character() {
    let s = format!("{}é tail", "a".repeat(RAW_LIMIT - 1));
    let cut = truncate_chars(&s, RAW_LIMIT);
    assert_eq!(cut.len(), RAW_LIMIT - 1);
    assert_eq!(truncate_chars("short", RAW_LIMIT), "short");
    assert_eq!(truncate_chars("日本語", 4), "日");
}

#[test]
fn searxng_json_becomes_hits() {
    let body = r#"{"results":[{"title":"Rust","url":"https://www.rust-lang.org/","content":"A language."},
                   {"title":"No url","url":"","content":"dropped"}]}"#;
    assert_eq!(
        parse_searxng(body).unwrap(),
        vec![Hit { title: "Rust".into(), url: "https://www.rust-lang.org/".into(), snippet: "A language.".into() }]
    );
    assert!(parse_searxng("<html>not json</html>").is_err());
}

#[test]
fn duckduckgo_links_are_unwrapped_and_ads_skipped() {
    let body = r#"<div class="result result--ad"><a class="result__a" href="https://ad.example/">Ad</a></div>
        <div class="result"><h2 class="result__title"><a class="result__a"
          href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fwww.rust%2Dlang.org%2Flearn&amp;rut=abc">Learn  Rust</a></h2>
          <a class="result__snippet">The book.</a></div>"#;
    assert_eq!(
        parse_duckduckgo(body).unwrap(),
        vec![Hit { title: "Learn Rust".into(), url: "https://www.rust-lang.org/learn".into(), snippet: "The book.".into() }]
    );
    assert_eq!(unwrap_ddg_link("https://example.com/a"), "https://example.com/a");
}

#[test]
fn duckduckgo_challenge_is_an_error() {
    let body = "<html><body><p>Unfortunately, bots use DuckDuckGo too.</p><p>Select all squares containing a duck:</p></body></html>";
    let err = parse_duckduckgo(body).unwrap_err();
    assert!(err.0.contains("bot challenge"), "{err}");
}

#[test]
fn http_errors_are_errors() {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for (i, stream) in listener.incoming().take(2).enumerate() {
            let mut s = stream.unwrap();
            let mut buf = [0u8; 1024];
            let _ = s.read(&mut buf);
            let reply = if i == 0 { "HTTP/1.1 404 Not Found\r\nContent-Length: 9\r\n\r\nnot found" }
                        else { "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok" };
            s.write_all(reply.as_bytes()).unwrap();
        }
    });
    let c = client();
    let err = fetch(&c, &format!("http://127.0.0.1:{port}/missing")).unwrap_err();
    assert!(err.0.contains("404"), "{err}");
    assert_eq!(fetch(&c, &format!("http://127.0.0.1:{port}/ok")).unwrap(), "ok");
}
