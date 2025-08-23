// tests/integration.rs
use actix_web::HttpServer;
use reqwest::{header, Client};
use std::{net::TcpListener, time::Duration};
use tempfile::TempDir;

use rust_buck3t::{app, AppState, consts};

fn start_server(cfg: consts::Config) -> (String, TempDir) {
    let td = TempDir::new().unwrap();
    let state = AppState { root: td.path().into() };

    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let addr = listener.local_addr().unwrap();

    let server = HttpServer::new(move || app(state.clone(), cfg.clone()))
        .listen(listener)
        .unwrap()
        .run();

    actix_web::rt::spawn(server);
    (format!("http://{}", addr), td)
}

async fn wait_alive(base: &str) {
    let client = Client::new();
    for _ in 0..20 {
        if let Ok(resp) = client.get(format!("{base}/healthz")).send().await {
            if resp.status().is_success() {
                return;
            }
        }
        actix_web::rt::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("server did not become ready");
}

#[test]
fn healthz_ok() {
    actix_web::rt::System::new().block_on(async {
        let (base, _td) = start_server(consts::Config::from_env());
        wait_alive(&base).await;

        let resp = Client::new()
            .get(format!("{base}/healthz"))
            .send()
            .await
            .unwrap();

        assert!(resp.status().is_success());
        assert_eq!(resp.text().await.unwrap(), "ok");
    });
}

#[test]
fn put_and_head_inline_attachment() {
    actix_web::rt::System::new().block_on(async {
        let (base, _td) = start_server(consts::Config::from_env());
        wait_alive(&base).await;
        let client = Client::new();

        // PUT
        let key = "t/one.txt";
        let resp = client
            .put(format!("{base}/objects/{key}"))
            .body("abc")
            .send()
            .await
            .unwrap();
        assert!(resp.status().is_success());

        // HEAD default (attachment)
        let head = client
            .head(format!("{base}/objects/{key}"))
            .send()
            .await
            .unwrap();
        assert!(head.status().is_success());
        let disp = head
            .headers()
            .get(header::CONTENT_DISPOSITION)
            .unwrap()
            .to_str()
            .unwrap();
        assert!(disp.starts_with("attachment"));
        assert_eq!(
            head.headers()
                .get(header::ACCEPT_RANGES)
                .unwrap()
                .to_str()
                .unwrap(),
            "bytes"
        );
        assert!(head.headers().get(header::ETAG).is_some());

        // HEAD inline
        let head_inline = client
            .head(format!("{base}/objects/{key}?download=0"))
            .send()
            .await
            .unwrap();
        let disp_inline = head_inline
            .headers()
            .get(header::CONTENT_DISPOSITION)
            .unwrap()
            .to_str()
            .unwrap();
        assert!(disp_inline.starts_with("inline"));
    });
}

#[test]
fn get_full_and_etag_304() {
    actix_web::rt::System::new().block_on(async {
        let (base, _td) = start_server(consts::Config::from_env());
        wait_alive(&base).await;
        let client = Client::new();

        let key = "t/two.txt";
        let _ = client
            .put(format!("{base}/objects/{key}"))
            .body("abc")
            .send()
            .await
            .unwrap();

        // GET full body
        let resp = client
            .get(format!("{base}/objects/{key}"))
            .send()
            .await
            .unwrap();
        assert!(resp.status().is_success());
        assert_eq!(resp.text().await.unwrap(), "abc");

        // Grab ETag via HEAD
        let head = client
            .head(format!("{base}/objects/{key}"))
            .send()
            .await
            .unwrap();
        let etag = head.headers().get(header::ETAG).unwrap().to_str().unwrap().to_string();

        // If-None-Match -> 304
        let not_modified = client
            .get(format!("{base}/objects/{key}"))
            .header(header::IF_NONE_MATCH, etag)
            .send()
            .await
            .unwrap();
        assert_eq!(not_modified.status(), reqwest::StatusCode::NOT_MODIFIED);
    });
}

#[test]
fn get_range_variants_and_416() {
    actix_web::rt::System::new().block_on(async {
        let (base, _td) = start_server(consts::Config::from_env());
        wait_alive(&base).await;
        let client = Client::new();

        let key = "t/range.txt";
        let _ = client
            .put(format!("{base}/objects/{key}"))
            .body("abc")
            .send()
            .await
            .unwrap();

        // "bytes=1-" -> "bc"
        let r1 = client
            .get(format!("{base}/objects/{key}"))
            .header(header::RANGE, "bytes=1-")
            .send()
            .await
            .unwrap();
        assert_eq!(r1.status(), reqwest::StatusCode::PARTIAL_CONTENT);
        assert_eq!(r1.text().await.unwrap(), "bc");

        // "bytes=0-1" -> "ab"
        let r2 = client
            .get(format!("{base}/objects/{key}"))
            .header(header::RANGE, "bytes=0-1")
            .send()
            .await
            .unwrap();
        assert_eq!(r2.status(), reqwest::StatusCode::PARTIAL_CONTENT);
        assert_eq!(r2.text().await.unwrap(), "ab");

        // "bytes=-1" -> "c"
        let r3 = client
            .get(format!("{base}/objects/{key}"))
            .header(header::RANGE, "bytes=-1")
            .send()
            .await
            .unwrap();
        assert_eq!(r3.status(), reqwest::StatusCode::PARTIAL_CONTENT);
        assert_eq!(r3.text().await.unwrap(), "c");

        // bad range -> 416
        let rbad = client
            .get(format!("{base}/objects/{key}"))
            .header(header::RANGE, "bytes=99-100")
            .send()
            .await
            .unwrap();
        assert_eq!(rbad.status(), reqwest::StatusCode::RANGE_NOT_SATISFIABLE);
    });
}

#[test]
fn list_prefix_recursive() {
    actix_web::rt::System::new().block_on(async {
        let (base, _td) = start_server(consts::Config::from_env());
        wait_alive(&base).await;
        let client = Client::new();

        // create: a/b.txt and a/c/d.txt
        let _ = client
            .put(format!("{base}/objects/a/b.txt"))
            .body("x")
            .send()
            .await
            .unwrap();
        let _ = client
            .put(format!("{base}/objects/a/c/d.txt"))
            .body("y")
            .send()
            .await
            .unwrap();

        // shallow list (a) -> only a/b.txt
        let l0 = client
            .get(format!("{base}/objects?prefix=a&recursive=0"))
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        let v0: Vec<serde_json::Value> = serde_json::from_str(&l0).unwrap();
        let keys0: Vec<String> = v0
            .into_iter()
            .map(|o| o.get("key").unwrap().as_str().unwrap().to_string())
            .collect();
        assert_eq!(keys0, vec!["a/b.txt".to_string()]);

        // recursive list -> a/b.txt and a/c/d.txt (sorted)
        let l1 = client
            .get(format!("{base}/objects?prefix=a&recursive=1"))
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        let v1: Vec<serde_json::Value> = serde_json::from_str(&l1).unwrap();
        let keys1: Vec<String> = v1
            .into_iter()
            .map(|o| o.get("key").unwrap().as_str().unwrap().to_string())
            .collect();
        assert_eq!(keys1, vec!["a/b.txt".to_string(), "a/c/d.txt".to_string()]);
    });
}

#[test]
fn delete_twice() {
    actix_web::rt::System::new().block_on(async {
        let (base, _td) = start_server(consts::Config::from_env());
        wait_alive(&base).await;
        let client = Client::new();

        let key = "t/del.txt";
        let _ = client
            .put(format!("{base}/objects/{key}"))
            .body("x")
            .send()
            .await
            .unwrap();

        let d1 = client
            .delete(format!("{base}/objects/{key}"))
            .send()
            .await
            .unwrap();
        assert_eq!(d1.status(), reqwest::StatusCode::NO_CONTENT);

        let d2 = client
            .delete(format!("{base}/objects/{key}"))
            .send()
            .await
            .unwrap();
        assert_eq!(d2.status(), reqwest::StatusCode::NOT_FOUND);
    });
}

#[test]
fn put_overwrite_guards_and_413() {
    actix_web::rt::System::new().block_on(async {
        // force tiny upload limit
        let mut cfg = consts::Config::from_env();
        cfg.max_upload_bytes = Some(1);

        let (base, _td) = start_server(cfg);
        wait_alive(&base).await;
        let client = Client::new();

        let key = "t/guards.txt";

        // First PUT should create (201 or 200 acceptable since server returns 201 on create)
        let r1 = client
            .put(format!("{base}/objects/{key}"))
            .body("x")
            .send()
            .await
            .unwrap();
        assert!(r1.status().is_success());

        // Fetch ETag via HEAD
        let head = client
            .head(format!("{base}/objects/{key}"))
            .send()
            .await
            .unwrap();
        let etag = head.headers().get(header::ETAG).unwrap().to_str().unwrap().to_string();

        // If-None-Match:* should fail when exists (412)
        let pre_fail = client
            .put(format!("{base}/objects/{key}"))
            .header(header::IF_NONE_MATCH, "*")
            .body("y")
            .send()
            .await
            .unwrap();
        assert_eq!(pre_fail.status(), reqwest::StatusCode::PRECONDITION_FAILED);

        // If-Match: correct etag -> allow overwrite
        let ok_match = client
            .put(format!("{base}/objects/{key}"))
            .header(header::IF_MATCH, etag.clone())
            .body("z")
            .send()
            .await
            .unwrap();
        assert!(ok_match.status().is_success());

        // If-Match: wrong etag -> 412
        let bad_match = client
            .put(format!("{base}/objects/{key}"))
            .header(header::IF_MATCH, "W/\"nope\"")
            .body("w")
            .send()
            .await
            .unwrap();
        assert_eq!(bad_match.status(), reqwest::StatusCode::PRECONDITION_FAILED);

        // 413 guard should fire
        let too_big = client
            .put(format!("{base}/objects/t/too_big.bin"))
            .body("ab") // 2 bytes > limit 1
            .send()
            .await
            .unwrap();
        assert_eq!(too_big.status(), reqwest::StatusCode::PAYLOAD_TOO_LARGE);

        // ensure partial cleaned (GET -> 404)
        let get_clean = client
            .get(format!("{base}/objects/t/too_big.bin"))
            .send()
            .await
            .unwrap();
        assert_eq!(get_clean.status(), reqwest::StatusCode::NOT_FOUND);
    });
}
