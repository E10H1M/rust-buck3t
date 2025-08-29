use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use serde::Serialize;
use std::env;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Serialize)]
struct Claims {
    sub: String,
    scope: String, // space-delimited
    exp: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    iss: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    aud: Option<String>,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();

    fn arg(flag: &str, args: &[String]) -> Option<String> {
        args.windows(2)
            .find(|w| w[0] == flag)
            .map(|w| w[1].clone())
    }

    let sub = arg("--sub", &args).unwrap_or_else(|| "u1".into());
    let scope = arg("--scope", &args).unwrap_or_else(|| "obj:write obj:read".into());
    let ttl: u64 = arg("--ttl", &args)
        .and_then(|s| s.parse().ok())
        .unwrap_or(3600);

    let iss = arg("--iss", &args).or_else(|| env::var("TEST_ISS").ok());
    let aud = arg("--aud", &args).or_else(|| env::var("JWT_AUDIENCE").ok());

    let secret = env::var("JWT_HS_SECRET").expect("set JWT_HS_SECRET");
    let exp = (SystemTime::now().duration_since(UNIX_EPOCH).unwrap() + Duration::from_secs(ttl))
        .as_secs() as usize;

    let claims = Claims { sub, scope, exp, iss, aud };
    let mut header = Header::new(Algorithm::HS256);
    header.typ = Some("JWT".into());

    let token = encode(&header, &claims, &EncodingKey::from_secret(secret.as_bytes()))
        .expect("encode");
    println!("{token}");
}
