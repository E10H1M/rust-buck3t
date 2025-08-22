# rust-buck3t
![alt text](./assets/rusty_ol_bucket.webp)

A tiny streaming object-store API built on [Actix Web](https://actix.rs/).  
Minimal deps, S3-ish behavior (ETag, Range, streaming).  <br>
(゜- ゜) Strong -ish, lol, it's not an S3 replacement... yet.



## Quickstart

```bash
# run (defaults to ./data)
cargo run

# or choose a data dir
RUST_BUCKET_DIR=/tmp/rust-buck3t cargo run
```





## 🔗 Endpoints

- `PUT /objects/{key}` — upload (streaming)
- `GET /objects/{key}` — download (streaming; supports `Range`)
- `HEAD /objects/{key}` — metadata (Content-Length, ETag, Accept-Ranges)
- `DELETE /objects/{key}` — delete


### Notes

- **Streaming** both ways — no buffering entire files in memory
- **ETag**: weak; based on file size + mtime
- **Range**: supports `bytes=start-end` and `bytes=start-`
- **Content-Type**: guessed from extension (fallback `application/octet-stream`)
- **Root dir**: set via `RUST_BUCKET_DIR` (default: `./data`)



## Installing the `requirements`
Some requiremnts for linux:
- pkg-config
- libssl-dev
```
sudo apt update
sudo apt install pkg-config libssl-dev
```

## Installing Rust
```
1) Download install script and follow the commands:
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
2) Add to path:
. "$HOME/.cargo/env"    
3) Then verify via: 
rustc --version
cargo --version
```

## Building the project
To build the rust project you would run this command:
```
cargo build
```



## 📜 License
Licensed under the [MIT](./LICENSE) license. Go make monies. <br>
Just mention me and include my license, k? (゜- ゜) 
