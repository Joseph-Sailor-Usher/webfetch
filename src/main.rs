use clap::{ArgGroup, Parser};
use webfetch::{client, content, fetch, format_hits, label, search, truncate_chars, Format, RAW_LIMIT};

#[derive(Parser)]
#[command(name = "webfetch", version)]
#[command(about = "Fetch a web page and print its main content as Markdown, or search through SearXNG")]
#[command(group(ArgGroup::new("target").required(true).args(["url", "search"])))]
struct Cli {
    /// URL to fetch
    url: Option<String>,

    /// Search query, answered by a SearXNG instance
    #[arg(short, long)]
    search: Option<String>,

    /// SearXNG base URL for --search
    #[arg(long, env = "WEBFETCH_SEARXNG", default_value = "http://127.0.0.1:8888")]
    searx: String,

    /// Max lines of output
    #[arg(short, long, default_value = "200")]
    lines: usize,

    /// Plain text instead of Markdown
    #[arg(short, long)]
    text: bool,

    /// The whole page instead of its main content
    #[arg(short, long, conflicts_with = "search")]
    full: bool,

    /// The page's HTML, at most 50,000 bytes
    #[arg(short, long, conflicts_with_all = ["search", "text", "full"])]
    raw: bool,

    /// Fetch private, loopback and link-local addresses too (refused by default)
    #[arg(long)]
    allow_private: bool,

    /// Leave out the line that marks the output as web content (data, not instructions)
    #[arg(long)]
    no_label: bool,

    /// User agent to send (default: webfetch's own name and version)
    #[arg(long, default_value = webfetch::USER_AGENT)]
    user_agent: String,
}

fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(&cli) {
        eprintln!("webfetch: {e}");
        std::process::exit(1);
    }
}

fn run(cli: &Cli) -> webfetch::Result<()> {
    let format = if cli.text { Format::Text } else { Format::Markdown };
    let (source, header, body) = if let Some(q) = &cli.search {
        let hits = search(&cli.searx, q)?;
        if hits.is_empty() {
            return Err(webfetch::Error(format!("no results for {q:?}")));
        }
        (format!("a search for {q:?}"), String::new(), format_hits(&hits, format))
    } else {
        let url = cli.url.as_deref().expect("clap requires a URL or --search");
        let guard = !cli.allow_private;
        let page = fetch(&client(guard, &cli.user_agent), url, guard)?;
        if cli.raw {
            if !cli.no_label {
                println!("{}", label(&page.url));
            }
            println!("{}", truncate_chars(&page.body, RAW_LIMIT));
            return Ok(());
        }
        let c = content(&page.body, &page.url, format, cli.full);
        let mut header = Vec::new();
        if !c.title.is_empty() {
            header.push(if format == Format::Markdown { format!("# {}", c.title) } else { c.title.clone() });
        }
        header.push(format!("Source: {}", page.url));
        if let Some(by) = &c.byline {
            header.push(format!("By: {by}"));
        }
        if !c.article && !cli.full {
            eprintln!("webfetch: no main content found; printing the whole page");
        }
        (page.url.clone(), header.join("\n"), c.body)
    };
    if !cli.no_label {
        println!("{}", label(&source));
    }
    if !header.is_empty() {
        println!("{header}\n");
    }
    // Markdown keeps its blank lines (they carry structure); text drops them
    let lines: Vec<&str> = body.lines().filter(|l| format == Format::Markdown || !l.trim().is_empty()).collect();
    for line in lines.iter().take(cli.lines) {
        println!("{line}");
    }
    if lines.len() > cli.lines {
        println!("... ({} more lines)", lines.len() - cli.lines);
    }
    Ok(())
}
