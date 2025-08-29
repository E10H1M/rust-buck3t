# rust-buck3t
![alt text](./assets/rusty_ol_bucket.webp)

A tiny streaming object-store API built on [Actix Web](https://actix.rs/).  
Minimal deps, S3-ish behavior (ETag, Range, streaming).  <br>
(゜- ゜) Strong -ish, lol, it's not an S3 replacement... yet.


## Docs
[![Documentation](https://img.shields.io/badge/docs-rust--buck3t--ui-blue)](https://e10h1m.github.io/rust-buck3t-ui/)
[![Actix](https://img.shields.io/badge/docs-actix-brown)](https://actix.rs/)

## Quickstart

```bash
# run (defaults to ./data)
cargo run
```
# Changelog — rust-buck3t

## [Unreleased]
- Planned: stronger password hashing (argon2/bcrypt), denylist or rotation-based logout

---

## [0.0.1] - 2025-08
### Added
- **Authentication**
  - HS256 JWT auth (`AUTH_MODE=jwt_hs256`) with enforced expiration (`exp`)
  - Signup / Login / Logout routes (dev-only)
  - User database persisted at `./auth/users.json` (plaintext, dev-only)
  - Route-level scope gates:
    - `NeedWrite` → required for PUT/DELETE
    - `NeedRead` → required for GET/HEAD
    - `NeedList` → required for object listings
  - TTL clamping via `AUTH_MAX_TTL_SECS`
  - Configurable scopes per route (`JWT_SCOPES_WRITE`, `JWT_SCOPES_READ`, `JWT_SCOPES_LIST`)

- **Object Storage**
  - Full binary-safe PUT/GET/HEAD/DELETE for objects
  - Conditional writes:
    - `If-None-Match: *` → reject if exists
    - `If-Match: <etag>` → reject if mismatch
  - Auto ETag generation per object
  - Range requests:
    - Byte range support (`Range`, `Content-Range`)
    - `bytes=-N` suffix ranges
  - Inline vs. attachment control via query (`?download=0|1`)
  - Object listing (`GET /objects?prefix=&recursive=1`)
  - Upload guard:
    - `MAX_UPLOAD_BYTES` enforced mid-stream; oversized uploads return `413 Payload Too Large`

- **Server & Config**
  - `.env` based configuration:
    - `HOST`, `PORT`, `RUST_BUCKET_DIR`
    - `MAX_UPLOAD_BYTES`
    - `AUTH_MODE`, `AUTH_MAX_TTL_SECS`
    - `JWT_HS_SECRET`, `AUTH_USER_DB`
  - Startup banner prints auth mode, scopes, and configured limits

### Changed
- Scope parsing supports `scope`, `scopes[]`, and `scp` claims
- Audience/issuer validation enforced if set
- Banner improved with auth information

---

## [0.0.1] - 2025-08
### Added
- Core Actix server and state (`AppState`)
- Basic object store:
  - PUT, GET, DELETE routes
  - Inline/attachment download headers
  - ETag + Range support
- Configurable bucket root via `.env`
- Health endpoint (`/healthz`)




## 📜 License
Licensed under the [MIT](./LICENSE) license. Go make monies. <br>
Just mention me and include my license, k? (゜- ゜) 
