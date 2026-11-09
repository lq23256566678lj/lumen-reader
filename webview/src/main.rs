#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

use std::fs;
use std::io::{Cursor, Read};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;

use serde::{Deserialize, Serialize};
use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoop};
use tao::window::WindowBuilder;
use tiny_http::{Header, Method, Response, Server};
use wry::WebViewBuilder;

type AppError = Box<dyn std::error::Error + Send + Sync>;

const INDEX_HTML: &str = include_str!("../web/index.html");
const APP_JS: &str = include_str!("../web/app.js");
const STYLE_CSS: &str = include_str!("../web/style.css");

#[derive(Clone, Serialize)]
struct SpineItem {
    href: String,
    title: String,
}

#[derive(Clone, Serialize)]
struct TocItem {
    title: String,
    href: String,
    fragment: String,
    spine: usize,
}

#[derive(Clone, Serialize)]
struct BookInfo {
    kind: String,
    title: String,
    author: String,
    spine: Vec<SpineItem>,
    toc: Vec<TocItem>,
    #[serde(default)]
    spine_map: Vec<usize>,
}

struct Shared {
    root: PathBuf,
    data_dir: PathBuf,
    book: Mutex<Option<BookInfo>>,
    file_path: Mutex<Option<PathBuf>>,
}

fn platform_data_dir(root: &Path) -> PathBuf {
    if let Some(configured) = std::env::var_os("LUMEN_DATA_DIR") {
        return PathBuf::from(configured);
    }
    #[cfg(target_os = "windows")]
    if let Some(appdata) = std::env::var_os("APPDATA") {
        return PathBuf::from(appdata).join("LumenReader");
    }
    #[cfg(target_os = "macos")]
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join("Library/Application Support/LumenReader");
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Some(data_home) = std::env::var_os("XDG_DATA_HOME") {
            return PathBuf::from(data_home).join("LumenReader");
        }
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(".local/share/LumenReader");
        }
    }
    root.join("data")
}

fn main() -> Result<(), AppError> {
    eprintln!("step: temp dir");
    let root = std::env::current_exe()?
        .parent()
        .unwrap_or(Path::new("."))
        .join("webview-data");
    eprintln!("root={}", root.display());
    fs::create_dir_all(&root)?;
    let data_dir = platform_data_dir(&root);
    eprintln!("step: server data={}", data_dir.display());
    let shared = Arc::new(Shared {
        root,
        data_dir,
        book: Mutex::new(None),
        file_path: Mutex::new(None),
    });

    let server = Server::http("127.0.0.1:0")?;
    let port = server
        .server_addr()
        .to_ip()
        .ok_or("server did not bind to TCP")?
        .port();
    let server_shared = Arc::clone(&shared);
    thread::spawn(move || {
        for request in server.incoming_requests() {
            if let Err(error) = handle_request(request, &server_shared) {
                eprintln!("request failed: {error}");
            }
        }
    });

    if std::env::args().any(|argument| argument == "--server") {
        println!("http://127.0.0.1:{port}/");
        loop {
            thread::sleep(std::time::Duration::from_secs(3600));
        }
    }

    if let Some(path) = std::env::args().skip(1).find(|argument| !argument.starts_with("--")) {
        let path = PathBuf::from(path);
        let title = path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("未命名");
        let format = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("epub");
        if let Err(error) = open_path(&path, title, "", format, &shared) {
            eprintln!("failed to preload {}: {error}", path.display());
        }
    }

    eprintln!("step: event loop");
    let event_loop = EventLoop::new();
    eprintln!("step: window");
    let window = WindowBuilder::new()
        .with_title("Lumen Reader · WebView2")
        .with_inner_size(LogicalSize::new(1320.0, 900.0))
        .with_min_inner_size(LogicalSize::new(900.0, 620.0))
        .build(&event_loop)?;

    let url = format!("http://127.0.0.1:{port}/");
    eprintln!("step: webview {url}");
    let webview = WebViewBuilder::new()
        .with_url(url)
        .with_devtools(true)
        .build(&window)?;
    eprintln!("step: webview built");

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        let _ = &webview;
        if let Event::WindowEvent {
            event: WindowEvent::CloseRequested,
            ..
        } = event
        {
            *control_flow = ControlFlow::Exit;
        }
    });
}

fn handle_request(request: tiny_http::Request, shared: &Shared) -> Result<(), AppError> {
    let method = request.method().clone();
    let url = request.url().split('?').next().unwrap_or("/").to_owned();

    if method == Method::Get && url == "/" {
        return respond_text(request, INDEX_HTML, "text/html; charset=utf-8");
    }
    if method == Method::Get && url == "/app.js" {
        return respond_text(request, APP_JS, "text/javascript; charset=utf-8");
    }
    if method == Method::Get && url == "/style.css" {
        return respond_text(request, STYLE_CSS, "text/css; charset=utf-8");
    }
    if method == Method::Post && url == "/open" {
        return open_book(request, shared);
    }
    if method == Method::Post && url == "/open-id" {
        return open_by_id(request, shared);
    }
    if method == Method::Post && url == "/import" {
        return import_book(request, shared);
    }
    if method == Method::Post && url == "/progress" {
        return save_progress(request, shared);
    }
    if method == Method::Get && url == "/api/bookmarks" {
        return list_bookmarks(request, shared);
    }
    if method == Method::Post && url == "/api/bookmarks" {
        return add_bookmark(request, shared);
    }
    if method == Method::Delete && url.starts_with("/api/bookmarks/") {
        return delete_bookmark(request, shared, &url["/api/bookmarks/".len()..]);
    }
    if method == Method::Post && url == "/folders" {
        return create_folder(request, shared);
    }
    if method == Method::Put && url.starts_with("/folders/") {
        return update_folder(request, shared, &url["/folders/".len()..]);
    }
    if method == Method::Delete && url.starts_with("/folders/") {
        return remove_folder(request, shared, &url["/folders/".len()..]);
    }
    if method == Method::Post && url == "/move-book" {
        return move_book(request, shared);
    }
    if method == Method::Get && url == "/api/settings" {
        return get_settings(request, shared);
    }
    if method == Method::Post && url == "/api/settings" {
        return save_settings(request, shared);
    }
    if method == Method::Get && url == "/api/webdav" {
        return get_webdav(request, shared);
    }
    if method == Method::Post && url == "/api/webdav" {
        return save_webdav(request, shared);
    }
    if method == Method::Post && url == "/api/webdav/test" {
        return test_webdav(request, shared);
    }
    if method == Method::Post && url == "/api/sync" {
        return sync_webdav(request, shared);
    }
    if method == Method::Get && url == "/api/library" {
        let path = shared.data_dir.join("library.json");
        let body = fs::read_to_string(path).unwrap_or_else(|_| {
            "{\"version\":2,\"books\":[],\"bookmarks\":[],\"collections\":[]}".to_owned()
        });
        return respond_text(request, &body, "application/json; charset=utf-8");
    }
    if method == Method::Get && url == "/bookfile" {
        return serve_current_file(request, shared);
    }
    if method == Method::Get {
        if let Some(name) = url.strip_prefix("/cover/") {
            return serve_cover(request, name, shared);
        }
    }
    if method == Method::Get && url == "/api/book" {
        let body = shared
            .book
            .lock()
            .map_err(|_| "book lock poisoned")?
            .clone()
            .map_or_else(|| "null".to_owned(), |book| serde_json::to_string(&book).unwrap_or_else(|_| "null".to_owned()));
        return respond_text(request, &body, "application/json; charset=utf-8");
    }
    if method == Method::Get {
        if let Some(path) = url.strip_prefix("/book/") {
            return serve_book_file(request, path, shared);
        }
    }
    if method == Method::Get && url == "/favicon.ico" {
        return respond_text(request, "", "image/x-icon");
    }

    request.respond(Response::from_string("not found").with_status_code(404))?;
    Ok(())
}

fn respond_text(
    request: tiny_http::Request,
    body: &str,
    content_type: &str,
) -> Result<(), AppError> {
    // `Response::from_string` sets a default text/plain Content-Type. Adding a
    // second header leaves the browser with text/plain, which displays the HTML
    // source as garbled text. Start from raw bytes so our header is the only one.
    let mut response = Response::from_data(body.as_bytes().to_vec());
    if let Ok(header) = Header::from_bytes("Content-Type", content_type) {
        response = response.with_header(header);
    }
    if let Ok(header) = Header::from_bytes("Cache-Control", "no-store") {
        response = response.with_header(header);
    }
    request.respond(response)?;
    Ok(())
}

fn open_book(mut request: tiny_http::Request, shared: &Shared) -> Result<(), AppError> {
    let mut bytes = Vec::new();
    request.as_reader().take(350 * 1024 * 1024).read_to_end(&mut bytes)?;
    if bytes.is_empty() {
        return respond_text(request, "empty upload", "text/plain; charset=utf-8");
    }

    let result = load_book(&bytes, shared);

    match result {
        Ok(info) => {
            let body = serde_json::to_string(&info)?;
            respond_text(request, &body, "application/json; charset=utf-8")
        }
        Err(error) => {
            let body = serde_json::json!({ "error": error.to_string() }).to_string();
            let mut response = Response::from_string(body).with_status_code(400);
            if let Ok(header) = Header::from_bytes("Content-Type", "application/json; charset=utf-8") {
                response = response.with_header(header);
            }
            request.respond(response)?;
            Ok(())
        }
    }
}

fn load_book(bytes: &[u8], shared: &Shared) -> Result<BookInfo, AppError> {
    let dir = shared.root.join("book");
    if dir.exists() {
        fs::remove_dir_all(&dir)?;
    }
    fs::create_dir_all(&dir)?;
    extract_zip(bytes, &dir)?;
    let info = parse_epub(&dir)?;
    *shared.book.lock().map_err(|_| "book lock poisoned")? = Some(info.clone());
    Ok(info)
}

#[derive(Deserialize)]
struct OpenRequest {
    id: String,
}

fn import_book(mut request: tiny_http::Request, shared: &Shared) -> Result<(), AppError> {
    let file_name = request
        .headers()
        .iter()
        .find(|header| header.field.equiv("X-Filename"))
        .map(|header| urlencoding::decode(header.value.as_str()).unwrap_or_default().into_owned())
        .unwrap_or_else(|| "imported.epub".to_owned());
    let mut bytes = Vec::new();
    request
        .as_reader()
        .take(350 * 1024 * 1024)
        .read_to_end(&mut bytes)?;
    if bytes.is_empty() {
        return respond_error(request, "文件为空".to_owned());
    }
    let extension = Path::new(&file_name)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("epub")
        .to_ascii_lowercase();
    let books_dir = shared.data_dir.join("books");
    fs::create_dir_all(&books_dir)?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    use std::hash::{Hash, Hasher};
    bytes.hash(&mut hasher);
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .hash(&mut hasher);
    let id = format!("web-{:016x}", hasher.finish());
    let stored = books_dir.join(format!("{id}.{extension}"));
    fs::write(&stored, &bytes)?;

    let library_path = shared.data_dir.join("library.json");
    let mut library: serde_json::Value = fs::read_to_string(&library_path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_else(|| serde_json::json!({"version":2,"books":[],"bookmarks":[],"collections":[]}));
    let title = Path::new(&file_name)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("未命名")
        .to_owned();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let book = serde_json::json!({
        "id": id,
        "title": title,
        "author": "",
        "collection_id": null,
        "format": extension,
        "path": stored.to_string_lossy(),
        "file_size": bytes.len(),
        "cover_file": null,
        "chapter_count": 0,
        "progress": {"chapter":0,"fraction":0.0,"scroll_offset":0.0,"updated_at":now},
        "added_at": now,
        "last_opened_at": null
    });
    if let Some(books) = library.get_mut("books").and_then(|value| value.as_array_mut()) {
        books.push(book);
    }
    fs::write(&library_path, serde_json::to_vec(&library)?)?;
    let body = serde_json::json!({"id": id, "title": title}).to_string();
    respond_text(request, &body, "application/json; charset=utf-8")
}

fn open_by_id(mut request: tiny_http::Request, shared: &Shared) -> Result<(), AppError> {
    let mut body = String::new();
    request
        .as_reader()
        .take(64 * 1024)
        .read_to_string(&mut body)?;
    let request_data: OpenRequest = match serde_json::from_str(&body) {
        Ok(value) => value,
        Err(error) => return respond_error(request, error.to_string()),
    };

    let library_path = shared.data_dir.join("library.json");
    let library_text = match fs::read_to_string(&library_path) {
        Ok(text) => text,
        Err(error) => return respond_error(request, format!("读取书架失败：{error}")),
    };
    let library: serde_json::Value = match serde_json::from_str(&library_text) {
        Ok(value) => value,
        Err(error) => return respond_error(request, format!("书架数据损坏：{error}")),
    };
    let Some(book) = library
        .get("books")
        .and_then(|value| value.as_array())
        .and_then(|books| {
            books
                .iter()
                .find(|book| book.get("id").and_then(|id| id.as_str()) == Some(&request_data.id))
        })
    else {
        return respond_error(request, "找不到这本书".to_owned());
    };
    let Some(path) = book.get("path").and_then(|path| path.as_str()) else {
        return respond_error(request, "这是云端书目，尚未导入本地文件".to_owned());
    };
    let title = book
        .get("title")
        .and_then(|value| value.as_str())
        .unwrap_or("未命名");
    let author = book
        .get("author")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let format = book
        .get("format")
        .and_then(|value| value.as_str())
        .unwrap_or("epub");

    match open_path(Path::new(path), title, author, format, shared) {
        Ok(info) => {
            let body = serde_json::to_string(&info)?;
            respond_text(request, &body, "application/json; charset=utf-8")
        }
        Err(error) => respond_error(request, error.to_string()),
    }
}

fn open_path(
    path: &Path,
    title: &str,
    author: &str,
    format: &str,
    shared: &Shared,
) -> Result<BookInfo, AppError> {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or(format)
        .to_ascii_lowercase();
    match extension.as_str() {
        "epub" => {
            let bytes = fs::read(path)?;
            let mut info = load_book(&bytes, shared)?;
            info.kind = "epub".to_owned();
            info.title = if title.is_empty() { info.title } else { title.to_owned() };
            info.author = if author.is_empty() { info.author } else { author.to_owned() };
            *shared.book.lock().map_err(|_| "book lock poisoned")? = Some(info.clone());
            *shared.file_path.lock().map_err(|_| "path lock poisoned")? = None;
            Ok(info)
        }
        "pdf" => open_single_file(path, title, author, "pdf", shared),
        "txt" => {
            let bytes = fs::read(path)?;
            let text = String::from_utf8_lossy(&bytes);
            let mut html = String::from("<meta charset=\"utf-8\"><style>body{max-width:820px;margin:40px auto;padding:0 24px;font:18px/1.9 'Microsoft YaHei',sans-serif;color:#1d1d1b;background:#fff}p{white-space:pre-wrap;margin:0 0 1.1em}</style>");
            for paragraph in text.split("\n\n") {
                if !paragraph.trim().is_empty() {
                    html.push_str("<p>");
                    html.push_str(&escape_html(paragraph.trim()));
                    html.push_str("</p>");
                }
            }
            open_generated_html(&html, title, author, shared)
        }
        "fb2" => {
            let text = fs::read_to_string(path)?;
            let document = parse_xml(&text)?;
            let mut html = String::from("<meta charset=\"utf-8\"><style>body{max-width:820px;margin:40px auto;padding:0 24px;font:18px/1.9 'Microsoft YaHei',sans-serif;color:#1d1d1b;background:#fff}p{white-space:pre-wrap;margin:0 0 1.1em}h3{margin-top:1.6em}</style>");
            for node in document.descendants().filter(|node| node.is_element()) {
                match local_name(node) {
                    "title" => {
                        let value: String = node
                            .descendants()
                            .filter(|child| child.is_text())
                            .filter_map(|child| child.text())
                            .collect();
                        if !value.trim().is_empty() {
                            html.push_str("<h3>");
                            html.push_str(&escape_html(value.trim()));
                            html.push_str("</h3>");
                        }
                    }
                    "p" => {
                        let value: String = node
                            .descendants()
                            .filter(|child| child.is_text())
                            .filter_map(|child| child.text())
                            .collect();
                        if !value.trim().is_empty() {
                            html.push_str("<p>");
                            html.push_str(&escape_html(value.trim()));
                            html.push_str("</p>");
                        }
                    }
                    _ => {}
                }
            }
            open_generated_html(&html, title, author, shared)
        }
        "mobi" | "azw" | "azw3" => {
            let parsed = ebook_rs::Book::from_file(path)?;
            let metadata = parsed.metadata();
            let count = parsed.sections().len().max(parsed.spine().len());
            let mut html = String::from("<meta charset=\"utf-8\"><style>body{max-width:820px;margin:40px auto;padding:0 24px;font:18px/1.9 'Microsoft YaHei',sans-serif;color:#1d1d1b;background:#fff}p{white-space:pre-wrap;margin:0 0 1.1em}h3{margin-top:1.6em}</style>");
            for index in 0..count {
                let Ok(section) = parsed.get_section(index) else {
                    continue;
                };
                for paragraph in section.plain_text.split("\n\n") {
                    if !paragraph.trim().is_empty() {
                        html.push_str("<p>");
                        html.push_str(&escape_html(paragraph.trim()));
                        html.push_str("</p>");
                    }
                }
            }
            let generated_title = if title.is_empty() {
                metadata.title.trim().to_owned()
            } else {
                title.to_owned()
            };
            open_generated_html(&html, &generated_title, author, shared)
        }
        "cbz" => {
            let bytes = fs::read(path)?;
            let mut archive = zip::ZipArchive::new(Cursor::new(bytes))?;
            let image_dir = shared.root.join("book").join("cbz");
            if image_dir.exists() {
                fs::remove_dir_all(&image_dir)?;
            }
            fs::create_dir_all(&image_dir)?;
            let mut pages = Vec::new();
            for index in 0..archive.len() {
                let mut entry = archive.by_index(index)?;
                let name = entry.name()?.to_string();
                let extension = Path::new(&name)
                    .extension()
                    .and_then(|value| value.to_str())
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                if !matches!(extension.as_str(), "jpg" | "jpeg" | "png" | "webp" | "gif") {
                    continue;
                }
                let file_name = Path::new(&name)
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or("page");
                let target = image_dir.join(file_name);
                let mut output = fs::File::create(&target)?;
                std::io::copy(&mut entry, &mut output)?;
                pages.push(file_name.to_owned());
            }
            pages.sort();
            let mut html = String::from("<meta charset=\"utf-8\"><style>body{margin:0;background:#111}img{display:block;max-width:100%;margin:0 auto 8px}</style>");
            for page in &pages {
                html.push_str("<img src=\"/book/cbz/");
                html.push_str(&urlencoding::encode(page));
                html.push_str("\">");
            }
            open_generated_html(&html, title, author, shared)
        }
        other => Err(format!(
            "WebView2 版暂未接入 .{other} 格式；目前支持 EPUB、PDF、TXT、FB2、CBZ"
        )
        .into()),
    }
}

fn open_single_file(
    path: &Path,
    title: &str,
    author: &str,
    kind: &str,
    shared: &Shared,
) -> Result<BookInfo, AppError> {
    let info = BookInfo {
        kind: kind.to_owned(),
        title: title.to_owned(),
        author: author.to_owned(),
        spine: Vec::new(),
        toc: Vec::new(),
        spine_map: Vec::new(),
    };
    *shared.book.lock().map_err(|_| "book lock poisoned")? = Some(info.clone());
    *shared.file_path.lock().map_err(|_| "path lock poisoned")? = Some(path.to_path_buf());
    Ok(info)
}

fn open_generated_html(
    html: &str,
    title: &str,
    author: &str,
    shared: &Shared,
) -> Result<BookInfo, AppError> {
    let path = shared.root.join("generated.html");
    fs::write(&path, html)?;
    open_single_file(&path, title, author, "html", shared)
}

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn read_json_body(request: &mut tiny_http::Request) -> Result<serde_json::Value, AppError> {
    let mut body = String::new();
    request
        .as_reader()
        .take(16 * 1024 * 1024)
        .read_to_string(&mut body)?;
    Ok(serde_json::from_str(&body)?)
}

fn library_path(shared: &Shared) -> PathBuf {
    shared.data_dir.join("library.json")
}

fn load_library(shared: &Shared) -> serde_json::Value {
    fs::read_to_string(library_path(shared))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_else(|| {
            serde_json::json!({"version":2,"books":[],"bookmarks":[],"collections":[]})
        })
}

fn save_library(shared: &Shared, library: &serde_json::Value) -> Result<(), AppError> {
    fs::write(library_path(shared), serde_json::to_vec(library)?)?;
    Ok(())
}

fn save_progress(mut request: tiny_http::Request, shared: &Shared) -> Result<(), AppError> {
    let body = read_json_body(&mut request)?;
    let book_id = body.get("book_id").and_then(|v| v.as_str()).unwrap_or_default();
    if book_id.is_empty() {
        return respond_error(request, "缺少 book_id".to_owned());
    }
    let mut library = load_library(shared);
    let Some(book) = library
        .get_mut("books")
        .and_then(|v| v.as_array_mut())
        .and_then(|books| books.iter_mut().find(|book| book.get("id").and_then(|v| v.as_str()) == Some(book_id)))
    else {
        return respond_error(request, "找不到这本书".to_owned());
    };
    let chapter = body.get("chapter").and_then(|v| v.as_u64()).unwrap_or(0);
    let fraction = body.get("fraction").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let scroll_offset = body.get("scroll_offset").and_then(|v| v.as_f64()).unwrap_or(0.0);
    book["progress"] = serde_json::json!({
        "chapter": chapter,
        "fraction": fraction,
        "scroll_offset": scroll_offset,
        "updated_at": now_secs()
    });
    book["last_opened_at"] = serde_json::json!(now_secs());
    save_library(shared, &library)?;
    respond_text(request, "{\"ok\":true}", "application/json; charset=utf-8")
}

fn list_bookmarks(request: tiny_http::Request, shared: &Shared) -> Result<(), AppError> {
    let query = request.url().split_once('?').map_or("", |(_, query)| query);
    let book_id = query
        .split('&')
        .find_map(|pair| pair.strip_prefix("book_id="))
        .and_then(|value| urlencoding::decode(value).ok())
        .map_or_else(String::new, |value| value.into_owned());
    let library = load_library(shared);
    let bookmarks: Vec<&serde_json::Value> = library
        .get("bookmarks")
        .and_then(|v| v.as_array())
        .map_or_else(Vec::new, |items| {
            items
                .iter()
                .filter(|item| {
                    book_id.is_empty()
                        || item.get("book_id").and_then(|v| v.as_str()) == Some(&book_id)
                })
                .collect()
        });
    let body = serde_json::to_string(&bookmarks)?;
    respond_text(request, &body, "application/json; charset=utf-8")
}

fn add_bookmark(mut request: tiny_http::Request, shared: &Shared) -> Result<(), AppError> {
    let body = read_json_body(&mut request)?;
    let book_id = body.get("book_id").and_then(|v| v.as_str()).unwrap_or_default();
    if book_id.is_empty() {
        return respond_error(request, "缺少 book_id".to_owned());
    }
    let now = now_secs();
    let bookmark = serde_json::json!({
        "id": format!("bm-{now}-{book_id}"),
        "book_id": book_id,
        "chapter": body.get("chapter").and_then(|v| v.as_u64()).unwrap_or(0),
        "fraction": body.get("fraction").and_then(|v| v.as_f64()).unwrap_or(0.0),
        "label": body.get("label").and_then(|v| v.as_str()).unwrap_or("书签"),
        "created_at": now,
        "updated_at": now
    });
    let mut library = load_library(shared);
    if let Some(items) = library.get_mut("bookmarks").and_then(|v| v.as_array_mut()) {
        items.push(bookmark.clone());
    } else {
        library["bookmarks"] = serde_json::json!([bookmark]);
    }
    save_library(shared, &library)?;
    let text = serde_json::to_string(&bookmark)?;
    respond_text(request, &text, "application/json; charset=utf-8")
}

fn delete_bookmark(request: tiny_http::Request, shared: &Shared, id: &str) -> Result<(), AppError> {
    let id = urlencoding::decode(id)
        .map_or_else(|_| id.to_owned(), |value| value.into_owned());
    let mut library = load_library(shared);
    if let Some(items) = library.get_mut("bookmarks").and_then(|v| v.as_array_mut()) {
        items.retain(|item| item.get("id").and_then(|v| v.as_str()) != Some(&id));
    }
    save_library(shared, &library)?;
    respond_text(request, "{\"ok\":true}", "application/json; charset=utf-8")
}

fn create_folder(mut request: tiny_http::Request, shared: &Shared) -> Result<(), AppError> {
    let body = read_json_body(&mut request)?;
    let name = body.get("name").and_then(|v| v.as_str()).unwrap_or("").trim();
    if name.is_empty() {
        return respond_error(request, "文件夹名称不能为空".to_owned());
    }
    let now = now_secs();
    let folder = serde_json::json!({
        "id": format!("folder-{now}"),
        "name": name,
        "created_at": now,
        "updated_at": now,
        "deleted": false
    });
    let mut library = load_library(shared);
    if let Some(items) = library.get_mut("collections").and_then(|v| v.as_array_mut()) {
        items.push(folder.clone());
    } else {
        library["collections"] = serde_json::json!([folder]);
    }
    save_library(shared, &library)?;
    let text = serde_json::to_string(&folder)?;
    respond_text(request, &text, "application/json; charset=utf-8")
}

fn update_folder(mut request: tiny_http::Request, shared: &Shared, id: &str) -> Result<(), AppError> {
    let id = urlencoding::decode(id).map_or_else(|_| id.to_owned(), |value| value.into_owned());
    let body = read_json_body(&mut request)?;
    let name = body.get("name").and_then(|v| v.as_str()).unwrap_or("").trim();
    if name.is_empty() {
        return respond_error(request, "文件夹名称不能为空".to_owned());
    }
    let mut library = load_library(shared);
    let Some(folder) = library
        .get_mut("collections")
        .and_then(|v| v.as_array_mut())
        .and_then(|items| items.iter_mut().find(|item| item.get("id").and_then(|v| v.as_str()) == Some(&id)))
    else {
        return respond_error(request, "找不到文件夹".to_owned());
    };
    folder["name"] = serde_json::json!(name);
    folder["updated_at"] = serde_json::json!(now_secs());
    save_library(shared, &library)?;
    respond_text(request, "{\"ok\":true}", "application/json; charset=utf-8")
}

fn remove_folder(request: tiny_http::Request, shared: &Shared, id: &str) -> Result<(), AppError> {
    let id = urlencoding::decode(id).map_or_else(|_| id.to_owned(), |value| value.into_owned());
    let mut library = load_library(shared);
    if let Some(items) = library.get_mut("collections").and_then(|v| v.as_array_mut()) {
        for item in items {
            if item.get("id").and_then(|v| v.as_str()) == Some(&id) {
                item["deleted"] = serde_json::json!(true);
                item["updated_at"] = serde_json::json!(now_secs());
            }
        }
    }
    if let Some(books) = library.get_mut("books").and_then(|v| v.as_array_mut()) {
        for book in books {
            if book.get("collection_id").and_then(|v| v.as_str()) == Some(&id) {
                book["collection_id"] = serde_json::Value::Null;
            }
        }
    }
    save_library(shared, &library)?;
    respond_text(request, "{\"ok\":true}", "application/json; charset=utf-8")
}

fn move_book(mut request: tiny_http::Request, shared: &Shared) -> Result<(), AppError> {
    let body = read_json_body(&mut request)?;
    let book_id = body.get("book_id").and_then(|v| v.as_str()).unwrap_or_default();
    let collection_id = body.get("collection_id").and_then(|v| v.as_str());
    let mut library = load_library(shared);
    let Some(book) = library
        .get_mut("books")
        .and_then(|v| v.as_array_mut())
        .and_then(|books| books.iter_mut().find(|book| book.get("id").and_then(|v| v.as_str()) == Some(book_id)))
    else {
        return respond_error(request, "找不到这本书".to_owned());
    };
    book["collection_id"] = collection_id.map_or(serde_json::Value::Null, |value| serde_json::json!(value));
    save_library(shared, &library)?;
    respond_text(request, "{\"ok\":true}", "application/json; charset=utf-8")
}

fn settings_path(shared: &Shared) -> PathBuf {
    shared.data_dir.join("webview-settings.json")
}

fn get_settings(request: tiny_http::Request, shared: &Shared) -> Result<(), AppError> {
    let body = fs::read_to_string(settings_path(shared)).unwrap_or_else(|_| {
        "{\"font_size\":18,\"line_height\":1.8,\"theme\":\"dark\",\"font_family\":\"\"}".to_owned()
    });
    respond_text(request, &body, "application/json; charset=utf-8")
}

fn save_settings(mut request: tiny_http::Request, shared: &Shared) -> Result<(), AppError> {
    let body = read_json_body(&mut request)?;
    fs::write(settings_path(shared), serde_json::to_vec_pretty(&body)?)?;
    respond_text(request, "{\"ok\":true}", "application/json; charset=utf-8")
}

#[derive(Serialize, Deserialize, Clone)]
struct WebDavConfig {
    #[serde(default)]
    url: String,
    #[serde(default)]
    username: String,
    #[serde(default)]
    password: String,
    #[serde(default)]
    remote_dir: String,
    #[serde(default)]
    device_id: String,
    #[serde(default)]
    last_sync_at: Option<u64>,
    #[serde(default)]
    last_sync_status: Option<String>,
}

impl Default for WebDavConfig {
    fn default() -> Self {
        Self {
            url: String::new(),
            username: String::new(),
            password: String::new(),
            remote_dir: String::new(),
            device_id: format!("device-{:016x}", now_secs()),
            last_sync_at: None,
            last_sync_status: None,
        }
    }
}

fn webdav_path(shared: &Shared) -> PathBuf {
    shared.data_dir.join("webdav.json")
}

fn load_webdav(shared: &Shared) -> WebDavConfig {
    let mut config: WebDavConfig = fs::read_to_string(webdav_path(shared))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    if config.device_id.trim().is_empty() {
        config.device_id = format!("device-{:016x}", now_secs());
    }
    config
}

fn get_webdav(request: tiny_http::Request, shared: &Shared) -> Result<(), AppError> {
    let config = load_webdav(shared);
    let body = serde_json::to_string(&config)?;
    respond_text(request, &body, "application/json; charset=utf-8")
}

fn save_webdav(mut request: tiny_http::Request, shared: &Shared) -> Result<(), AppError> {
    let existing = load_webdav(shared);
    let mut config: WebDavConfig = serde_json::from_value(read_json_body(&mut request)?)?;
    if config.device_id.trim().is_empty() {
        config.device_id = if existing.device_id.trim().is_empty() {
            format!("device-{:016x}", now_secs())
        } else {
            existing.device_id
        };
    }
    config.last_sync_at = existing.last_sync_at;
    config.last_sync_status = existing.last_sync_status;
    fs::write(webdav_path(shared), serde_json::to_vec_pretty(&config)?)?;
    respond_text(request, "{\"ok\":true}", "application/json; charset=utf-8")
}

struct DavClient {
    client: reqwest::blocking::Client,
    base: reqwest::Url,
    username: String,
    password: String,
    remote_dir: String,
}

impl DavClient {
    fn new(config: &WebDavConfig) -> Result<Self, AppError> {
        if config.url.trim().is_empty() {
            return Err("还没有填写 WebDAV 地址".into());
        }
        let base = reqwest::Url::parse(config.url.trim())?;
        if base.scheme() != "http" && base.scheme() != "https" {
            return Err("WebDAV 地址只支持 http:// 或 https://".into());
        }
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()?;
        Ok(Self {
            client,
            base,
            username: config.username.trim().to_owned(),
            password: config.password.clone(),
            remote_dir: config.remote_dir.trim().replace('\\', "/"),
        })
    }

    fn endpoint(&self, file: Option<&str>) -> Result<reqwest::Url, AppError> {
        let mut url = self.base.clone();
        {
            let mut segments = url
                .path_segments_mut()
                .map_err(|()| "WebDAV 地址不能作为目录使用")?;
            for part in self
                .remote_dir
                .trim_matches('/')
                .split('/')
                .filter(|part| !part.is_empty() && *part != "." && *part != "..")
            {
                segments.push(part);
            }
            if let Some(file) = file {
                segments.push(file);
            }
        }
        Ok(url)
    }

    fn request(&self, method: reqwest::Method, url: reqwest::Url) -> reqwest::blocking::RequestBuilder {
        let request = self.client.request(method, url);
        if self.username.is_empty() {
            request
        } else {
            request.basic_auth(&self.username, Some(&self.password))
        }
    }

    fn ensure_directory(&self) -> Result<(), AppError> {
        let parts: Vec<&str> = self
            .remote_dir
            .trim_matches('/')
            .split('/')
            .filter(|part| !part.is_empty() && *part != "." && *part != "..")
            .collect();
        for end in 1..=parts.len() {
            let mut url = self.base.clone();
            {
                let mut segments = url
                    .path_segments_mut()
                    .map_err(|()| "WebDAV 地址不能作为目录使用")?;
                for part in &parts[..end] {
                    segments.push(part);
                }
            }
            let response = self
                .request(reqwest::Method::from_bytes(b"MKCOL")?, url)
                .send()?;
            if !response.status().is_success()
                && response.status() != reqwest::StatusCode::METHOD_NOT_ALLOWED
            {
                return Err(format!("创建远端目录失败：HTTP {}", response.status()).into());
            }
        }
        Ok(())
    }

    fn get_snapshot(&self) -> Result<Option<serde_json::Value>, AppError> {
        let response = self
            .request(reqwest::Method::GET, self.endpoint(Some("lumen-reader-sync.json"))?)
            .send()?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !response.status().is_success() {
            return Err(format!("下载远端进度失败：HTTP {}", response.status()).into());
        }
        let bytes = response.bytes()?;
        Ok(Some(serde_json::from_slice(&bytes)?))
    }

    fn put_snapshot(&self, snapshot: &serde_json::Value) -> Result<(), AppError> {
        let response = self
            .request(
                reqwest::Method::PUT,
                self.endpoint(Some("lumen-reader-sync.json"))?,
            )
            .header("Content-Type", "application/json; charset=utf-8")
            .body(serde_json::to_vec_pretty(snapshot)?)
            .send()?;
        if !response.status().is_success() {
            return Err(format!("上传远端进度失败：HTTP {}", response.status()).into());
        }
        Ok(())
    }

    fn test(&self) -> Result<String, AppError> {
        let response = self
            .request(reqwest::Method::from_bytes(b"PROPFIND")?, self.endpoint(None)?)
            .header("Depth", "0")
            .header("Content-Type", "application/xml; charset=utf-8")
            .body(r#"<?xml version="1.0" encoding="utf-8"?><d:propfind xmlns:d="DAV:"><d:prop><d:resourcetype/></d:prop></d:propfind>"#)
            .send()?;
        match response.status() {
            reqwest::StatusCode::MULTI_STATUS | reqwest::StatusCode::OK => Ok("连接成功".to_owned()),
            reqwest::StatusCode::NOT_FOUND => Ok("服务器可达，远端目录会在同步时创建".to_owned()),
            reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN => {
                Err("WebDAV 认证失败，请检查用户名和密码".into())
            }
            status => Err(format!("WebDAV 返回 HTTP {status}").into()),
        }
    }
}

fn test_webdav(request: tiny_http::Request, shared: &Shared) -> Result<(), AppError> {
    let config = load_webdav(shared);
    match DavClient::new(&config).and_then(|client| client.test()) {
        Ok(message) => {
            let body = serde_json::json!({"ok": true, "message": message}).to_string();
            respond_text(request, &body, "application/json; charset=utf-8")
        }
        Err(error) => respond_error(request, error.to_string()),
    }
}

fn sync_webdav(mut request: tiny_http::Request, shared: &Shared) -> Result<(), AppError> {
    let _ = read_json_body(&mut request);
    let mut config = load_webdav(shared);
    let client = match DavClient::new(&config) {
        Ok(client) => client,
        Err(error) => return respond_error(request, error.to_string()),
    };
    let result = (|| -> Result<(usize, usize, usize), AppError> {
        client.ensure_directory()?;
        let library = load_library(shared);
        let local = build_cloud_snapshot(&library, &config.device_id);
        let remote = client.get_snapshot()?;
        let merged = merge_cloud_snapshots(&local, remote.as_ref());
        client.put_snapshot(&merged)?;
        apply_cloud_snapshot(shared, &merged)?;
        Ok((
            merged.get("books").and_then(|v| v.as_array()).map_or(0, Vec::len),
            merged.get("bookmarks").and_then(|v| v.as_array()).map_or(0, Vec::len),
            merged.get("collections").and_then(|v| v.as_array()).map_or(0, Vec::len),
        ))
    })();
    match result {
        Ok((books, bookmarks, collections)) => {
            config.last_sync_at = Some(now_secs());
            config.last_sync_status = Some(format!("成功：{books} 本 / {bookmarks} 个书签 / {collections} 个文件夹"));
            let _ = fs::write(webdav_path(shared), serde_json::to_vec_pretty(&config).unwrap_or_default());
            let body = serde_json::json!({
                "ok": true,
                "books": books,
                "bookmarks": bookmarks,
                "collections": collections,
                "message": config.last_sync_status
            })
            .to_string();
            respond_text(request, &body, "application/json; charset=utf-8")
        }
        Err(error) => {
            config.last_sync_status = Some(format!("失败：{error}"));
            let _ = fs::write(webdav_path(shared), serde_json::to_vec_pretty(&config).unwrap_or_default());
            respond_error(request, error.to_string())
        }
    }
}

fn build_cloud_snapshot(library: &serde_json::Value, device_id: &str) -> serde_json::Value {
    let books: Vec<serde_json::Value> = library
        .get("books")
        .and_then(|v| v.as_array())
        .map_or_else(Vec::new, |books| {
            books
                .iter()
                .map(|book| {
                    serde_json::json!({
                        "id": book.get("id").cloned().unwrap_or(serde_json::Value::Null),
                        "title": book.get("title").cloned().unwrap_or(serde_json::Value::Null),
                        "author": book.get("author").cloned().unwrap_or(serde_json::Value::Null),
                        "collection_id": book.get("collection_id").cloned().unwrap_or(serde_json::Value::Null),
                        "format": book.get("format").cloned().unwrap_or(serde_json::Value::Null),
                        "chapter_count": book.get("chapter_count").cloned().unwrap_or(serde_json::json!(0)),
                        "progress": book.get("progress").cloned().unwrap_or_else(|| serde_json::json!({"chapter":0,"fraction":0.0,"scroll_offset":0.0,"updated_at":0})),
                        "added_at": book.get("added_at").cloned().unwrap_or(serde_json::json!(0)),
                        "last_opened_at": book.get("last_opened_at").cloned().unwrap_or(serde_json::Value::Null)
                    })
                })
                .collect()
        });
    serde_json::json!({
        "schema": 1,
        "device_id": device_id,
        "updated_at": now_secs(),
        "books": books,
        "bookmarks": library.get("bookmarks").cloned().unwrap_or_else(|| serde_json::json!([])),
        "collections": library.get("collections").cloned().unwrap_or_else(|| serde_json::json!([]))
    })
}

fn merge_cloud_snapshots(local: &serde_json::Value, remote: Option<&serde_json::Value>) -> serde_json::Value {
    let Some(remote) = remote else {
        return local.clone();
    };
    let mut books: std::collections::HashMap<String, serde_json::Value> = std::collections::HashMap::new();
    for book in local.get("books").and_then(|v| v.as_array()).into_iter().flatten() {
        if let Some(id) = book.get("id").and_then(|v| v.as_str()) {
            books.insert(id.to_owned(), book.clone());
        }
    }
    for incoming in remote.get("books").and_then(|v| v.as_array()).into_iter().flatten() {
        let Some(id) = incoming.get("id").and_then(|v| v.as_str()) else {
            continue;
        };
        if let Some(existing) = books.get_mut(id) {
            let local_updated = existing.get("progress").and_then(|v| v.get("updated_at")).and_then(|v| v.as_u64()).unwrap_or(0);
            let remote_updated = incoming.get("progress").and_then(|v| v.get("updated_at")).and_then(|v| v.as_u64()).unwrap_or(0);
            if remote_updated > local_updated {
                existing["progress"] = incoming.get("progress").cloned().unwrap_or(serde_json::Value::Null);
                existing["collection_id"] = incoming.get("collection_id").cloned().unwrap_or(serde_json::Value::Null);
            }
            if existing.get("collection_id").is_none() || existing.get("collection_id") == Some(&serde_json::Value::Null) {
                existing["collection_id"] = incoming.get("collection_id").cloned().unwrap_or(serde_json::Value::Null);
            }
            let local_count = existing.get("chapter_count").and_then(|v| v.as_u64()).unwrap_or(0);
            let remote_count = incoming.get("chapter_count").and_then(|v| v.as_u64()).unwrap_or(0);
            if remote_count > local_count {
                existing["chapter_count"] = serde_json::json!(remote_count);
            }
            let local_open = existing.get("last_opened_at").and_then(|v| v.as_u64()).unwrap_or(0);
            let remote_open = incoming.get("last_opened_at").and_then(|v| v.as_u64()).unwrap_or(0);
            if remote_open > local_open {
                existing["last_opened_at"] = incoming.get("last_opened_at").cloned().unwrap_or(serde_json::Value::Null);
            }
        } else {
            books.insert(id.to_owned(), incoming.clone());
        }
    }
    let mut bookmark_map: std::collections::HashMap<String, serde_json::Value> = std::collections::HashMap::new();
    for collection in [local.get("bookmarks"), remote.get("bookmarks")].into_iter().flatten().flat_map(|v| v.as_array()).flatten() {
        if let Some(id) = collection.get("id").and_then(|v| v.as_str()) {
            let current = bookmark_map.get(id);
            let current_at = current.and_then(|v| v.get("updated_at")).and_then(|v| v.as_u64()).unwrap_or(0);
            let incoming_at = collection.get("updated_at").and_then(|v| v.as_u64()).unwrap_or(0);
            if current.is_none() || incoming_at > current_at {
                bookmark_map.insert(id.to_owned(), collection.clone());
            }
        }
    }
    let mut folder_map: std::collections::HashMap<String, serde_json::Value> = std::collections::HashMap::new();
    for collection in [local.get("collections"), remote.get("collections")].into_iter().flatten().flat_map(|v| v.as_array()).flatten() {
        if let Some(id) = collection.get("id").and_then(|v| v.as_str()) {
            let current = folder_map.get(id);
            let current_at = current.and_then(|v| v.get("updated_at")).and_then(|v| v.as_u64()).unwrap_or(0);
            let incoming_at = collection.get("updated_at").and_then(|v| v.as_u64()).unwrap_or(0);
            if current.is_none() || incoming_at >= current_at {
                folder_map.insert(id.to_owned(), collection.clone());
            }
        }
    }
    let mut merged_books: Vec<_> = books.into_values().collect();
    merged_books.sort_by_key(|book| std::cmp::Reverse(book.get("last_opened_at").and_then(|v| v.as_u64()).unwrap_or(0)));
    serde_json::json!({
        "schema": 1,
        "device_id": local.get("device_id").cloned().unwrap_or(serde_json::Value::Null),
        "updated_at": now_secs(),
        "books": merged_books,
        "bookmarks": bookmark_map.into_values().collect::<Vec<_>>(),
        "collections": folder_map.into_values().collect::<Vec<_>>()
    })
}

fn apply_cloud_snapshot(shared: &Shared, snapshot: &serde_json::Value) -> Result<(), AppError> {
    let mut library = load_library(shared);
    for incoming in snapshot.get("books").and_then(|v| v.as_array()).into_iter().flatten() {
        let id = incoming.get("id").and_then(|v| v.as_str()).unwrap_or_default();
        if let Some(existing) = library
            .get_mut("books")
            .and_then(|v| v.as_array_mut())
            .and_then(|books| books.iter_mut().find(|book| book.get("id").and_then(|v| v.as_str()) == Some(id)))
        {
            existing["progress"] = incoming.get("progress").cloned().unwrap_or(serde_json::Value::Null);
            existing["collection_id"] = incoming.get("collection_id").cloned().unwrap_or(serde_json::Value::Null);
            existing["chapter_count"] = incoming.get("chapter_count").cloned().unwrap_or(serde_json::json!(0));
            existing["last_opened_at"] = incoming.get("last_opened_at").cloned().unwrap_or(serde_json::Value::Null);
        } else if let Some(books) = library.get_mut("books").and_then(|v| v.as_array_mut()) {
            books.push(serde_json::json!({
                "id": id,
                "title": incoming.get("title").cloned().unwrap_or(serde_json::Value::Null),
                "author": incoming.get("author").cloned().unwrap_or(serde_json::Value::Null),
                "collection_id": incoming.get("collection_id").cloned().unwrap_or(serde_json::Value::Null),
                "format": incoming.get("format").cloned().unwrap_or(serde_json::Value::Null),
                "path": serde_json::Value::Null,
                "file_size": 0,
                "cover_file": serde_json::Value::Null,
                "chapter_count": incoming.get("chapter_count").cloned().unwrap_or(serde_json::json!(0)),
                "progress": incoming.get("progress").cloned().unwrap_or(serde_json::Value::Null),
                "added_at": incoming.get("added_at").cloned().unwrap_or(serde_json::json!(0)),
                "last_opened_at": incoming.get("last_opened_at").cloned().unwrap_or(serde_json::Value::Null)
            }));
        }
    }
    library["bookmarks"] = snapshot.get("bookmarks").cloned().unwrap_or_else(|| serde_json::json!([]));
    library["collections"] = snapshot.get("collections").cloned().unwrap_or_else(|| serde_json::json!([]));
    save_library(shared, &library)
}

fn respond_error(request: tiny_http::Request, message: String) -> Result<(), AppError> {
    let body = serde_json::json!({ "error": message }).to_string();
    let mut response = Response::from_data(body.into_bytes()).with_status_code(400);
    if let Ok(header) = Header::from_bytes("Content-Type", "application/json; charset=utf-8") {
        response = response.with_header(header);
    }
    request.respond(response)?;
    Ok(())
}

fn serve_cover(request: tiny_http::Request, name: &str, shared: &Shared) -> Result<(), AppError> {
    let name = urlencoding::decode(name)
        .map_err(|_| "bad cover path")?
        .into_owned();
    if name.contains("..") || name.contains('/') || name.contains('\\') {
        request.respond(Response::from_string("bad path").with_status_code(400))?;
        return Ok(());
    }
    let path = shared.data_dir.join("covers").join(name);
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(_) => {
            request.respond(Response::from_string("not found").with_status_code(404))?;
            return Ok(());
        }
    };
    let mut response = Response::from_data(bytes);
    if let Ok(header) = Header::from_bytes("Content-Type", mime_type(&path)) {
        response = response.with_header(header);
    }
    request.respond(response)?;
    Ok(())
}

fn serve_current_file(request: tiny_http::Request, shared: &Shared) -> Result<(), AppError> {
    let guard = shared.file_path.lock().map_err(|_| "path lock poisoned")?;
    let Some(path) = guard.as_ref().cloned() else {
        request.respond(Response::from_string("not found").with_status_code(404))?;
        return Ok(());
    };
    drop(guard);
    let bytes = fs::read(&path)?;
    let mut response = Response::from_data(bytes);
    if let Ok(header) = Header::from_bytes("Content-Type", mime_type(&path)) {
        response = response.with_header(header);
    }
    request.respond(response)?;
    Ok(())
}

fn serve_book_file(
    request: tiny_http::Request,
    encoded: &str,
    shared: &Shared,
) -> Result<(), AppError> {
    let decoded = urlencoding::decode(encoded)
        .map_err(|_| "bad path encoding")?
        .into_owned();
    let relative = Path::new(&decoded);
    let mut output = shared.root.join("book");
    for component in relative.components() {
        match component {
            Component::Normal(part) => output.push(part),
            Component::CurDir => {}
            _ => {
                request.respond(Response::from_string("bad path").with_status_code(400))?;
                return Ok(());
            }
        }
    }
    let bytes = match fs::read(&output) {
        Ok(bytes) => bytes,
        Err(_) => {
            request.respond(Response::from_string("not found").with_status_code(404))?;
            return Ok(());
        }
    };
    let mut response = Response::from_data(bytes);
    if let Ok(header) = Header::from_bytes("Content-Type", mime_type(&output)) {
        response = response.with_header(header);
    }
    request.respond(response)?;
    Ok(())
}

fn mime_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "html" | "htm" => "text/html; charset=utf-8",
        "xhtml" => "application/xhtml+xml; charset=utf-8",
        "pdf" => "application/pdf",
        "css" => "text/css; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "json" => "application/json",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "otf" => "font/otf",
        "ttf" => "font/ttf",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "mp3" => "audio/mpeg",
        "mp4" => "video/mp4",
        _ => "application/octet-stream",
    }
}

fn extract_zip(bytes: &[u8], destination: &Path) -> Result<(), AppError> {
    let cursor = Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(cursor)?;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let name = entry.name()?.to_string();
        let mut path = destination.to_path_buf();
        let mut unsafe_path = false;
        for component in Path::new(&name).components() {
            match component {
                Component::Normal(part) => path.push(part),
                Component::CurDir => {}
                _ => unsafe_path = true,
            }
        }
        if unsafe_path || name.ends_with('/') {
            continue;
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut output = fs::File::create(path)?;
        std::io::copy(&mut entry, &mut output)?;
    }
    Ok(())
}

fn parse_epub(root: &Path) -> Result<BookInfo, AppError> {
    let container_path = root.join("META-INF/container.xml");
    let container = fs::read_to_string(&container_path)?;
    let container_xml = parse_xml(&container)?;
    let opf_path = container_xml
        .descendants()
        .find(|node| node.is_element() && local_name(*node) == "rootfile")
        .and_then(|node| attribute(node, "full-path"))
        .ok_or("EPUB 缺少 OPF 路径")?;
    let opf_full = root.join(opf_path);
    let opf_text = fs::read_to_string(&opf_full)?;
    let opf = parse_xml(&opf_text)?;

    let mut title = "未命名".to_owned();
    let mut author = String::new();
    for node in opf.descendants().filter(|node| node.is_element()) {
        match local_name(node) {
            "title" if title == "未命名" => {
                title = node.text().unwrap_or_default().trim().to_owned();
            }
            "creator" if author.is_empty() => {
                author = node.text().unwrap_or_default().trim().to_owned();
            }
            _ => {}
        }
    }

    let opf_dir = Path::new(opf_path).parent().unwrap_or(Path::new("")).to_path_buf();
    let mut manifest: std::collections::HashMap<String, (String, String)> =
        std::collections::HashMap::new();
    for node in opf
        .descendants()
        .filter(|node| node.is_element() && local_name(*node) == "item")
    {
        let Some(id) = attribute(node, "id") else { continue };
        let href = attribute(node, "href").unwrap_or_default().to_owned();
        let media_type = attribute(node, "media-type").unwrap_or_default().to_owned();
        manifest.insert(id.to_owned(), (href, media_type));
    }

    let mut spine = Vec::new();
    for node in opf
        .descendants()
        .filter(|node| node.is_element() && local_name(*node) == "itemref")
    {
        let Some(idref) = attribute(node, "idref") else { continue };
        let Some((href, _)) = manifest.get(idref) else { continue };
        let href = resolve_href(&opf_dir, href);
        spine.push(SpineItem {
            title: file_title(&href),
            href,
        });
    }
    if spine.is_empty() {
        return Err("EPUB spine 为空".into());
    }

    let mut toc = Vec::new();
    let nav_href = opf
        .descendants()
        .filter(|node| node.is_element() && local_name(*node) == "item")
        .find(|node| {
            attribute(*node, "properties")
                .is_some_and(|properties| properties.split_whitespace().any(|value| value == "nav"))
        })
        .and_then(|node| attribute(node, "href"))
        .map(|href| resolve_href(&opf_dir, href));

    if let Some(nav_href) = nav_href {
        if let Ok(nav_text) = fs::read_to_string(root.join(&nav_href)) {
            if let Ok(nav) = parse_xml(&nav_text) {
                let nav_dir = Path::new(&nav_href)
                    .parent()
                    .unwrap_or(Path::new(""))
                    .to_path_buf();
                for link in nav.descendants().filter(|node| {
                    node.is_element() && local_name(*node) == "a" && attribute(*node, "href").is_some()
                }) {
                    let Some(raw_href) = attribute(link, "href") else { continue };
                    let (raw_path, fragment) = raw_href
                        .split_once('#')
                        .map_or((raw_href, ""), |(path, fragment)| (path, fragment));
                    if raw_path.trim().is_empty() {
                        continue;
                    }
                    let href = resolve_href(&nav_dir, raw_path);
                    let Some(spine_index) = match_spine(&spine, &href, raw_path) else {
                        continue;
                    };
                    let title = link
                        .descendants()
                        .filter(|node| node.is_text())
                        .filter_map(|node| node.text())
                        .collect::<String>()
                        .trim()
                        .to_owned();
                    toc.push(TocItem {
                        title,
                        href,
                        fragment: fragment.to_owned(),
                        spine: spine_index,
                    });
                }
            }
        }
    }

    // Many older EPUB 2 books ship only toc.ncx (no XHTML nav document).
    // Without this fallback the sidebar shows raw file names such as
    // "summary.xhtml" / "Section02", even though the NCX contains Chinese
    // chapter labels.
    if toc.is_empty() {
        let ncx_href = manifest
            .values()
            .find(|(_, media_type)| media_type == "application/x-dtbncx+xml")
            .map(|(href, _)| resolve_href(&opf_dir, href));
        if let Some(ncx_href) = ncx_href {
            if let Ok(ncx_text) = fs::read_to_string(root.join(&ncx_href)) {
                if let Ok(ncx) = parse_xml(&ncx_text) {
                    let ncx_dir = Path::new(&ncx_href)
                        .parent()
                        .unwrap_or(Path::new(""))
                        .to_path_buf();
                    for point in ncx
                        .descendants()
                        .filter(|node| node.is_element() && local_name(*node) == "navPoint")
                    {
                        let Some(raw_src) = point
                            .children()
                            .filter(|node| node.is_element() && local_name(*node) == "content")
                            .find_map(|node| attribute(node, "src"))
                        else {
                            continue;
                        };
                        let (raw_path, fragment) = raw_src
                            .split_once('#')
                            .map_or((raw_src, ""), |(path, fragment)| (path, fragment));
                        let title: String = point
                            .children()
                            .filter(|node| node.is_element() && local_name(*node) == "navLabel")
                            .flat_map(|label| label.descendants())
                            .filter(|node| node.is_text())
                            .filter_map(|node| node.text())
                            .collect::<String>()
                            .trim()
                            .to_owned();
                        if title.is_empty() {
                            continue;
                        }
                        let href = resolve_href(&ncx_dir, raw_path);
                        let Some(spine_index) = match_spine(&spine, &href, raw_path) else {
                            continue;
                        };
                        toc.push(TocItem {
                            title,
                            href,
                            fragment: fragment.to_owned(),
                            spine: spine_index,
                        });
                    }
                }
            }
        }
    }

    for (index, item) in spine.iter_mut().enumerate() {
        if let Some(entry) = toc.iter().find(|entry| entry.spine == index) {
            if !entry.title.is_empty() {
                item.title = entry.title.clone();
            }
        }
    }
    if toc.is_empty() {
        toc = spine
            .iter()
            .enumerate()
            .map(|(index, item)| TocItem {
                title: item.title.clone(),
                href: item.href.clone(),
                fragment: String::new(),
                spine: index,
            })
            .collect();
    }

    let mut spine_map = (0..spine.len()).collect::<Vec<_>>();
    consolidate_volume_color_pages(root, &mut spine, &mut toc, &mut spine_map)?;

    Ok(BookInfo {
        kind: "epub".to_owned(),
        title: if title.is_empty() { "未命名".to_owned() } else { title },
        author,
        spine,
        toc,
        spine_map,
    })
}

/// Combine all image-only pages into one continuous 彩页 document.
fn consolidate_color_pages(
    root: &Path,
    spine: &mut Vec<SpineItem>,
    toc: &mut Vec<TocItem>,
) -> Result<(), AppError> {
    let mut debug = Vec::new();
    let color_pages: Vec<(usize, Vec<String>)> = spine
        .iter()
        .enumerate()
        .filter_map(|(index, item)| {
            let stem = Path::new(&item.href)
                .file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase();
            if stem == "cover" || item.title.contains("封面") {
                debug.push(format!("{index} {} skipped-cover", item.href));
                return None;
            }
            let text = match fs::read_to_string(root.join(&item.href)) {
                Ok(text) => text,
                Err(error) => {
                    debug.push(format!("{index} {} read-error {error}", item.href));
                    return None;
                }
            };
            let doc = match parse_xml(&text) {
                Ok(doc) => doc,
                Err(error) => {
                    debug.push(format!("{index} {} parse-error {error}", item.href));
                    return None;
                }
            };
            let visible_text: String = doc
                .descendants()
                .filter(|node| node.is_text())
                .filter_map(|node| node.text())
                .collect::<String>();
            let page_dir = Path::new(&item.href).parent().unwrap_or(Path::new(""));
            let mut images = Vec::new();
            for node in doc.descendants().filter(|node| node.is_element()) {
                let raw = match local_name(node) {
                    "img" => attribute(node, "src"),
                    "image" => node
                        .attributes()
                        .find(|attribute| attribute.name() == "href")
                        .map(|attribute| attribute.value()),
                    _ => None,
                };
                let Some(raw) = raw else { continue };
                if raw.starts_with("data:") || raw.trim().is_empty() {
                    continue;
                }
                let href = resolve_href(page_dir, raw);
                if !root.join(&href).is_file() {
                    continue;
                }
                let encoded = href
                    .split('/')
                    .map(|part| urlencoding::encode(part).into_owned())
                    .collect::<Vec<_>>()
                    .join("/");
                if !images.iter().any(|existing| existing == &encoded) {
                    images.push(encoded);
                }
            }
            debug.push(format!(
                "{index} {} stem={stem} text={} images={}",
                item.href,
                visible_text.trim().len(),
                images.len()
            ));
            if !visible_text.trim().is_empty() {
                return None;
            }
            (!images.is_empty()).then_some((index, images))
        })
        .collect();

    if color_pages.is_empty() {
        return Ok(());
    }

    let href = "lumen-color-pages.html";
    let mut html = String::from(
        "<meta charset=\"utf-8\"><style>html,body{margin:0;padding:0;background:#111}body{display:grid;grid-template-columns:1fr 1fr;gap:4px}  .lumen-color-page{display:block;width:100%;max-width:100%;height:auto;margin:0;padding:0;border:0}</style>",
    );
    for (_, images) in &color_pages {
        for image in images {
            html.push_str("<img class=\"lumen-color-page\" src=\"/book/");
            html.push_str(image);
            html.push_str("\">");
        }
    }
    fs::write(root.join(href), html)?;

    let first_color = color_pages[0].0;
    let color_indices: Vec<usize> = color_pages.iter().map(|(index, _)| *index).collect();
    let insert_at = first_color;
    let mut old_to_new = vec![0_usize; spine.len()];
    let mut merged_spine = Vec::with_capacity(spine.len() - color_indices.len() + 1);
    let mut merged_color_index = insert_at;
    let mut color_inserted = false;
    for (index, item) in spine.iter().enumerate() {
        if color_indices.contains(&index) {
            continue;
        }
        if !color_inserted && merged_spine.len() >= insert_at {
            merged_color_index = merged_spine.len();
            merged_spine.push(SpineItem {
                href: href.to_owned(),
                title: "彩页".to_owned(),
            });
            color_inserted = true;
        }
        old_to_new[index] = merged_spine.len();
        merged_spine.push(item.clone());
    }
    if !color_inserted {
        merged_color_index = merged_spine.len();
        merged_spine.push(SpineItem {
            href: href.to_owned(),
            title: "彩页".to_owned(),
        });
    }
    for index in &color_indices {
        old_to_new[*index] = merged_color_index;
    }
    *spine = merged_spine;

    if !toc.is_empty() {
        let mut merged_toc = Vec::new();
        let mut toc_color_inserted = false;
        for item in toc.iter() {
            if color_indices.contains(&item.spine) {
                continue;
            }
            let new_spine = old_to_new.get(item.spine).copied().unwrap_or(item.spine);
            if !toc_color_inserted && new_spine > merged_color_index {
                merged_toc.push(TocItem {
                    title: "彩页".to_owned(),
                    href: href.to_owned(),
                    fragment: String::new(),
                    spine: merged_color_index,
                });
                toc_color_inserted = true;
            }
            let mut item = item.clone();
            item.spine = new_spine.min(spine.len().saturating_sub(1));
            merged_toc.push(item);
        }
        if !toc_color_inserted {
            merged_toc.push(TocItem {
                title: "彩页".to_owned(),
                href: href.to_owned(),
                fragment: String::new(),
                spine: merged_color_index.min(spine.len().saturating_sub(1)),
            });
        }
        *toc = merged_toc;
    }
    Ok(())
}

/// Combine inline full-page illustration blocks (`divimage`, `illus`, ...)
/// into the same 彩页 gallery. Some EPUBs do not use separate illustration
/// documents; they put all colour plates at the start of chapter XHTML files.
fn consolidate_inline_color_images(
    root: &Path,
    spine: &mut Vec<SpineItem>,
    toc: &mut Vec<TocItem>,
) -> Result<(), AppError> {
    if spine
        .iter()
        .any(|item| item.href == "lumen-color-pages.html" || item.title == "彩页")
    {
        return Ok(());
    }
    let mut images = Vec::new();
    for item in spine.iter() {
        let Ok(text) = fs::read_to_string(root.join(&item.href)) else {
            continue;
        };
        let Ok(doc) = parse_xml(&text) else {
            continue;
        };
        let page_dir = Path::new(&item.href).parent().unwrap_or(Path::new(""));
        for node in doc.descendants().filter(|node| node.is_element()) {
            let is_illustration = attribute(node, "class").is_some_and(|classes| {
                classes.split_whitespace().any(|class| {
                    matches!(
                        class.to_ascii_lowercase().as_str(),
                        "divimage" | "illus" | "duokan-image-single" | "color-page"
                    )
                })
            });
            if !is_illustration {
                continue;
            }
            for image_node in node.descendants().filter(|child| {
                child.is_element() && matches!(local_name(*child), "img" | "image")
            }) {
                let Some(raw) = attribute(image_node, "src") else {
                    continue;
                };
                let href = resolve_href(page_dir, raw);
                if !root.join(&href).is_file() {
                    continue;
                }
                let encoded = href
                    .split('/')
                    .map(|part| urlencoding::encode(part).into_owned())
                    .collect::<Vec<_>>()
                    .join("/");
                if !images.iter().any(|existing| existing == &encoded) {
                    images.push(encoded);
                }
            }
        }
    }
    if images.len() < 2 {
        return Ok(());
    }
    let href = "lumen-color-pages.html";
    let mut html = String::from(
        "<meta charset=\"utf-8\"><style>html,body{margin:0;padding:0;background:#111}body{display:grid;grid-template-columns:1fr 1fr;gap:4px}  .lumen-color-page{display:block;width:100%;max-width:100%;height:auto;margin:0;padding:0;border:0}</style>",
    );
    for image in &images {
        html.push_str("<img class=\"lumen-color-page\" src=\"/book/");
        html.push_str(image);
        html.push_str("\">");
    }
    fs::write(root.join(href), html)?;

    let color_index = 0;
    spine.insert(
        0,
        SpineItem {
            href: href.to_owned(),
            title: "彩页".to_owned(),
        },
    );
    if !toc.is_empty() {
        for item in toc.iter_mut() {
            item.spine = item.spine.saturating_add(1);
        }
        toc.insert(
            0,
            TocItem {
                title: "彩页".to_owned(),
                href: href.to_owned(),
                fragment: String::new(),
                spine: color_index,
            },
        );
    }
    Ok(())
}

/// Move only the leading illustration block of each volume into a per-volume
/// colour gallery. Later illustrations stay inline in their original chapter.
fn consolidate_volume_color_pages(
    root: &Path,
    spine: &mut Vec<SpineItem>,
    toc: &mut Vec<TocItem>,
    spine_map: &mut Vec<usize>,
) -> Result<(), AppError> {
    let mut volumes = Vec::new();
    for (index, item) in spine.iter().enumerate() {
        if item.href.starts_with("lumen-color") {
            continue;
        }
        let stem = Path::new(&item.href)
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if stem == "cover" || item.title.contains("封面") {
            continue;
        }
        let Ok(text) = fs::read_to_string(root.join(&item.href)) else {
            continue;
        };
        let (images, modified) = extract_leading_illustrations(&text, &item.href, root);
        if images.is_empty() || modified == text {
            continue;
        }
        fs::write(root.join(&item.href), modified)?;
        let href = format!("lumen-color-volume-{index}.html");
        let mut html = String::from(
            "<meta charset=\"utf-8\"><style>html,body{margin:0;padding:0;background:#111}body{display:grid;grid-template-columns:1fr 1fr;gap:4px}  .lumen-color-page{display:block;width:100%;max-width:100%;height:auto;margin:0;padding:0;border:0}</style>",
        );
        for image in &images {
            html.push_str("<img class=\"lumen-color-page\" src=\"/book/");
            html.push_str(image);
            html.push_str("\">");
        }
        fs::write(root.join(&href), html)?;
        volumes.push((index, href, item.title.clone()));
    }
    if volumes.is_empty() {
        return Ok(());
    }

    let mut merged_spine = Vec::with_capacity(spine.len() + volumes.len());
    let mut old_to_new = vec![0_usize; spine.len()];
    for (index, item) in spine.iter().enumerate() {
        if let Some((_, href, title)) = volumes.iter().find(|(volume, _, _)| *volume == index) {
            merged_spine.push(SpineItem {
                href: href.clone(),
                title: format!("彩页 · {title}"),
            });
        }
        old_to_new[index] = merged_spine.len();
        merged_spine.push(item.clone());
    }
    let mut merged_toc = Vec::new();
    let mut inserted = vec![false; volumes.len()];
    for entry in toc.iter() {
        for (volume_number, (volume, href, title)) in volumes.iter().enumerate() {
            if !inserted[volume_number] && entry.spine >= *volume {
                merged_toc.push(TocItem {
                    title: format!("彩页 · {title}"),
                    href: href.clone(),
                    fragment: String::new(),
                    spine: old_to_new[*volume].saturating_sub(1),
                });
                inserted[volume_number] = true;
            }
        }
        let mut entry = entry.clone();
        entry.spine = old_to_new.get(entry.spine).copied().unwrap_or(entry.spine);
        merged_toc.push(entry);
    }
    for (volume_number, (volume, href, title)) in volumes.iter().enumerate() {
        if !inserted[volume_number] {
            merged_toc.push(TocItem {
                title: format!("彩页 · {title}"),
                href: href.clone(),
                fragment: String::new(),
                spine: old_to_new[*volume].saturating_sub(1),
            });
        }
    }
    *spine = merged_spine;
    *toc = merged_toc;
    *spine_map = old_to_new;
    Ok(())
}

fn extract_leading_illustrations(
    text: &str,
    page_href: &str,
    root: &Path,
) -> (Vec<String>, String) {
    let Some(body_start) = text.find("<body") else {
        return (Vec::new(), text.to_owned());
    };
    let Some(body_tag_end) = text[body_start..].find('>').map(|offset| body_start + offset + 1) else {
        return (Vec::new(), text.to_owned());
    };
    let mut cursor = body_tag_end;
    let page_dir = Path::new(page_href).parent().unwrap_or(Path::new(""));
    let mut ranges = Vec::new();
    let mut images = Vec::new();
    loop {
        while cursor < text.len() && text.as_bytes()[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if text[cursor..].starts_with("<h1")
            || text[cursor..].starts_with("<h2")
            || text[cursor..].starts_with("<h3")
            || text[cursor..].starts_with("<h4")
            || text[cursor..].starts_with("<h5")
            || text[cursor..].starts_with("<h6")
        {
            let tag = &text[cursor..cursor + 3];
            if let Some(close) = text[cursor..].find(&format!("</{tag}>")) {
                cursor += close + tag.len() + 2;
                continue;
            }
            break;
        }
        if text[cursor..].starts_with("<br") || text[cursor..].starts_with("<hr") {
            if let Some(end) = text[cursor..].find('>') {
                cursor += end + 1;
                continue;
            }
            break;
        }
        if !text[cursor..].starts_with("<div") {
            break;
        }
        let Some(tag_end) = text[cursor..].find('>').map(|offset| cursor + offset + 1) else {
            break;
        };
        let opening = &text[cursor..tag_end];
        let class = extract_attribute(opening, "class").unwrap_or_default();
        let classes = class.to_ascii_lowercase();
        if classes
            .split_whitespace()
            .any(|value| value == "chaptercontent")
        {
            cursor = tag_end;
            continue;
        }
        if classes.split_whitespace().any(|value| {
            matches!(
                value,
                "volumetitle" | "chaptertitle" | "titlepage" | "coverpage"
            )
        }) {
            if let Some(after) = skip_div_block(text, cursor) {
                cursor = after;
                continue;
            }
            break;
        }
        if !class.split_whitespace().any(|value| {
            matches!(
                value.to_ascii_lowercase().as_str(),
                "divimage" | "illus" | "duokan-image-single" | "color-page"
            )
        }) {
            break;
        }
        let Some(close) = text[tag_end..].find("</div>").map(|offset| tag_end + offset + 6) else {
            break;
        };
        let block = &text[cursor..close];
        let Some(raw) = extract_attribute(block, "src") else {
            break;
        };
        let href = resolve_href(page_dir, raw);
        if !root.join(&href).is_file() {
            break;
        }
        let encoded = href
            .split('/')
            .map(|part| urlencoding::encode(part).into_owned())
            .collect::<Vec<_>>()
            .join("/");
        if !images.iter().any(|existing| existing == &encoded) {
            images.push(encoded);
        }
        ranges.push((cursor, close));
        cursor = close;
    }
    if ranges.is_empty() {
        return (Vec::new(), text.to_owned());
    }
    let mut modified = String::with_capacity(text.len());
    let mut last = 0_usize;
    for (start, end) in ranges {
        modified.push_str(&text[last..start]);
        last = end;
    }
    modified.push_str(&text[last..]);
    (images, modified)
}

fn extract_attribute<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    for quote in ['"', '\''] {
        let needle = format!("{name}={quote}");
        if let Some(start) = text.find(&needle) {
            let value_start = start + needle.len();
            if let Some(end) = text[value_start..].find(quote) {
                return Some(&text[value_start..value_start + end]);
            }
        }
    }
    None
}

fn skip_div_block(text: &str, start: usize) -> Option<usize> {
    let mut depth = 0_i32;
    let mut cursor = start;
    loop {
        let next_open = text[cursor..].find("<div").map(|offset| cursor + offset);
        let next_close = text[cursor..].find("</div>").map(|offset| cursor + offset);
        match (next_open, next_close) {
            (Some(open), Some(close)) if open < close => {
                depth += 1;
                cursor = open + 4;
            }
            (_, Some(close)) => {
                depth -= 1;
                cursor = close + 6;
                if depth <= 0 {
                    return Some(cursor);
                }
            }
            _ => return None,
        }
    }
}

fn parse_xml(text: &str) -> Result<roxmltree::Document<'_>, roxmltree::Error> {
    roxmltree::Document::parse_with_options(
        text,
        roxmltree::ParsingOptions {
            allow_dtd: true,
            ..roxmltree::ParsingOptions::default()
        },
    )
}

fn local_name<'a, 'input>(node: roxmltree::Node<'a, 'input>) -> &'a str {
    node.tag_name().name()
}

fn attribute<'a, 'input>(node: roxmltree::Node<'a, 'input>, name: &str) -> Option<&'a str> {
    node.attributes()
        .find(|attribute| attribute.name() == name)
        .map(|attribute| attribute.value())
}

fn match_spine(spine: &[SpineItem], href: &str, raw_path: &str) -> Option<usize> {
    spine
        .iter()
        .position(|item| item.href == href)
        .or_else(|| {
            spine
                .iter()
                .position(|item| item.href.replace('\\', "/").ends_with(&format!("/{raw_path}")))
        })
        .or_else(|| {
            let wanted = Path::new(raw_path)
                .file_name()
                .and_then(|value| value.to_str())?;
            spine.iter().position(|item| {
                Path::new(&item.href)
                    .file_name()
                    .and_then(|value| value.to_str())
                    == Some(wanted)
            })
        })
}

fn resolve_href(base: &Path, href: &str) -> String {
    let href = href.split('#').next().unwrap_or(href);
    let mut path = base.to_path_buf();
    for component in Path::new(href).components() {
        match component {
            Component::Normal(part) => path.push(part),
            Component::ParentDir => {
                path.pop();
            }
            Component::CurDir => {}
            _ => {}
        }
    }
    path.to_string_lossy().replace('\\', "/")
}

fn file_title(href: &str) -> String {
    Path::new(href)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(href)
        .to_owned()
}
