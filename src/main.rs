use clap::{ArgGroup, Parser};
use scraper::Html;
use webfetch::{client, extract_text, fetch, format_hits, search, truncate_chars, RAW_LIMIT};

#[derive(Parser)]
#[command(name = "webfetch", version)]
#[command(about = "Fetch a URL and print its readable text, or search the web")]
#[command(group(ArgGroup::new("target").required(true).args(["url", "search"])))]
struct Cli {
    /// URL to fetch
    url: Option<String>,

    /// Search query (SearXNG when one answers, else DuckDuckGo's HTML endpoint)
    #[arg(short, long)]
    search: Option<String>,

    /// SearXNG base URL for --search
    #[arg(long, env = "WEBFETCH_SEARXNG", default_value = "http://127.0.0.1:8888")]
    searx: String,

    /// Search DuckDuckGo only, without trying SearXNG
    #[arg(long)]
    no_searx: bool,

    /// Max lines of output
    #[arg(short, long, default_value = "200")]
    lines: usize,

    /// Raw output: the page's HTML, at most 50,000 bytes (not used with --search)
    #[arg(short, long, conflicts_with = "search")]
    raw: bool,
}

fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(&cli) {
        eprintln!("webfetch: {e}");
        std::process::exit(1);
    }
}

fn run(cli: &Cli) -> webfetch::Result<()> {
    let client = client();
    let text = if let Some(q) = &cli.search {
        let searx = (!cli.no_searx).then_some(cli.searx.as_str());
        let hits = search(&client, searx, q)?;
        if hits.is_empty() {
            return Err(webfetch::Error(format!("no results for {q:?}")));
        }
        format_hits(&hits)
    } else {
        let url = cli.url.as_deref().expect("clap requires a URL or --search");
        let body = fetch(&client, url)?;
        if cli.raw {
            println!("{}", truncate_chars(&body, RAW_LIMIT));
            return Ok(());
        }
        extract_text(&Html::parse_document(&body))
    };
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    for line in lines.iter().take(cli.lines) {
        println!("{line}");
    }
    if lines.len() > cli.lines {
        println!("... ({} more lines)", lines.len() - cli.lines);
    }
    Ok(())
}
