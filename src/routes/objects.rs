// // routes/objects.rs

use actix_web::{http::header, web, HttpRequest, HttpResponse, Result};
use futures_util::StreamExt;
use std::path::{Component, Path, PathBuf};
use tokio::{
    fs,
    fs::File,
    io::{ AsyncReadExt, AsyncSeekExt, AsyncWriteExt},
};
use tokio_util::io::ReaderStream;

use crate::{AppState, consts::Config};
use crate::consts::PATH_OBJECTS;
use crate::auth::{NeedWrite, NeedRead, NeedList}; // ← add

pub(crate) fn init(cfg: &mut web::ServiceConfig) {
    cfg
        .route(format!("/{}", PATH_OBJECTS).as_str(), web::get().to(list_objects))
        .service(
            web::resource(format!("/{}/{{key:.+}}", PATH_OBJECTS).as_str())
                .route(web::put().to(put_object))
                .route(web::head().to(head_object))
                .route(web::get().to(get_object))
                .route(web::delete().to(delete_object)),
        );
}

/* ---------- helpers (private) ---------- */

fn resolve_key(root: &Path, key: &str) -> Option<PathBuf> {
    let mut cleaned = PathBuf::new();
    for comp in Path::new(key).components() {
        match comp {
            Component::Normal(s) => cleaned.push(s),
            _ => return None,
        }
    }
    if cleaned.as_os_str().is_empty() { None } else { Some(root.join(cleaned)) }
}

fn guess_content_type(key: &str) -> &'static str {
    match Path::new(key).extension().and_then(|s| s.to_str()).map(|s| s.to_ascii_lowercase()) {
        Some(ref ext) if ext == "png" => "image/png",
        Some(ref ext) if ext == "jpg" || ext == "jpeg" => "image/jpeg",
        Some(ref ext) if ext == "gif" => "image/gif",
        Some(ref ext) if ext == "webp" => "image/webp",
        Some(ref ext) if ext == "svg" => "image/svg+xml",
        Some(ref ext) if ext == "txt" => "text/plain; charset=utf-8",
        Some(ref ext) if ext == "json" => "application/json",
        Some(ref ext) if ext == "html" => "text/html; charset=utf-8",
        Some(ref ext) if ext == "css" => "text/css; charset=utf-8",
        Some(ref ext) if ext == "js" => "application/javascript",
        Some(ref ext) if ext == "pdf" => "application/pdf",
        Some(ref ext) if ext == "mp4" => "video/mp4",
        Some(ref ext) if ext == "mp3" => "audio/mpeg",
        Some(ref ext) if ext == "wav" => "audio/wav",
        _ => "application/octet-stream",
    }
}

fn make_etag(meta: &std::fs::Metadata) -> String {
    let len = meta.len();
    let ts = meta.modified().ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| (d.as_secs(), d.subsec_nanos()))
        .unwrap_or((0, 0));
    format!("W/\"{}-{}-{}\"", len, ts.0, ts.1)
}

fn parse_range(h: &str, total: u64) -> Option<(u64, u64)> {
    let s = h.trim();
    if !s.starts_with("bytes=") { return None; }
    let spec = &s[6..];
    if spec.contains(',') { return None; }
    let parts: Vec<&str> = spec.split('-').collect();
    if parts.len() != 2 { return None; }

    match (parts[0], parts[1]) {
        ("", n_str) => {
            let n = n_str.parse::<u64>().ok()?;
            if n == 0 || total == 0 { return None; }
            let n = n.min(total);
            let start = total - n;
            let end = total - 1;
            Some((start, end))
        }
        (start_str, "") => {
            let start = start_str.parse::<u64>().ok()?;
            if start >= total { return None; }
            Some((start, total - 1))
        }
        (start_str, end_str) => {
            let start = start_str.parse::<u64>().ok()?;
            let end = end_str.parse::<u64>().ok()?;
            if start > end || end >= total { return None; }
            Some((start, end))
        }
    }
}

/* ---------- types (private) ---------- */

#[derive(serde::Deserialize)]
struct ListQuery {
    prefix: Option<String>,
    recursive: Option<u8>,
}

#[derive(serde::Serialize)]
struct ListedObject {
    key: String,
    size: u64,
    modified: u64,
}

#[derive(serde::Deserialize)]
struct GetQuery {
    download: Option<u8>,
}

/* ---------- handlers (private) ---------- */

async fn put_object(
    _auth: NeedWrite,                 // ← enforce write
    req: HttpRequest,
    state: web::Data<AppState>,
    cfg: web::Data<Config>,
    key: web::Path<String>,
    mut body: web::Payload,
) -> Result<HttpResponse> {
    println!("→ PUT /{}/{}", PATH_OBJECTS, key);
    let key = key.into_inner();
    let path = resolve_key(&state.root, &key)
        .ok_or_else(|| actix_web::error::ErrorBadRequest("invalid key"))?;

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await.map_err(actix_web::error::ErrorInternalServerError)?;
    }

    let meta_opt = fs::metadata(&path).await.ok();
    if let Some(h) = req.headers().get(header::IF_NONE_MATCH) {
        if h.to_str().ok().map(|s| s.trim()) == Some("*") && meta_opt.is_some() {
            return Err(actix_web::error::ErrorPreconditionFailed("exists"));
        }
    }
    if let Some(h) = req.headers().get(header::IF_MATCH) {
        match meta_opt.as_ref() {
            Some(meta) => {
                let current = make_etag(meta);
                if h.to_str().ok().map(|s| s.trim()) != Some(current.as_str()) {
                    return Err(actix_web::error::ErrorPreconditionFailed("etag mismatch"));
                }
            }
            None => return Err(actix_web::error::ErrorPreconditionFailed("missing")),
        }
    }

    if let Some(limit) = cfg.max_upload_bytes {
        println!("→ MAX_UPLOAD_BYTES set to {} bytes", limit);

        let mut file = File::create(&path)
            .await
            .map_err(actix_web::error::ErrorInternalServerError)?;

        let mut received: u64 = 0;
        while let Some(chunk) = body.next().await {
            let bytes = chunk.map_err(actix_web::error::ErrorBadRequest)?;
            received += bytes.len() as u64;

            if received > limit {
                drop(file);
                let _ = fs::remove_file(&path).await;
                return Err(actix_web::error::ErrorPayloadTooLarge("upload too large"));
            }

            file.write_all(&bytes)
                .await
                .map_err(actix_web::error::ErrorInternalServerError)?;
        }
    } else {
        // no limit
        let mut file = File::create(&path)
            .await
            .map_err(actix_web::error::ErrorInternalServerError)?;
        while let Some(chunk) = body.next().await {
            let bytes = chunk.map_err(actix_web::error::ErrorBadRequest)?;
            file.write_all(&bytes)
                .await
                .map_err(actix_web::error::ErrorInternalServerError)?;
        }
    }

    let existed = meta_opt.is_some();
    Ok(if existed { HttpResponse::Ok().finish() } else { HttpResponse::Created().finish() })
}


async fn head_object(
    _auth: NeedRead,                  // ← enforce read
    state: web::Data<AppState>,
    key: web::Path<String>,
    q: web::Query<GetQuery>,
) -> Result<HttpResponse> {
    println!("→ HEAD /{}/{}", PATH_OBJECTS, key);
    let key = key.into_inner();
    let path = resolve_key(&state.root, &key)
        .ok_or_else(|| actix_web::error::ErrorBadRequest("invalid key"))?;

    let meta = fs::metadata(&path).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            actix_web::error::ErrorNotFound("not found")
        } else {
            actix_web::error::ErrorInternalServerError(e)
        }
    })?;

    let etag = make_etag(&meta);
    let ctype = guess_content_type(&key);

    let attachment = q.download.unwrap_or(1) != 0;
    let disp = if attachment { "attachment" } else { "inline" };
    let filename = key.split('/').last().unwrap_or("file");

    Ok(HttpResponse::Ok()
        .append_header(("Content-Type", ctype))
        .append_header(("Content-Length", meta.len().to_string()))
        .append_header(("ETag", etag))
        .append_header(("Accept-Ranges", "bytes"))
        .append_header(("Content-Disposition", format!("{disp}; filename=\"{filename}\"")))
        .finish())
}

async fn get_object(
    _auth: NeedRead,                  // ← enforce read
    req: HttpRequest,
    state: web::Data<AppState>,
    key: web::Path<String>,
    q: web::Query<GetQuery>,
) -> Result<HttpResponse> {
    println!("→ GET /{}/{}", PATH_OBJECTS, key);
    let key = key.into_inner();
    let path = resolve_key(&state.root, &key)
        .ok_or_else(|| actix_web::error::ErrorBadRequest("invalid key"))?;

    let meta = fs::metadata(&path).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            actix_web::error::ErrorNotFound("not found")
        } else {
            actix_web::error::ErrorInternalServerError(e)
        }
    })?;
    let etag = make_etag(&meta);
    if let Some(inm) = req.headers().get(header::IF_NONE_MATCH) {
        if let Ok(val) = inm.to_str() {
            if val.trim() == etag { return Ok(HttpResponse::NotModified().finish()); }
        }
    }

    let total = meta.len();
    let ctype = guess_content_type(&key);

    let attachment = q.download.unwrap_or(1) != 0;
    let disp = if attachment { "attachment" } else { "inline" };
    let filename = key.split('/').last().unwrap_or("file");

    if let Some(rh) = req.headers().get(header::RANGE) {
        if let Ok(rs) = rh.to_str() {
            if let Some((start, end)) = parse_range(rs, total) {
                let mut file = File::open(&path).await.map_err(actix_web::error::ErrorInternalServerError)?;
                file.seek(std::io::SeekFrom::Start(start)).await.map_err(actix_web::error::ErrorInternalServerError)?;
                let len = end - start + 1;
                let stream = ReaderStream::new(file.take(len));
                return Ok(HttpResponse::PartialContent()
                    .append_header(("Content-Type", ctype))
                    .append_header(("Content-Length", len.to_string()))
                    .append_header(("Content-Range", format!("bytes {}-{}/{}", start, end, total)))
                    .append_header(("Accept-Ranges", "bytes"))
                    .append_header(("ETag", etag))
                    .append_header(("Content-Disposition", format!("{disp}; filename=\"{filename}\"")))
                    .streaming(stream));
            } else {
                return Ok(HttpResponse::RangeNotSatisfiable()
                    .append_header(("Content-Range", format!("bytes */{}", total)))
                    .finish());
            }
        }
    }

    let file = File::open(&path).await.map_err(actix_web::error::ErrorInternalServerError)?;
    let stream = ReaderStream::new(file);
    Ok(HttpResponse::Ok()
        .append_header(("Content-Type", ctype))
        .append_header(("Content-Length", total.to_string()))
        .append_header(("Accept-Ranges", "bytes"))
        .append_header(("ETag", etag))
        .append_header(("Content-Disposition", format!("{disp}; filename=\"{filename}\"")))
        .streaming(stream))
}

async fn delete_object(
    _auth: NeedWrite,                 // ← enforce write
    state: web::Data<AppState>,
    key: web::Path<String>,
) -> Result<HttpResponse> {
    println!("→ DELETE /{}/{}", PATH_OBJECTS, key);
    let key = key.into_inner();
    let path = resolve_key(&state.root, &key)
        .ok_or_else(|| actix_web::error::ErrorBadRequest("invalid key"))?;

    match fs::remove_file(&path).await {
        Ok(_) => Ok(HttpResponse::NoContent().finish()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(actix_web::error::ErrorNotFound("not found")),
        Err(e) => Err(actix_web::error::ErrorInternalServerError(e)),
    }
}

async fn list_objects(
    _auth: NeedList,                  // ← enforce list
    state: web::Data<AppState>,
    q: web::Query<ListQuery>,
) -> Result<HttpResponse> {
    println!("→ LIST /{}", PATH_OBJECTS);
    let root = state.root.clone();
    let recursive = q.recursive.unwrap_or(0) != 0;

    let base = if let Some(pref) = q.prefix.as_deref() {
        resolve_key(&root, pref)
            .ok_or_else(|| actix_web::error::ErrorBadRequest("invalid prefix"))?
    } else {
        root.clone()
    };

    let mut out: Vec<ListedObject> = Vec::new();

    if let Ok(meta) = fs::metadata(&base).await {
        if meta.is_file() {
            let key = base.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
            let modified = meta.modified().ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs()).unwrap_or(0);
            out.push(ListedObject { key, size: meta.len(), modified });
            return Ok(HttpResponse::Ok().json(out));
        }
    }

    let mut stack = vec![base];
    while let Some(dir) = stack.pop() {
        let mut rd = match fs::read_dir(&dir).await {
            Ok(r) => r,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(actix_web::error::ErrorInternalServerError(e)),
        };
        while let Ok(Some(entry)) = rd.next_entry().await {
            let p = entry.path();
            match entry.file_type().await {
                Ok(ft) if ft.is_dir() => {
                    if recursive { stack.push(p); }
                }
                Ok(ft) if ft.is_file() => {
                    let meta = entry.metadata().await
                        .map_err(actix_web::error::ErrorInternalServerError)?;
                    let key = p.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
                    let modified = meta.modified().ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_secs()).unwrap_or(0);
                    out.push(ListedObject { key, size: meta.len(), modified });
                }
                _ => {}
            }
        }
    }

    out.sort_by(|a, b| a.key.cmp(&b.key));
    Ok(HttpResponse::Ok().json(out))
}
