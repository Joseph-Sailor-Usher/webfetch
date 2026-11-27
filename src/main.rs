use clap::{ArgGroup, Parser};
use webfetch::{client, content, fetch_capped, format_hits, label, search, truncate_chars, Format, RAW_LIMIT};

#[derive(Parser)]
#[command(name = "webfetch", version)]
#[command(about = "Fetch a web page and print its main content as Markdown, or search through SearXNG")]
#[command(group(ArgGroup::new("target").required(true).args(["url", "search", "mcp"])))]
struct Cli {
    /// URL to fetch
    url: Option<String>,

    /// Search query, answered by a SearXNG instance
    #[arg(short, long)]
    search: Option<String>,

    /// Run as an MCP server on stdio (tools: fetch_page, search); --searx, --allow-private,
    /// --allow-port, --user-agent, --allow-url, --allow-domain and --max-fetches apply to every call
    #[arg(long, conflicts_with_all = ["text", "full", "raw", "no_label", "lines", "max_bytes"])]
    mcp: bool,

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

    /// Fetch this port too, besides 80 and 443 (repeatable; others are refused while the guard is on)
    #[arg(long = "allow-port", value_name = "PORT")]
    allow_port: Vec<u16>,

    /// Leave out the line that marks the output as web content (data, not instructions)
    #[arg(long)]
    no_label: bool,

    /// With --mcp: an exact URL fetch_page may fetch without a search returning it (repeatable)
    #[arg(long = "allow-url", value_name = "URL")]
    allow_url: Vec<String>,

    /// With --mcp: a host whose pages (and its subdomains' pages) fetch_page may fetch (repeatable)
    #[arg(long = "allow-domain", value_name = "HOST")]
    allow_domain: Vec<String>,

    /// With --mcp: most fetch_page calls one session may make [default: 50]
    #[arg(long, value_name = "N")]
    max_fetches: Option<u32>,
    /// The most bytes of the response body to read; a longer body is cut there, and a
    /// Content-Length over it is refused
    #[arg(long, default_value_t = webfetch::MAX_BYTES)]
    max_bytes: usize,

    /// User agent to send (default: webfetch's own name and version)
    #[arg(long, default_value = webfetch::USER_AGENT)]
    user_agent: String,
}

fn main() {
    let cli = Cli::parse();
    webfetch::allow_ports(cli.allow_port.iter().copied());
    if !cli.mcp && (!cli.allow_url.is_empty() || !cli.allow_domain.is_empty() || cli.max_fetches.is_some()) {
        eprintln!("webfetch: --allow-url, --allow-domain and --max-fetches apply only with --mcp \
(a URL you type is fetched as you typed it)");
        std::process::exit(2);
    }
    if cli.mcp {
        if let Some(bad) = cli.allow_url.iter().find(|u| webfetch::mcp::normalize_url(u).is_none()) {
            eprintln!("webfetch: --allow-url {bad:?} is not a URL");
            std::process::exit(2);
        }
        webfetch::mcp::serve(webfetch::mcp::Settings {
            searx: cli.searx.clone(),
            allow_private: cli.allow_private,
            user_agent: cli.user_agent.clone(),
            allow_urls: cli.allow_url.clone(),
            allow_domains: cli.allow_domain.clone(),
            max_fetches: cli.max_fetches.unwrap_or(50),
            session: Default::default(),
        });
        return;
    }
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
        let checker = guard.then(webfetch::Guard::from_interfaces);
        let page = fetch_capped(&client(guard, &cli.user_agent), url, checker.as_ref(), cli.max_bytes)?;
        let cut = page.truncation_notice();
        if let Some(n) = &cut {
            eprintln!("webfetch: {n}");
        }
        if cli.raw {
            if !cli.no_label {
                println!("{}", label(&page.url));
            }
            if let Some(n) = &cut {
                println!("[{n}]");
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
        if let Some(n) = &cut {
            header.push(format!("[{n}]"));
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
