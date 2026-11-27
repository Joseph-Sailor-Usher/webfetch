//! webfetch: fetch a URL and print its readable text, or search the web and print the results.

use ego_tree::NodeRef;
use scraper::{Html, Node, Selector};
use std::time::Duration;

/// The longest `--raw` output, in bytes (cut back to a character boundary).
pub const RAW_LIMIT: usize = 50_000;

/// A failure the CLI reports on stderr before exiting with status 1.
#[derive(Debug)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

pub fn client() -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .user_agent("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/125.0 Safari/537.36")
        .redirect(reqwest::redirect::Policy::limited(5))
        .timeout(Duration::from_secs(15))
        .build()
        .expect("failed to build HTTP client")
}

/// GET `url` and return its body. A status outside 2xx is an error, not a page.
pub fn fetch(client: &reqwest::blocking::Client, url: &str) -> Result<String> {
    let resp = client.get(url).send().map_err(|e| Error(format!("error fetching {url}: {e}")))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(Error(format!("HTTP {status} from {url}")));
    }
    resp.text().map_err(|e| Error(format!("error reading body of {url}: {e}")))
}

/// The first `limit` bytes of `s`, cut back to the nearest character boundary.
pub fn truncate_chars(s: &str, limit: usize) -> &str {
    if s.len() <= limit {
        return s;
    }
    let mut end = limit;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// Elements whose content is not readable text: code, styling, navigation and forms.
const SKIP: &[&str] = &[
    "script", "style", "noscript", "template", "head", "nav", "form", "svg", "iframe",
    "canvas", "button", "select", "option", "textarea",
];

/// The page's own header and footer are chrome; inside `main` or `article` they are content
/// (an article's title block, a hero headline), so they are kept there.
const CHROME: &[&str] = &["header", "footer"];
const CONTENT: &[&str] = &["main", "article"];

/// Elements that start a new line; everything else is inline and flows into the current line.
const BLOCK: &[&str] = &[
    "address", "article", "aside", "blockquote", "br", "dd", "details", "dialog", "div", "dl", "dt",
    "fieldset", "figcaption", "figure", "h1", "h2", "h3", "h4", "h5", "h6", "hr", "li", "main", "ol",
    "p", "pre", "section", "summary", "table", "tbody", "td", "tfoot", "th", "thead", "tr", "ul", "body",
];

/// The readable text of a page: one line per block, inline text kept together, whitespace collapsed,
/// and script, style, navigation, header, footer and form content left out.
pub fn extract_text(doc: &Html) -> String {
    let body = Selector::parse("body").unwrap();
    let root = doc.select(&body).next().map(|b| *b).unwrap_or_else(|| doc.tree.root());
    let mut lines = vec![String::new()];
    walk(root, &mut lines, false);
    lines.into_iter().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect::<Vec<_>>().join("\n")
}

fn walk(node: NodeRef<'_, Node>, lines: &mut Vec<String>, in_content: bool) {
    for child in node.children() {
        match child.value() {
            Node::Text(t) => push_text(lines, t),
            Node::Element(e) => {
                let name = e.name();
                if SKIP.contains(&name) || (!in_content && CHROME.contains(&name)) {
                    continue;
                }
                let block = BLOCK.contains(&name) || CHROME.contains(&name);
                if block {
                    new_line(lines);
                }
                walk(child, lines, in_content || CONTENT.contains(&name));
                if block {
                    new_line(lines);
                }
            }
            _ => {}
        }
    }
}

fn push_text(lines: &mut [String], text: &str) {
    let line = lines.last_mut().unwrap();
    for (i, word) in text.split_whitespace().enumerate() {
        // a space before the word when the text had whitespace there, or between words
        let spaced = i > 0 || text.starts_with(char::is_whitespace);
        if spaced && !line.is_empty() && !line.ends_with(' ') {
            line.push(' ');
        }
        line.push_str(word);
    }
    if text.ends_with(char::is_whitespace) && !line.is_empty() && !line.ends_with(' ') {
        line.push(' ');
    }
}

fn new_line(lines: &mut Vec<String>) {
    if !lines.last().unwrap().trim().is_empty() {
        lines.push(String::new());
    }
}

/// One search result.
#[derive(Debug, PartialEq)]
pub struct Hit {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

pub fn format_hits(hits: &[Hit]) -> String {
    hits.iter()
        .enumerate()
        .map(|(i, h)| format!("[{}] {}\n    {}\n    {}", i + 1, h.title, h.url, h.snippet))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Search through a SearXNG instance's JSON API (`/search?format=json`).
pub fn search_searxng(client: &reqwest::blocking::Client, base: &str, query: &str) -> Result<Vec<Hit>> {
    let url = format!("{}/search?q={}&format=json", base.trim_end_matches('/'), urlencoding::encode(query));
    parse_searxng(&fetch(client, &url)?)
}

pub fn parse_searxng(body: &str) -> Result<Vec<Hit>> {
    let v: serde_json::Value =
        serde_json::from_str(body).map_err(|e| Error(format!("SearXNG did not answer JSON ({e}); is format=json enabled?")))?;
    let results = v.get("results").and_then(|r| r.as_array()).cloned().unwrap_or_default();
    let field = |r: &serde_json::Value, k: &str| r.get(k).and_then(|x| x.as_str()).unwrap_or("").trim().to_string();
    Ok(results
        .iter()
        .map(|r| Hit { title: field(r, "title"), url: field(r, "url"), snippet: field(r, "content") })
        .filter(|h| !h.url.is_empty())
        .collect())
}

/// Search DuckDuckGo's HTML endpoint (the fallback when no SearXNG answers).
pub fn search_duckduckgo(client: &reqwest::blocking::Client, query: &str) -> Result<Vec<Hit>> {
    let url = format!("https://html.duckduckgo.com/html/?q={}", urlencoding::encode(query));
    parse_duckduckgo(&fetch(client, &url)?)
}

pub fn parse_duckduckgo(body: &str) -> Result<Vec<Hit>> {
    let doc = Html::parse_document(body);
    if is_ddg_challenge(&doc) {
        return Err(Error(
            "DuckDuckGo answered with its bot challenge instead of results; use a SearXNG instance (--searx or WEBFETCH_SEARXNG)".into(),
        ));
    }
    let result = Selector::parse(".result").unwrap();
    let title = Selector::parse(".result__a").unwrap();
    let snippet = Selector::parse(".result__snippet").unwrap();
    let text = |e: scraper::ElementRef| e.text().collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ");
    Ok(doc
        .select(&result)
        .filter(|r| !r.value().classes().any(|c| c == "result--ad"))
        .filter_map(|r| {
            let a = r.select(&title).next()?;
            let url = unwrap_ddg_link(a.value().attr("href")?);
            Some(Hit { title: text(a), url, snippet: r.select(&snippet).next().map(text).unwrap_or_default() })
        })
        .filter(|h| !h.url.is_empty())
        .collect())
}

fn is_ddg_challenge(doc: &Html) -> bool {
    let anomaly = Selector::parse(".anomaly-modal, #challenge-form, form[action*='anomaly']").unwrap();
    doc.select(&anomaly).next().is_some() || doc.root_element().text().any(|t| t.contains("bots use DuckDuckGo too"))
}

/// DuckDuckGo wraps result links as `//duckduckgo.com/l/?uddg=<encoded target>&rut=...`; return the target.
pub fn unwrap_ddg_link(href: &str) -> String {
    let full = if href.starts_with("//") { format!("https:{href}") } else { href.to_string() };
    if let Some(query) = full.split_once("/l/?").map(|(_, q)| q) {
        for pair in query.split('&') {
            if let Some(v) = pair.strip_prefix("uddg=") {
                if let Ok(target) = urlencoding::decode(v) {
                    return target.into_owned();
                }
            }
        }
    }
    full
}

/// Search: SearXNG when one answers, else DuckDuckGo.
pub fn search(client: &reqwest::blocking::Client, searx: Option<&str>, query: &str) -> Result<Vec<Hit>> {
    if let Some(base) = searx {
        match search_searxng(client, base, query) {
            Ok(hits) => return Ok(hits),
            Err(e) => eprintln!("webfetch: SearXNG at {base} failed ({e}); trying DuckDuckGo"),
        }
    }
    search_duckduckgo(client, query)
}

#[cfg(test)]
mod tests {
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
}
