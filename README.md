# webfetch

A small Rust CLI that fetches a web page and prints its readable text, or searches the web and prints
the results.

```sh
cargo build --release
target/release/webfetch https://www.rust-lang.org/          # the page's text, 200 lines at most
target/release/webfetch -l 50 https://example.com           # at most 50 lines
target/release/webfetch --raw https://example.com           # the HTML, at most 50,000 bytes
target/release/webfetch --search "rust borrow checker"      # search results
```

## Text

The text is read from the page's `<body>`: one line per block element (paragraphs, headings, list
items, table cells), inline elements kept on their line, whitespace collapsed. Scripts, styles,
navigation, forms and the page's own header and footer are left out; a header or footer inside
`<main>` or `<article>` (an article's title, a hero headline) is kept.

## Search

`--search` asks a [SearXNG](https://docs.searxng.org/) instance first, through its JSON API
(`/search?format=json`; JSON must be enabled in its `settings.yml`). Set it with `--searx <url>` or
`WEBFETCH_SEARXNG`; the default is `http://127.0.0.1:8888`. If SearXNG does not answer, webfetch tries
DuckDuckGo's HTML endpoint (`--no-searx` goes straight there). DuckDuckGo often answers automated
clients with a bot challenge; webfetch reports that as an error rather than printing the challenge.

## Exit status

- `0`: the page or the results were printed
- `1`: the fetch failed (network error, an HTTP status outside 2xx, a search with no results or a
  blocked search); the reason is on stderr
- `2`: the command line was wrong (for example neither a URL nor `--search`)
