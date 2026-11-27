Status: agreed


# src: planning

## URL-provenance gate on the MCP server (card-2)

Precondition: the MCP server (src/mcp.rs, tests/mcp_test.rs) exists only on branch feat/readable-markdown. That branch has to be merged into main before this gate can be built, because it brings new Cargo deps and a reworked lib.rs.

### The rule

In one MCP server session (one `webfetch --mcp` process), fetch_page fetches a URL only when one of these holds:

1. **Seen in search.** The exact URL string came back as a result `url` from this session's own `search` tool. The match is exact after URL parsing and normalization (`url::Url` serialization: scheme and host lowercased, default port dropped). The query string and path must match byte for byte. The fragment is ignored, because it is never sent to the server. `https://a.example/p?q=1` does not license `https://a.example/p?q=1&d=secret`.
2. **Allowed by the operator.** At start the operator passed `--allow-url <URL>` (an exact URL, matched like rule 1) or `--allow-domain <host>` (the host itself or any subdomain, at a dot boundary: `docs.rs` allows `docs.rs` and `x.docs.rs` but not `evildocs.rs`). Both flags can be repeated.

Any other URL is refused with an isError result that names the rule, for example: `refused: <url> was not returned by this session's search and is not allowed by the operator (--allow-url/--allow-domain); ask the person to fetch it or allow it`. The agent can then ask the person.

- **Links inside fetched pages are never added to the seen set.** A page could otherwise chain generated links to leak data. Anthropic closed exactly this bypass in its web_fetch. Only search results and operator flags add URLs.
- **Redirects.** The gate checks the URL the agent asked for. The redirect policy (card-4) and the private-address guard still apply to every hop.
- **Fetch cap.** `--max-fetches N` (default 50) limits how many fetch_page calls a session may make that pass the gate and are attempted, like the API tool's max_uses. Once the cap is reached, fetch_page returns isError `fetch limit reached (N per session; --max-fetches)`. Refused URLs do not count toward the cap.
- **The CLI is unchanged.** A URL that a person typed is their own choice.

### Shape

- `Settings` gains `allow_urls: Vec<String>`, `allow_domains: Vec<String>` and `max_fetches: u32`. A `Session { seen: Mutex<HashSet<String>>, fetches: AtomicU32 }` is held next to the settings, because `handle()` takes `&Settings` and the state has to change during the session.
- `search_tool` inserts the normalized `url` of every hit into `seen`. `fetch_page` checks the gate first, then the cap, then fetches.
- main.rs: `--allow-url`, `--allow-domain` (repeatable) and `--max-fetches` (default 50). These flags are valid only with `--mcp`.

### Test (tests/mcp_test.rs: fetch_page_refuses_an_unseen_url)

The test runs a local server that plays SearXNG and serves the pages. It checks these cases:

- An unseen URL is refused (isError, and the message names the rule).
- A URL returned by search is fetched.
- A link found only inside that fetched page is refused.
- A URL under an `--allow-domain` host passes.
- With `--max-fetches 2`, the third allowed fetch is refused.

The README documents the gate and its flags.
