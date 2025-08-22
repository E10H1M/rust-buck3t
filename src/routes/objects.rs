// objects.rs

use actix_web::{http::header, web, HttpRequest, HttpResponse, Result};
use futures_util::StreamExt;
use std::path::{Component, Path, PathBuf};
use tokio::{
    fs,
    fs::File,
    io::{ AsyncReadExt, AsyncSeekExt, AsyncWriteExt},
};
use tokio_util::io::ReaderStream;

use crate::AppState;

// wire up the endpoints for this module
pub(crate) fn init(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::resource("/objects/{key:.*}")
            .route(web::put().to(put_object))
            .route(web::head().to(head_object))
            .route(web::get().to(get_object))
            .route(web::delete().to(delete_object)),
    );
}

/* ---------- helpers (private) ---------- */

// prevent path traversal
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

// tiny content-type guesser
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

// weak etag (size + mtime)
fn make_etag(meta: &std::fs::Metadata) -> String {
    let len = meta.len();
    let ts = meta.modified().ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| (d.as_secs(), d.subsec_nanos()))
        .unwrap_or((0, 0));
    format!("W/\"{}-{}-{}\"", len, ts.0, ts.1)
}

// parse "bytes=start-" or "bytes=start-end"
fn parse_range(h: &str, total: u64) -> Option<(u64, u64)> {
    let s = h.trim();
    if !s.starts_with("bytes=") { return None; }
    let spec = &s[6..];
    if spec.contains(',') { return None; }
    let parts: Vec<&str> = spec.split('-').collect();
    if parts.len() != 2 { return None; }
    let start = parts[0].parse::<u64>().ok()?;
    let end = if parts[1].is_empty() { total.saturating_sub(1) } else { parts[1].parse::<u64>().ok()? };
    if start > end || end >= total { return None; }
    Some((start, end))
}

/* ---------- handlers (private) ---------- */

// PUT /objects/{key}
async fn put_object(
    state: web::Data<AppState>,
    key: web::Path<String>,
    mut body: web::Payload,
) -> Result<HttpResponse> {
    let key = key.into_inner();
    let path = resolve_key(&state.root, &key)
        .ok_or_else(|| actix_web::error::ErrorBadRequest("invalid key"))?;

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await.map_err(actix_web::error::ErrorInternalServerError)?;
    }

    let existed = fs::metadata(&path).await.is_ok();
    let mut file = File::create(&path).await.map_err(actix_web::error::ErrorInternalServerError)?;

    while let Some(chunk) = body.next().await {
        let bytes = chunk.map_err(actix_web::error::ErrorBadRequest)?;
        file.write_all(&bytes).await.map_err(actix_web::error::ErrorInternalServerError)?;
    }

    Ok(if existed { HttpResponse::Ok().finish() } else { HttpResponse::Created().finish() })
}

// HEAD /objects/{key}
async fn head_object(
    state: web::Data<AppState>,
    key: web::Path<String>,
) -> Result<HttpResponse> {
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

    Ok(HttpResponse::Ok()
        .append_header(("Content-Type", guess_content_type(&key)))
        .append_header(("Content-Length", meta.len().to_string()))
        .append_header(("ETag", etag))
        .append_header(("Accept-Ranges", "bytes"))
        .finish())
}

// GET /objects/{key} (Range + If-None-Match)
async fn get_object(
    req: HttpRequest,
    state: web::Data<AppState>,
    key: web::Path<String>,
) -> Result<HttpResponse> {
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
        .append_header(("Content-Disposition", format!("attachment; filename=\"{}\"", key.split('/').last().unwrap_or("file"))))
        .append_header(("Accept-Ranges", "bytes"))
        .append_header(("ETag", etag))
        .streaming(stream))
}

// DELETE /objects/{key}
async fn delete_object(
    state: web::Data<AppState>,
    key: web::Path<String>,
) -> Result<HttpResponse> {
    let key = key.into_inner();
    let path = resolve_key(&state.root, &key)
        .ok_or_else(|| actix_web::error::ErrorBadRequest("invalid key"))?;

    match fs::remove_file(&path).await {
        Ok(_) => Ok(HttpResponse::NoContent().finish()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(actix_web::error::ErrorNotFound("not found")),
        Err(e) => Err(actix_web::error::ErrorInternalServerError(e)),
    }
}
