//! `plates serve` — a dev server for the archive's sites.
//!
//! Renders through [`crate::build`], the same collector and the same engine
//! `plates build` runs, and hands the result to a browser instead of to a
//! directory. The point is to answer "does this read right?" without deploying
//! to find out.
//!
//! # Shape
//!
//! Three threads' worth of machinery, and no more:
//!
//! - A **builder** thread owns the archive. Every prov future is `!Send`, so the
//!   workspace never crosses a thread boundary; what crosses is the finished
//!   bytes, which are plain data. It re-opens the archive for each build rather
//!   than reusing one, because a rebuild has to see files that did not exist
//!   when the server started — including a changed config document, which is
//!   where views and sites are declared.
//! - The **accept loop** hands each connection to a short-lived thread that
//!   serves from the current snapshot. Serving never blocks on a build and a
//!   build never blocks a request; a request mid-rebuild is answered from the
//!   previous snapshot, which is the same thing a static host would do.
//! - **Change detection** is [`crate::scan`]'s stat walk, and it only runs while
//!   a browser is actually watching — the injected reload script's poll is what
//!   marks the server active. An unattended `plates serve` costs nothing, which
//!   matters when the archive is tens of thousands of files rather than a blog.
//!
//! No HTTP framework, for the same reason there is no filesystem-watch crate:
//! what is needed is GET on a `BTreeMap`, and the whole of it fits in the space
//! a router's configuration would have taken.
//!
//! # What is served
//!
//! Each site is mounted under its own name (`/blog/`, `/family/`), which is the
//! layout a host serving several of them uses, and `/` lists them. `--site NAME`
//! serves that one site at the root instead. Pages link relatively, so both work
//! without rewriting anything.
//!
//! Served HTML carries one thing a built page does not: a small script that
//! polls [`REV_PATH`] and reloads when the build number moves. It is injected at
//! serve time and never written into a build, so `plates build` output is
//! byte-identical to what the server renders.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock, mpsc};
use std::thread;
use std::time::Duration;

use plates::mime_type_from_ext;
use plates_render::percent_decode;

use crate::build::{BuiltSite, build_sites, plural};
use crate::cli::SiteArgs;
use crate::session::Session;

/// The build-number endpoint the injected reload script polls. Absolute, so a
/// page at any depth reaches it.
const REV_PATH: &str = "/__plates/rev";

/// Where `serve` starts looking for a free port. Ten are tried before falling
/// back to whatever the OS hands out, so a second archive served alongside the
/// first just works.
const DEFAULT_PORT: u16 = 4321;
const PORT_ATTEMPTS: u16 = 10;

/// How often the builder may re-stat the archive. Only reached while a browser
/// is watching; see [`State::active`].
const POLL: Duration = Duration::from_millis(500);

/// A client that opens a connection and then says nothing is dropped rather
/// than held. Browsers do this — speculative connections — and a dev server
/// that waited on them forever would leak a thread per tab.
const READ_TIMEOUT: Duration = Duration::from_secs(10);

/// The largest request head accepted. A browser's is a couple of KiB; this is
/// only here so a malformed client cannot make the server allocate without
/// bound.
const MAX_HEAD: usize = 64 * 1024;

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// One build's worth of output — what every request in flight is answered from.
struct Snapshot {
    /// Build number. Monotonic, and the only thing the reload script compares.
    revision: u64,
    /// The sites, in declaration order.
    sites: Vec<BuiltSite>,
    /// Why the build failed, if it did — served in place of every path but
    /// [`REV_PATH`], which has to keep answering or the browser would never
    /// learn the archive had been fixed.
    ///
    /// The alternative, holding the last good render and serving it on, would
    /// mean a preview that silently showed HTML the archive no longer produces:
    /// the one thing a preview must not do.
    error: Option<String>,
}

struct State {
    snapshot: RwLock<Arc<Snapshot>>,
    /// Whether anything has asked for a page since the builder last looked.
    ///
    /// The builder re-stats the archive only when this is set, so an idle
    /// server — no tab open, nobody watching — does no work at all. The reload
    /// script's poll is what keeps it set while a browser is attached.
    active: AtomicBool,
    /// Whether a single site is mounted at the server root (`--site`).
    rooted: bool,
}

impl State {
    fn snapshot(&self) -> Arc<Snapshot> {
        // A poisoned lock means a serving thread panicked mid-read, which
        // leaves the snapshot itself intact — it is only ever replaced whole.
        match self.snapshot.read() {
            Ok(guard) => guard.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }

    fn publish(&self, snapshot: Snapshot) {
        let next = Arc::new(snapshot);
        match self.snapshot.write() {
            Ok(mut guard) => *guard = next,
            Err(poisoned) => *poisoned.into_inner() = next,
        }
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// `plates serve` — run until interrupted.
pub fn serve(site: &SiteArgs, host: &str, port: Option<u16>, open: bool) -> Result<(), String> {
    let root = Session::open()?.root_dir;

    // Bind before building: a port that is already taken is a much cheaper
    // thing to discover than a render of the whole archive is to throw away.
    let listener = bind(host, port).map_err(|e| format!("cannot listen on {host}: {e}"))?;
    let addr = listener
        .local_addr()
        .map_err(|e| format!("cannot read the listening address: {e}"))?;

    let state = Arc::new(State {
        snapshot: RwLock::new(Arc::new(Snapshot {
            revision: 0,
            sites: Vec::new(),
            error: None,
        })),
        active: AtomicBool::new(false),
        rooted: site.site.is_some(),
    });

    let (ready_tx, ready_rx) = mpsc::channel::<()>();
    {
        let state = state.clone();
        let root = root.clone();
        let site = site.clone();
        thread::Builder::new()
            .name("plates-build".to_string())
            .spawn(move || builder_loop(state, root, site, ready_tx))
            .map_err(|e| format!("cannot start the builder thread: {e}"))?;
    }

    // The first build is waited on so an archive that cannot be rendered at all
    // says so and exits, rather than starting a server that serves an error.
    if ready_rx.recv().is_err() {
        return Err("the builder stopped before it produced anything".to_string());
    }
    let first = state.snapshot();
    if let Some(error) = &first.error {
        return Err(error.clone());
    }

    let origin = format!("http://{}", display_addr(host, addr.port()));
    println!(
        "✓ Serving {} site{} from {}",
        first.sites.len(),
        plural(first.sites.len()),
        root.display()
    );
    for built in &first.sites {
        let path = if state.rooted {
            "/".to_string()
        } else {
            format!("/{}/", built.name)
        };
        println!(
            "  {origin}{path}  — {}, {} page{}",
            built.audience,
            built.pages,
            plural(built.pages)
        );
    }
    println!();
    println!("  Reloads on change. Press Ctrl-C to stop.");

    if open {
        let first_path = match (state.rooted, first.sites.first()) {
            (true, _) | (_, None) => String::new(),
            (false, Some(built)) => format!("/{}/", built.name),
        };
        open_browser(&format!("{origin}{first_path}"));
    }
    drop(first);

    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let state = state.clone();
        // A thread per connection, because each one is answered from an
        // already-built snapshot and finishes immediately. `Connection: close`
        // means there is never more than one request on it.
        if thread::Builder::new()
            .spawn(move || serve_connection(stream, &state))
            .is_err()
        {
            eprintln!("! Dropped a connection: could not spawn a thread for it");
        }
    }

    Ok(())
}

/// Bind the listener, choosing a port when none was named.
fn bind(host: &str, port: Option<u16>) -> std::io::Result<TcpListener> {
    if let Some(port) = port {
        return TcpListener::bind((host, port));
    }
    for port in DEFAULT_PORT..DEFAULT_PORT.saturating_add(PORT_ATTEMPTS) {
        match TcpListener::bind((host, port)) {
            Ok(listener) => return Ok(listener),
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => continue,
            Err(e) => return Err(e),
        }
    }
    // Everything in the range is taken: take whatever the OS has. The address
    // is printed, so an unusual port is not a mystery.
    TcpListener::bind((host, 0))
}

/// The host as a browser should be pointed at it — a wildcard bind is reachable
/// at every address the machine has, and `http://0.0.0.0/` is not one of them.
fn display_addr(host: &str, port: u16) -> String {
    match host {
        "0.0.0.0" | "::" | "[::]" => format!("localhost:{port}"),
        host if host.contains(':') => format!("[{host}]:{port}"),
        host => format!("{host}:{port}"),
    }
}

fn open_browser(url: &str) {
    #[cfg(target_os = "macos")]
    let (program, args) = ("open", vec![url]);
    #[cfg(target_os = "windows")]
    let (program, args) = ("cmd", vec!["/C", "start", "", url]);
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let (program, args) = ("xdg-open", vec![url]);

    if std::process::Command::new(program)
        .args(args)
        .spawn()
        .is_err()
    {
        eprintln!("! Could not open a browser; visit {url}");
    }
}

// ---------------------------------------------------------------------------
// The builder thread
// ---------------------------------------------------------------------------

/// Build once, signal readiness, then rebuild whenever the archive changes
/// under a watching browser.
fn builder_loop(
    state: Arc<State>,
    root: std::path::PathBuf,
    site: SiteArgs,
    ready: mpsc::Sender<()>,
) {
    let mut revision = 0u64;
    // `serve` writes nothing, so there is no build directory to leave out.
    let mut fingerprint = crate::scan::fingerprint(&root, None);
    revision += 1;
    state.publish(build(&site, revision));
    let _ = ready.send(());

    loop {
        thread::sleep(POLL);
        // `swap` rather than `load`: one scan per burst of requests, and none
        // at all once the browser stops asking.
        if !state.active.swap(false, Ordering::Relaxed) {
            continue;
        }
        let next = crate::scan::fingerprint(&root, None);
        if next == fingerprint {
            continue;
        }
        fingerprint = next;
        revision += 1;
        let snapshot = build(&site, revision);
        match &snapshot.error {
            Some(error) => eprintln!("✗ Rebuild failed: {error}"),
            None => println!(
                "  rebuilt {} site{} (#{revision})",
                snapshot.sites.len(),
                plural(snapshot.sites.len())
            ),
        }
        state.publish(snapshot);
    }
}

/// Open the archive and render it. A failure is carried in the snapshot rather
/// than ending the server: the usual cause is a half-finished edit, and the fix
/// is the next keystroke.
fn build(site: &SiteArgs, revision: u64) -> Snapshot {
    let built = Session::open().and_then(|session| {
        crate::commands::report(&session.warnings);
        build_sites(&session, site.site.as_deref(), site.base_url.as_deref())
    });
    match built {
        Ok(sites) => {
            // A theme that would not load or a shell that would not compile is
            // reported every rebuild: the dev server is where a person is
            // editing the shell, so it is the one place the message is worth
            // repeating.
            for built in &sites {
                crate::commands::report(&built.warnings);
            }
            Snapshot {
                revision,
                sites,
                error: None,
            }
        }
        Err(error) => Snapshot {
            revision,
            sites: Vec::new(),
            error: Some(error),
        },
    }
}

// ---------------------------------------------------------------------------
// HTTP
// ---------------------------------------------------------------------------

struct Reply {
    status: u16,
    content_type: String,
    body: Vec<u8>,
    location: Option<String>,
}

impl Reply {
    fn html(status: u16, body: String) -> Self {
        Self {
            status,
            content_type: "text/html; charset=utf-8".to_string(),
            body: body.into_bytes(),
            location: None,
        }
    }

    fn text(status: u16, body: String) -> Self {
        Self {
            status,
            content_type: "text/plain; charset=utf-8".to_string(),
            body: body.into_bytes(),
            location: None,
        }
    }

    fn redirect(to: &str) -> Self {
        Self {
            status: 302,
            content_type: "text/plain; charset=utf-8".to_string(),
            body: Vec::new(),
            location: Some(to.to_string()),
        }
    }

    fn file(rel: &str, body: Vec<u8>) -> Self {
        Self {
            status: 200,
            content_type: content_type_for(rel),
            body,
            location: None,
        }
    }

    fn is_html(&self) -> bool {
        self.content_type.starts_with("text/html")
    }
}

/// The type to serve a path as.
///
/// The text formats a render produces — the stylesheet, the sitemap, the feeds,
/// `robots.txt` — are named here with their charsets; everything else is an
/// attachment, and those are exactly what `plates`' own table is for, so it
/// answers rather than a second copy of it.
fn content_type_for(rel: &str) -> String {
    let lower = rel.to_ascii_lowercase();
    if lower.ends_with(".html") {
        "text/html; charset=utf-8".to_string()
    } else if lower.ends_with(".css") {
        "text/css; charset=utf-8".to_string()
    } else if lower.ends_with(".xml") {
        "application/xml; charset=utf-8".to_string()
    } else if lower.ends_with(".txt") {
        "text/plain; charset=utf-8".to_string()
    } else if lower.ends_with(".json") {
        "application/json".to_string()
    } else {
        mime_type_from_ext(Path::new(rel))
    }
}

fn serve_connection(stream: TcpStream, state: &State) {
    let _ = stream.set_read_timeout(Some(READ_TIMEOUT));
    let _ = stream.set_nodelay(true);

    let Some((method, target)) = read_request(&stream) else {
        return;
    };

    let mut reply = match method.as_str() {
        "GET" | "HEAD" => route(state, &target),
        _ => Reply::text(405, format!("{method} is not supported\n")),
    };
    if reply.is_html() {
        inject_reload(&mut reply);
    }

    let head_only = method == "HEAD";
    let _ = write_reply(stream, &reply, head_only);
}

/// Read a request's head and return its method and target. `None` for anything
/// that is not a request line — including a connection that was opened and
/// never used, which is what the read timeout catches.
fn read_request(stream: &TcpStream) -> Option<(String, String)> {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    if reader.read_line(&mut line).ok()? == 0 {
        return None;
    }

    let mut parts = line.split_whitespace();
    let method = parts.next()?.to_string();
    let target = parts.next()?.to_string();

    // The rest of the head is read and discarded — nothing served here varies
    // by header — but it is read, so the client is not left writing into a
    // socket nobody drained.
    let mut consumed = line.len();
    loop {
        let mut header = String::new();
        match reader.read_line(&mut header) {
            Ok(0) => break,
            Ok(n) => {
                consumed += n;
                if header.trim().is_empty() || consumed > MAX_HEAD {
                    break;
                }
            }
            Err(_) => return None,
        }
    }

    Some((method, target))
}

fn write_reply(mut stream: TcpStream, reply: &Reply, head_only: bool) -> std::io::Result<()> {
    let mut head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\nConnection: close\r\n",
        reply.status,
        reason(reply.status),
        reply.content_type,
        reply.body.len(),
    );
    if let Some(location) = &reply.location {
        head.push_str(&format!("Location: {location}\r\n"));
    }
    head.push_str("\r\n");

    stream.write_all(head.as_bytes())?;
    if !head_only {
        stream.write_all(&reply.body)?;
    }
    stream.flush()
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        302 => "Found",
        404 => "Not Found",
        405 => "Method Not Allowed",
        500 => "Internal Server Error",
        _ => "OK",
    }
}

// ---------------------------------------------------------------------------
// Routing
// ---------------------------------------------------------------------------

fn route(state: &State, target: &str) -> Reply {
    let snapshot = state.snapshot();
    let path = percent_decode(target.split(['?', '#']).next().unwrap_or("/"));

    // Any request means someone is watching, which is what lets the builder
    // stop looking at the archive when nobody is.
    state.active.store(true, Ordering::Relaxed);

    // The build number is the one thing that must still be answerable while a
    // build is failing, or a browser would never learn the archive was fixed.
    if path == REV_PATH {
        return Reply::text(200, snapshot.revision.to_string());
    }

    if let Some(error) = &snapshot.error {
        return Reply::html(500, error_page(error));
    }

    let rest = path.strip_prefix('/').unwrap_or(&path);
    let (site, rel) = if state.rooted {
        (snapshot.sites.first(), rest.to_string())
    } else {
        match rest.split_once('/') {
            Some((name, rel)) => (find(&snapshot.sites, name), rel.to_string()),
            None if rest.is_empty() => return Reply::html(200, index_page(&snapshot.sites)),
            // `/blog` names a site but is not inside it: redirect, so the
            // page's relative links resolve below the site rather than beside
            // it.
            None => {
                return match find(&snapshot.sites, rest) {
                    Some(site) => Reply::redirect(&format!("/{}/", site.name)),
                    None => Reply::html(404, missing_page(&snapshot.sites, None, &path)),
                };
            }
        }
    };

    let Some(site) = site else {
        return Reply::html(404, missing_page(&snapshot.sites, None, &path));
    };

    let rel = if rel.is_empty() || rel.ends_with('/') {
        format!("{rel}index.html")
    } else {
        rel
    };

    match read(site, &rel) {
        Some(bytes) => Reply::file(&rel, bytes),
        None => Reply::html(404, missing_page(&snapshot.sites, Some(site), &path)),
    }
}

fn find<'s>(sites: &'s [BuiltSite], name: &str) -> Option<&'s BuiltSite> {
    sites.iter().find(|site| site.name == name)
}

/// The bytes for `rel` within a site: rendered output as it stands, or an
/// attachment read from the archive now.
///
/// The read is why an attachment's bytes are never collected (see
/// `UnreadAttachments`): a photograph is served when a browser asks for it, and
/// an archive whose photographs outweigh its prose costs a preview nothing
/// until then.
fn read(site: &BuiltSite, rel: &str) -> Option<Vec<u8>> {
    if let Some(bytes) = site.files.get(rel) {
        return Some(bytes.clone());
    }
    std::fs::read(site.attachments.get(rel)?).ok()
}

// ---------------------------------------------------------------------------
// Generated pages
// ---------------------------------------------------------------------------

/// Polls the build number and reloads when it moves.
///
/// Deliberately not a websocket: a poll is a dozen lines with no handshake, no
/// framing and no second connection to keep alive, and at this interval the
/// difference is imperceptible. The backoff on failure is what makes Ctrl-C
/// quiet — a dead server produces one failed fetch every two seconds rather
/// than a console full of them.
const RELOAD_SCRIPT: &str = r#"<script>
(function () {
  var rev = null;
  function poll() {
    fetch('/__plates/rev', { cache: 'no-store' })
      .then(function (r) { return r.text(); })
      .then(function (next) {
        if (rev === null) { rev = next; }
        else if (next !== rev) { location.reload(); return; }
        setTimeout(poll, 700);
      })
      .catch(function () { setTimeout(poll, 2000); });
  }
  poll();
})();
</script>"#;

/// Put the reload script in the served HTML, before `</body>` where one exists.
fn inject_reload(reply: &mut Reply) {
    let Ok(html) = std::str::from_utf8(&reply.body) else {
        return;
    };
    let injected = match html.rfind("</body>") {
        Some(at) => format!("{}{RELOAD_SCRIPT}{}", &html[..at], &html[at..]),
        None => format!("{html}{RELOAD_SCRIPT}"),
    };
    reply.body = injected.into_bytes();
}

const PAGE_CSS: &str = "body{font:16px/1.5 system-ui,sans-serif;max-width:42rem;\
margin:4rem auto;padding:0 1.5rem;color:#1c1c1c;background:#fbfbfa}\
a{color:#2f5fdf}h1{font-size:1.4rem}li{margin:.35rem 0}\
pre{white-space:pre-wrap;background:#f2f0ec;padding:1rem;border-radius:.4rem}\
.dim{color:#6b6b6b;font-size:.9rem}";

fn shell(title: &str, body: &str) -> String {
    format!(
        "<!DOCTYPE html><html lang=\"en\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
         <title>{}</title><style>{PAGE_CSS}</style></head><body>{body}</body></html>",
        escape(title)
    )
}

/// The listing at `/` when more than one site is mounted.
fn index_page(sites: &[BuiltSite]) -> String {
    let items: String = sites
        .iter()
        .map(|site| {
            format!(
                "<li><a href=\"/{name}/\">{name}</a> <span class=\"dim\">— {audience}, \
                 {pages} page{s}</span></li>",
                name = escape(&site.name),
                audience = escape(&site.audience),
                pages = site.pages,
                s = plural(site.pages),
            )
        })
        .collect();
    shell(
        "plates — sites",
        &format!("<h1>Sites in this archive</h1><ul>{items}</ul>"),
    )
}

fn error_page(error: &str) -> String {
    shell(
        "plates — build failed",
        &format!(
            "<h1>This archive did not render</h1><pre>{}</pre>\
             <p class=\"dim\">Fix it and this page reloads itself.</p>",
            escape(error)
        ),
    )
}

/// A 404 that says what *is* here — the fastest way to see that a page was
/// filtered out by its audience rather than misspelled in a link.
fn missing_page(sites: &[BuiltSite], site: Option<&BuiltSite>, path: &str) -> String {
    const SHOWN: usize = 100;

    let body = match site {
        Some(site) => {
            let pages: Vec<&String> = site
                .files
                .keys()
                .filter(|key| key.ends_with(".html"))
                .collect();
            let items: String = pages
                .iter()
                .take(SHOWN)
                .map(|page| {
                    format!(
                        "<li><a href=\"/{prefix}{page}\">{page}</a></li>",
                        prefix = escape(&format!("{}/", site.name)),
                        page = escape(page),
                    )
                })
                .collect();
            let more = pages.len().saturating_sub(SHOWN);
            let note = if more > 0 {
                format!("<p class=\"dim\">and {more} more</p>")
            } else {
                String::new()
            };
            format!(
                "<h1>Not in <em>{}</em></h1><p class=\"dim\">{}</p>\
                 <p>This site holds:</p><ul>{items}</ul>{note}",
                escape(&site.name),
                escape(path),
            )
        }
        None => {
            let items: String = sites
                .iter()
                .map(|site| {
                    format!(
                        "<li><a href=\"/{name}/\">{name}</a></li>",
                        name = escape(&site.name)
                    )
                })
                .collect();
            format!(
                "<h1>No such site</h1><p class=\"dim\">{}</p>\
                 <p>This archive serves:</p><ul>{items}</ul>",
                escape(path)
            )
        }
    };

    shell("plates — not found", &body)
}

/// Escape text for HTML. The generated pages above quote paths and error
/// messages, which come from the archive and therefore from whatever someone
/// named a file.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn built(name: &str, pages: &[&str]) -> BuiltSite {
        BuiltSite {
            name: name.to_string(),
            audience: "public".to_string(),
            files: pages
                .iter()
                .map(|p| ((*p).to_string(), b"<html><body>x</body></html>".to_vec()))
                .collect(),
            attachments: BTreeMap::new(),
            pages: pages.len(),
            warnings: Vec::new(),
        }
    }

    fn state(sites: Vec<BuiltSite>, rooted: bool) -> State {
        State {
            snapshot: RwLock::new(Arc::new(Snapshot {
                revision: 7,
                sites,
                error: None,
            })),
            active: AtomicBool::new(false),
            rooted,
        }
    }

    /// A site's root is reached at `/{name}/`, and its pages below that — the
    /// same shape a build has, so a link that works here works there.
    #[test]
    fn a_site_is_mounted_under_its_name() {
        let state = state(
            vec![built("blog", &["index.html", "notes/post.html"])],
            false,
        );

        assert_eq!(route(&state, "/blog/").status, 200);
        assert_eq!(route(&state, "/blog/notes/post.html").status, 200);
        assert_eq!(route(&state, "/blog/missing.html").status, 404);
    }

    /// Without the trailing slash the browser would resolve the page's relative
    /// links one level too high, so the site's bare name redirects rather than
    /// serving.
    #[test]
    fn a_bare_site_name_redirects_to_its_root() {
        let state = state(vec![built("blog", &["index.html"])], false);
        let reply = route(&state, "/blog");

        assert_eq!(reply.status, 302);
        assert_eq!(reply.location.as_deref(), Some("/blog/"));
    }

    /// `--site` means "this site is the whole server", so it answers at the
    /// root with no prefix to strip.
    #[test]
    fn a_single_site_serves_at_the_root() {
        let state = state(
            vec![built("blog", &["index.html", "notes/post.html"])],
            true,
        );

        assert_eq!(route(&state, "/").status, 200);
        assert_eq!(route(&state, "/notes/post.html").status, 200);
        assert_eq!(route(&state, "/blog/").status, 404);
    }

    /// The reload script is what makes this a dev server rather than a static
    /// one, and it goes inside the document rather than after it.
    #[test]
    fn served_html_carries_the_reload_script() {
        let state = state(vec![built("blog", &["index.html"])], true);
        let mut reply = route(&state, "/");
        inject_reload(&mut reply);
        let html = String::from_utf8(reply.body).unwrap();

        assert!(html.contains(REV_PATH), "the poll endpoint is named");
        assert!(
            html.find(REV_PATH) < html.find("</body>"),
            "and the script is inside the body it was injected into"
        );
    }

    /// The build number is the only thing the reload script compares, so it has
    /// to be answerable even while the build is broken.
    #[test]
    fn the_build_number_is_served_through_a_failure() {
        let state = State {
            snapshot: RwLock::new(Arc::new(Snapshot {
                revision: 12,
                sites: vec![built("blog", &["index.html"])],
                error: Some("bad frontmatter".to_string()),
            })),
            active: AtomicBool::new(false),
            rooted: true,
        };

        let rev = route(&state, REV_PATH);
        assert_eq!(rev.status, 200);
        assert_eq!(String::from_utf8(rev.body).unwrap(), "12");

        let page = route(&state, "/");
        assert_eq!(page.status, 500, "and the page says so rather than lying");
        assert!(
            String::from_utf8(page.body)
                .unwrap()
                .contains("bad frontmatter")
        );
    }

    /// A percent-encoded path is a file name with a space in it, which archives
    /// are full of.
    #[test]
    fn percent_encoded_paths_reach_their_page() {
        let state = state(vec![built("blog", &["my note.html"])], true);
        assert_eq!(route(&state, "/my%20note.html").status, 200);
    }

    /// A query string is the reload script's cache-buster and a browser's
    /// habit; neither is part of the path.
    #[test]
    fn a_query_string_is_not_part_of_the_path() {
        let state = state(vec![built("blog", &["index.html"])], true);
        assert_eq!(route(&state, "/?t=1").status, 200);
    }

    /// The stylesheet has to arrive as CSS or the browser ignores it, and an
    /// attachment's type is `plates`' table to answer.
    #[test]
    fn content_types_cover_both_halves_of_a_site() {
        assert!(content_type_for("style.css").starts_with("text/css"));
        assert!(content_type_for("feed.xml").starts_with("application/xml"));
        assert_eq!(content_type_for("img/photo.jpg"), "image/jpeg");
    }
}
