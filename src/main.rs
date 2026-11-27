use clap::Parser;
use scraper::{Html, Selector};

#[derive(Parser)]
#[command(name = "webfetch")]
#[command(about = "Fetch a URL and extract readable text")]
struct Cli {
    /// URL to fetch (ignored when --search is set)
    #[arg(required = false)]
    url: Option<String>,

    /// Search query (queries DuckDuckGo HTML endpoint)
    #[arg(short, long)]
    search: Option<String>,

    /// Max lines of output (default: 200)
    #[arg(short, long, default_value = "200")]
    lines: usize,

    /// Raw output (don't strip to text)
    #[arg(short, long)]
    raw: bool,
}

fn main() {
    let cli = Cli::parse();
    let client = reqwest::blocking::Client::builder()
        .user_agent("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/125.0 Safari/537.36")
        .redirect(reqwest::redirect::Policy::limited(5))
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .expect("failed to build client");

    let url = match &cli.search {
        Some(q) => format!("https://html.duckduckgo.com/html/?q={}", urlencoding::encode(q)),
        None => cli.url.as_deref().expect("need a URL or --search").to_string(),
    };

    let body = match client.get(&url).send() {
        Ok(resp) => match resp.text() {
            Ok(t) => t,
            Err(e) => { eprintln!("error reading body: {e}"); std::process::exit(1); }
        },
        Err(e) => { eprintln!("error fetching {url}: {e}"); std::process::exit(1); }
    };

    if cli.raw {
        println!("{}", &body[..body.len().min(50_000)]);
        return;
    }

    let parsed = Html::parse_document(&body);
    let text = if body.contains("duckduckgo") {
        extract_search_results(&parsed)
    } else {
        extract_general_text(&parsed)
    };

    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    for line in lines.iter().take(cli.lines) {
        println!("{line}");
    }
    if lines.len() > cli.lines {
        println!("... ({} more lines)", lines.len() - cli.lines);
    }
}

fn extract_general_text(parsed: &Html) -> String {
    // Select body, skip script/style/nav/footer/header/form
    let body = parsed.select(&Selector::parse("body").unwrap()).next()
        .map(|el| el.text().collect::<Vec<_>>().join("\n"))
        .unwrap_or_default();

    body
}

fn extract_search_results(parsed: &Html) -> String {
    let mut out = String::new();

    let title_sel = Selector::parse(".result__title").unwrap();
    let snippet_sel = Selector::parse(".result__snippet").unwrap();
    let link_sel = Selector::parse(".result__a").unwrap();
    let result_sel = Selector::parse(".result").unwrap();

    for (i, result) in parsed.select(&result_sel).enumerate() {
        let title = result.select(&title_sel).next()
            .map(|e| e.text().collect::<String>())
            .unwrap_or_default();
        let url = result.select(&link_sel).next()
            .and_then(|a| a.value().attr("href").map(|s| s.to_string()))
            .unwrap_or_default();
        let snippet = result.select(&snippet_sel).next()
            .map(|e| e.text().collect::<String>())
            .unwrap_or_default();

        out.push_str(&format!("[{}] {}\n    {}\n    {}\n", i + 1, title, url, snippet));
    }

    if out.is_empty() {
        out = extract_general_text(parsed);
    }

    out
}
