//! Account sign-in, following each provider's documented flow:
//! * GitHub Copilot — OAuth Device Flow with the app's own OAuth App (no client secret).
//!   https://docs.github.com/en/apps/oauth-apps/building-oauth-apps/authorizing-oauth-apps#device-flow
//! * ChatGPT — "Sign in with ChatGPT" for open-source apps: OAuth 2.0 + OIDC, PKCE (S256),
//!   dynamic client registration, loopback redirect on 127.0.0.1, ID token verified against
//!   OpenAI's JWKS. https://developers.openai.com/siwc/token-sharing-open-source/sign-in
//!
//! No password ever passes through Papryvio. Tokens go to the OS credential store.
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::Engine;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::application::error::{AppError, AppResult, ErrorCode};
use crate::application::ports::{ProviderKind, SignInPoll, SignInService, SignInStart};
use crate::infrastructure::models::http::{agent, read_body};

pub const OPENAI_ISSUER: &str = "https://auth.openai.com";
const OPENAI_RESOURCE: &str = "https://api.openai.com/v1";
const OPENAI_SCOPES: &str = "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct";
const PLAN_SCOPE: &str = "chatgpt.tokens.use.direct";
const DYNAMIC_CLIENT: &str = "dynamic_agent_client";
const APP_NAME: &str = "Papryvio";
const CALLBACK_PATH: &str = "/auth/callback";
const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(600);
pub const CHATGPT_REFRESH_LIFETIME_MS: i64 = 30 * 24 * 3600 * 1000;

const GITHUB_DEVICE_URL: &str = "https://github.com/login/device/code";
const GITHUB_TOKEN_URL: &str = "https://github.com/login/oauth/access_token";
const GITHUB_USER_URL: &str = "https://api.github.com/user";

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

fn b64url(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn random_token() -> String {
    let mut bytes = Vec::with_capacity(32);
    bytes.extend_from_slice(uuid::Uuid::new_v4().as_bytes());
    bytes.extend_from_slice(uuid::Uuid::new_v4().as_bytes());
    b64url(&bytes)
}

fn pct(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn pct_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                    out.push(v);
                    i += 2;
                } else {
                    out.push(b'%');
                }
            }
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn query_params(q: &str) -> HashMap<String, Vec<String>> {
    let mut m: HashMap<String, Vec<String>> = HashMap::new();
    for part in q.split('&').filter(|p| !p.is_empty()) {
        let (k, v) = part.split_once('=').unwrap_or((part, ""));
        m.entry(pct_decode(k)).or_default().push(pct_decode(v));
    }
    m
}

fn auth_err(msg: impl Into<String>) -> AppError {
    AppError::new(ErrorCode::Unsupported, msg).retryable()
}

fn post_form(url: &str, form: &[(&str, &str)]) -> AppResult<(u16, Value)> {
    let a = agent(Duration::from_secs(10), Duration::from_secs(30));
    let mut resp = a
        .post(url)
        .header("Accept", "application/json")
        .send_form(form.iter().copied())
        .map_err(|e| auth_err(format!("cant_reach: {e}")))?;
    let status = resp.status().as_u16();
    let body = read_body(&mut resp)?;
    Ok((status, serde_json::from_str(&body).unwrap_or(Value::Null)))
}

// ── ChatGPT ──────────────────────────────────────────────────────────────────────────

#[derive(Clone)]
struct Discovery {
    authorization: String,
    token: String,
    jwks: String,
}

fn discovery() -> AppResult<Discovery> {
    let a = agent(Duration::from_secs(10), Duration::from_secs(20));
    let mut resp = a.get(format!("{OPENAI_ISSUER}/.well-known/openid-configuration")).call().map_err(|e| auth_err(format!("cant_reach: {e}")))?;
    let v: Value = serde_json::from_str(&read_body(&mut resp)?).map_err(|e| auth_err(e.to_string()))?;
    if v.get("issuer").and_then(Value::as_str) != Some(OPENAI_ISSUER) {
        return Err(auth_err("unexpected OpenID configuration"));
    }
    let get = |k: &str| -> AppResult<String> {
        let s = v.get(k).and_then(Value::as_str).ok_or_else(|| auth_err(format!("missing {k}")))?;
        if !s.starts_with(&format!("{OPENAI_ISSUER}/")) {
            return Err(auth_err("endpoint outside OpenAI's issuer"));
        }
        Ok(s.to_string())
    };
    Ok(Discovery { authorization: get("authorization_endpoint")?, token: get("token_endpoint")?, jwks: get("jwks_uri")? })
}

/// Verifies signature, issuer, audience, expiry and (when given) nonce of OpenAI's ID token.
fn verify_id_token(d: &Discovery, id_token: &str, client_id: &str, nonce: Option<&str>) -> AppResult<Value> {
    use jsonwebtoken::{decode, decode_header, jwk::JwkSet, Algorithm, DecodingKey, Validation};
    let header = decode_header(id_token).map_err(|e| auth_err(format!("id token: {e}")))?;
    let a = agent(Duration::from_secs(10), Duration::from_secs(20));
    let mut resp = a.get(&d.jwks).call().map_err(|e| auth_err(format!("cant_reach: {e}")))?;
    let set: JwkSet = serde_json::from_str(&read_body(&mut resp)?).map_err(|e| auth_err(format!("jwks: {e}")))?;
    let jwk = header.kid.as_deref().and_then(|k| set.find(k)).ok_or_else(|| auth_err("id token key not found"))?;
    let key = DecodingKey::from_jwk(jwk).map_err(|e| auth_err(format!("jwk: {e}")))?;
    let mut v = Validation::new(Algorithm::RS256);
    v.set_audience(&[client_id]);
    v.set_issuer(&[OPENAI_ISSUER]);
    v.leeway = 5;
    v.set_required_spec_claims(&["iss", "aud", "exp", "sub"]);
    let data = decode::<Value>(id_token, &key, &v).map_err(|e| auth_err(format!("id token invalid: {e}")))?;
    let claims = data.claims;
    if let Some(n) = nonce {
        if claims.get("nonce").and_then(Value::as_str) != Some(n) {
            return Err(auth_err("id token nonce mismatch"));
        }
    }
    if let Some(azp) = claims.get("azp").and_then(Value::as_str) {
        if azp != client_id {
            return Err(auth_err("id token azp mismatch"));
        }
    }
    Ok(claims)
}

// ── Flows ────────────────────────────────────────────────────────────────────────────

enum Flow {
    Github { device_code: String, interval: Duration, next_poll: Instant, expires: Instant },
    Chatgpt { outcome: Arc<Mutex<Option<SignInPoll>>>, cancel: Arc<std::sync::atomic::AtomicBool>, expires: Instant },
}

pub struct AccountAuth {
    state_file: PathBuf,
    github_client_id: String,
    flows: Mutex<HashMap<String, Flow>>,
}

impl AccountAuth {
    pub fn new(data_dir: PathBuf, github_client_id: String) -> Self {
        Self { state_file: data_dir.join("chatgpt-client.json"), github_client_id, flows: Mutex::new(HashMap::new()) }
    }

    fn load_state(&self) -> serde_json::Map<String, Value> {
        std::fs::read_to_string(&self.state_file).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    fn save_state(&self, state: &serde_json::Map<String, Value>) {
        let tmp = self.state_file.with_extension("json.tmp");
        if std::fs::write(&tmp, serde_json::to_string_pretty(state).unwrap_or_default()).is_ok() {
            let _ = std::fs::rename(&tmp, &self.state_file);
        }
    }

    /// Stable per installation (`ext_agent_host_id`).
    fn host_id(&self) -> String {
        let mut s = self.load_state();
        if let Some(h) = s.get("host_id").and_then(Value::as_str) {
            return h.to_string();
        }
        let h = format!("urn:uuid:{}", uuid::Uuid::new_v4());
        s.insert("host_id".into(), json!(h));
        self.save_state(&s);
        h
    }

    pub fn issued_client_id(&self) -> Option<String> {
        self.load_state().get("client_id").and_then(Value::as_str).map(str::to_string)
    }

    fn remember_client_id(&self, id: &str) {
        let mut s = self.load_state();
        s.insert("client_id".into(), json!(id));
        self.save_state(&s);
    }

    fn start_github(&self) -> AppResult<SignInStart> {
        if self.github_client_id.is_empty() {
            return Err(AppError::new(ErrorCode::Unsupported, "no GitHub OAuth App client id in this build"));
        }
        let (status, body) = post_form(GITHUB_DEVICE_URL, &[("client_id", &self.github_client_id), ("scope", "read:user")])?;
        if status >= 400 || body.get("error").is_some() {
            return Err(auth_err(format!("github: {}", body.get("error").and_then(Value::as_str).unwrap_or("rejected"))));
        }
        let device_code = body.get("device_code").and_then(Value::as_str).ok_or_else(|| auth_err("github: no device code"))?.to_string();
        let interval = body.get("interval").and_then(Value::as_u64).unwrap_or(5);
        let expires_in = body.get("expires_in").and_then(Value::as_u64).unwrap_or(900);
        let flow_id = uuid::Uuid::new_v4().to_string();
        self.flows.lock().unwrap().insert(
            flow_id.clone(),
            Flow::Github {
                device_code,
                interval: Duration::from_secs(interval),
                next_poll: Instant::now() + Duration::from_secs(interval),
                expires: Instant::now() + Duration::from_secs(expires_in),
            },
        );
        Ok(SignInStart {
            flow_id,
            kind: "device".into(),
            user_code: body.get("user_code").and_then(Value::as_str).map(str::to_string),
            verification_uri: body.get("verification_uri").and_then(Value::as_str).unwrap_or("https://github.com/login/device").to_string(),
            expires_in: expires_in as u32,
            interval: interval as u32,
        })
    }

    fn poll_github(&self, flow_id: &str) -> AppResult<SignInPoll> {
        let device_code = {
            let mut flows = self.flows.lock().unwrap();
            let Some(Flow::Github { device_code, interval, next_poll, expires }) = flows.get_mut(flow_id) else { return Ok(SignInPoll::Expired) };
            if Instant::now() > *expires {
                flows.remove(flow_id);
                return Ok(SignInPoll::Expired);
            }
            if Instant::now() < *next_poll {
                return Ok(SignInPoll::Pending); // GitHub rate-limits polling
            }
            *next_poll = Instant::now() + *interval;
            device_code.clone()
        };
        let (_, body) = match post_form(
            GITHUB_TOKEN_URL,
            &[("client_id", &self.github_client_id), ("device_code", &device_code), ("grant_type", "urn:ietf:params:oauth:grant-type:device_code")],
        ) {
            Ok(v) => v,
            Err(_) => return Ok(SignInPoll::Pending), // offline for a moment: keep waiting
        };
        match body.get("error").and_then(Value::as_str) {
            Some("authorization_pending") => return Ok(SignInPoll::Pending),
            Some("slow_down") => {
                if let Some(Flow::Github { interval, next_poll, .. }) = self.flows.lock().unwrap().get_mut(flow_id) {
                    *interval += Duration::from_secs(5);
                    *next_poll = Instant::now() + *interval;
                }
                return Ok(SignInPoll::Pending);
            }
            Some("expired_token") => {
                self.flows.lock().unwrap().remove(flow_id);
                return Ok(SignInPoll::Expired);
            }
            Some("access_denied") => {
                self.flows.lock().unwrap().remove(flow_id);
                return Ok(SignInPoll::Denied("access_denied".into()));
            }
            Some(other) => {
                self.flows.lock().unwrap().remove(flow_id);
                return Ok(SignInPoll::Failed(other.into()));
            }
            None => {}
        }
        self.flows.lock().unwrap().remove(flow_id);
        let token = body.get("access_token").and_then(Value::as_str).ok_or_else(|| auth_err("github: no token"))?.to_string();
        let a = agent(Duration::from_secs(10), Duration::from_secs(20));
        let mut resp = a
            .get(GITHUB_USER_URL)
            .header("Authorization", &format!("Bearer {token}"))
            .header("Accept", "application/json")
            .call()
            .map_err(|e| auth_err(format!("cant_reach: {e}")))?;
        let user: Value = serde_json::from_str(&read_body(&mut resp)?).unwrap_or(Value::Null);
        let login = user.get("login").and_then(Value::as_str).unwrap_or("GitHub").to_string();
        let id = user.get("id").map(|v| v.to_string()).unwrap_or_default();
        let secret = json!({
            "access_token": token,
            "refresh_token": body.get("refresh_token"),
            "expires_at": body.get("expires_in").and_then(Value::as_i64).map(|s| now_ms() + s * 1000),
            "refresh_expires_at": body.get("refresh_token_expires_in").and_then(Value::as_i64).map(|s| now_ms() + s * 1000),
        });
        Ok(SignInPoll::Complete { label: login, external_id: id, secret_json: secret.to_string() })
    }

    fn start_chatgpt(self: &Arc<Self>) -> AppResult<SignInStart> {
        let d = discovery()?;
        let previous = self.issued_client_id();
        let listener = TcpListener::bind("127.0.0.1:1455").or_else(|_| TcpListener::bind("127.0.0.1:0")).map_err(|e| auth_err(format!("no local port: {e}")))?;
        let port = listener.local_addr().map_err(|e| auth_err(e.to_string()))?.port();
        let redirect_uri = format!("http://127.0.0.1:{port}{CALLBACK_PATH}");
        let state = random_token();
        let nonce = random_token();
        let verifier = format!("{}{}", random_token(), random_token());
        let challenge = b64url(&Sha256::digest(verifier.as_bytes()));
        let mut params: Vec<(&str, String)> = vec![
            ("client_id", previous.clone().unwrap_or_else(|| DYNAMIC_CLIENT.into())),
            ("response_type", "code".into()),
            ("redirect_uri", redirect_uri.clone()),
            ("scope", OPENAI_SCOPES.into()),
            ("resource", OPENAI_RESOURCE.into()),
            ("state", state.clone()),
            ("nonce", nonce.clone()),
            ("code_challenge", challenge),
            ("code_challenge_method", "S256".into()),
            ("ext_agent_host_id", self.host_id()),
        ];
        if previous.is_none() {
            params.push(("agent_name_hint", APP_NAME.into()));
        }
        let url = format!("{}?{}", d.authorization, params.iter().map(|(k, v)| format!("{k}={}", pct(v))).collect::<Vec<_>>().join("&"));

        let outcome: Arc<Mutex<Option<SignInPoll>>> = Arc::new(Mutex::new(None));
        let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flow_id = uuid::Uuid::new_v4().to_string();
        self.flows.lock().unwrap().insert(
            flow_id.clone(),
            Flow::Chatgpt { outcome: Arc::clone(&outcome), cancel: Arc::clone(&cancel), expires: Instant::now() + SIGN_IN_TIMEOUT },
        );
        let this = Arc::clone(self);
        std::thread::Builder::new()
            .name("papryvio-signin".into())
            .spawn(move || {
                let result = this.serve_callback(listener, &d, &redirect_uri, &state, &nonce, &verifier, previous, &cancel);
                *outcome.lock().unwrap() = Some(result);
            })
            .map_err(|e| auth_err(e.to_string()))?;
        Ok(SignInStart { flow_id, kind: "browser".into(), user_code: None, verification_uri: url, expires_in: SIGN_IN_TIMEOUT.as_secs() as u32, interval: 2 })
    }

    #[allow(clippy::too_many_arguments)]
    fn serve_callback(
        &self,
        listener: TcpListener,
        d: &Discovery,
        redirect_uri: &str,
        state: &str,
        nonce: &str,
        verifier: &str,
        previous: Option<String>,
        cancel: &std::sync::atomic::AtomicBool,
    ) -> SignInPoll {
        let _ = listener.set_nonblocking(true);
        let deadline = Instant::now() + SIGN_IN_TIMEOUT;
        let expected_host = redirect_uri.trim_start_matches("http://").split('/').next().unwrap_or("").to_string();
        loop {
            if cancel.load(std::sync::atomic::Ordering::SeqCst) || Instant::now() > deadline {
                return SignInPoll::Expired;
            }
            let (stream, _) = match listener.accept() {
                Ok(s) => s,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(150));
                    continue;
                }
                Err(_) => continue,
            };
            let _ = stream.set_nonblocking(false);
            let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
            let mut reader = BufReader::new(&stream);
            let mut request_line = String::new();
            if reader.read_line(&mut request_line).is_err() || request_line.len() > 8192 {
                continue;
            }
            let mut host = String::new();
            for _ in 0..64 {
                let mut line = String::new();
                if reader.read_line(&mut line).is_err() || line.trim().is_empty() {
                    break;
                }
                if let Some((k, v)) = line.split_once(':') {
                    if k.trim().eq_ignore_ascii_case("host") {
                        host = v.trim().to_string();
                    }
                }
            }
            let mut parts = request_line.split_whitespace();
            let (method, target) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
            let (path, query) = target.split_once('?').unwrap_or((target, ""));
            let mut w = &stream;
            if method != "GET" || path != CALLBACK_PATH || host != expected_host {
                let _ = reply(&mut w, 404, "Not found", "");
                continue;
            }
            let q = query_params(query);
            let one = |k: &str| q.get(k).filter(|v| v.len() == 1).map(|v| v[0].clone());
            // A request that is not from this sign-in must not consume it.
            if one("state").as_deref() != Some(state) {
                let _ = reply(&mut w, 400, "This link is not from the current sign-in", "Return to the tab where you started signing in.");
                continue;
            }
            if let Some(err) = one("error") {
                let _ = reply(&mut w, 200, "Sign-in cancelled", "You can close this page and return to Papryvio.");
                return if err == "access_denied" { SignInPoll::Denied(err) } else { SignInPoll::Failed(err) };
            }
            let _ = reply(&mut w, 200, "Back to Papryvio", "Papryvio is finishing the ChatGPT connection. You can close this page.");
            return self.finish_chatgpt(d, &q, redirect_uri, nonce, verifier, previous);
        }
    }

    fn finish_chatgpt(&self, d: &Discovery, q: &HashMap<String, Vec<String>>, redirect_uri: &str, nonce: &str, verifier: &str, previous: Option<String>) -> SignInPoll {
        let code = match q.get("code").filter(|v| v.len() == 1) {
            Some(v) if !v[0].is_empty() => v[0].clone(),
            _ => return SignInPoll::Failed("no authorization code".into()),
        };
        let returned = q.get("client_id").filter(|v| v.len() == 1).map(|v| v[0].clone());
        let client_id = match (&previous, &returned) {
            (Some(p), Some(r)) if p != r => return SignInPoll::Failed("client id changed".into()),
            (Some(p), _) => p.clone(),
            (None, Some(r)) if r != DYNAMIC_CLIENT => r.clone(),
            _ => return SignInPoll::Failed("registration incomplete".into()),
        };
        if previous.is_none() {
            // Saved before the one-time code is spent so a failed exchange does not register twice.
            self.remember_client_id(&client_id);
        }
        let (status, data) = match post_form(
            &d.token,
            &[
                ("grant_type", "authorization_code"),
                ("client_id", &client_id),
                ("code", &code),
                ("code_verifier", verifier),
                ("redirect_uri", redirect_uri),
                ("resource", OPENAI_RESOURCE),
            ],
        ) {
            Ok(v) => v,
            Err(e) => return SignInPoll::Failed(e.detail),
        };
        if status >= 400 {
            return SignInPoll::Failed(data.get("error").and_then(Value::as_str).unwrap_or("token exchange refused").into());
        }
        let Some(id_token) = data.get("id_token").and_then(Value::as_str) else { return SignInPoll::Failed("no id token".into()) };
        let claims = match verify_id_token(d, id_token, &client_id, Some(nonce)) {
            Ok(c) => c,
            Err(e) => return SignInPoll::Failed(e.detail),
        };
        let scopes = data.get("scope").and_then(Value::as_str).unwrap_or("");
        let (Some(access), Some(refresh)) = (data.get("access_token").and_then(Value::as_str), data.get("refresh_token").and_then(Value::as_str)) else {
            return SignInPoll::Failed("incomplete tokens".into());
        };
        if !scopes.split_whitespace().any(|s| s == PLAN_SCOPE) {
            return SignInPoll::Failed("chatgpt_plan_off".into());
        }
        let sub = claims.get("sub").and_then(Value::as_str).unwrap_or_default().to_string();
        let label = claims.get("email").or_else(|| claims.get("name")).and_then(Value::as_str).unwrap_or("ChatGPT").to_string();
        let secret = json!({
            "access_token": access,
            "refresh_token": refresh,
            "expires_at": now_ms() + data.get("expires_in").and_then(Value::as_i64).unwrap_or(3600) * 1000,
            "client_id": client_id,
            "subject": sub,
        });
        SignInPoll::Complete { label, external_id: sub, secret_json: secret.to_string() }
    }

    /// Renews a ChatGPT access token (rotating refresh token). Returns the new secret JSON.
    pub fn refresh_chatgpt(&self, secret: &Value) -> AppResult<Value> {
        let client_id = secret.get("client_id").and_then(Value::as_str).map(str::to_string).or_else(|| self.issued_client_id()).ok_or_else(|| auth_err("chatgpt_signed_out"))?;
        let refresh = secret.get("refresh_token").and_then(Value::as_str).ok_or_else(|| auth_err("chatgpt_signed_out"))?;
        let d = discovery()?;
        let (status, data) = post_form(&d.token, &[("grant_type", "refresh_token"), ("client_id", &client_id), ("refresh_token", refresh), ("resource", OPENAI_RESOURCE)])?;
        if status >= 400 {
            return Err(auth_err(format!("chatgpt_signed_out: {}", data.get("error").and_then(Value::as_str).unwrap_or("refresh refused"))));
        }
        if let (Some(id_token), Some(subject)) = (data.get("id_token").and_then(Value::as_str), secret.get("subject").and_then(Value::as_str)) {
            let claims = verify_id_token(&d, id_token, &client_id, None)?;
            if claims.get("sub").and_then(Value::as_str) != Some(subject) {
                return Err(auth_err("chatgpt_signed_out: refreshed token belongs to another account"));
            }
        }
        let access = data.get("access_token").and_then(Value::as_str).ok_or_else(|| auth_err("incomplete refresh"))?;
        let new_refresh = data.get("refresh_token").and_then(Value::as_str).ok_or_else(|| auth_err("incomplete refresh"))?;
        let mut out = secret.clone();
        out["access_token"] = json!(access);
        out["refresh_token"] = json!(new_refresh);
        out["expires_at"] = json!(now_ms() + data.get("expires_in").and_then(Value::as_i64).unwrap_or(3600) * 1000);
        Ok(out)
    }

    /// Renews an expiring GitHub user token with its refresh token (client id only, no secret).
    pub fn refresh_github(&self, secret: &Value) -> AppResult<Value> {
        let refresh = secret.get("refresh_token").and_then(Value::as_str).ok_or_else(|| auth_err("copilot_signed_out"))?;
        let (status, body) = post_form(GITHUB_TOKEN_URL, &[("client_id", &self.github_client_id), ("grant_type", "refresh_token"), ("refresh_token", refresh)])?;
        if status >= 400 || body.get("access_token").is_none() {
            return Err(auth_err(format!("copilot_signed_out: {}", body.get("error").and_then(Value::as_str).unwrap_or("refresh refused"))));
        }
        let mut out = secret.clone();
        out["access_token"] = body["access_token"].clone();
        if let Some(r) = body.get("refresh_token") {
            out["refresh_token"] = r.clone();
        }
        out["expires_at"] = json!(body.get("expires_in").and_then(Value::as_i64).map(|s| now_ms() + s * 1000));
        Ok(out)
    }
}

fn reply(w: &mut impl Write, status: u16, title: &str, body: &str) -> std::io::Result<()> {
    let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    let page = format!(
        "<!doctype html><html><meta charset=\"utf-8\"><title>Papryvio</title>\
         <body style=\"font-family:system-ui,'Segoe UI',sans-serif;background:#FCFAF4;color:#242521;display:grid;place-items:center;min-height:100vh;margin:0\">\
         <div style=\"text-align:center;max-width:26rem;padding:2rem\"><div style=\"width:48px;height:48px;border-radius:10px;background:#561F32;margin:0 auto 1rem\"></div>\
         <h1 style=\"font-size:1.2rem\">{}</h1><p style=\"color:#55564F\">{}</p></div>\
         <script>history.replaceState(null,'','/auth/complete')</script></body></html>",
        esc(title),
        esc(body)
    );
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        _ => "Not Found",
    };
    write!(
        w,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nConnection: close\r\n\r\n{page}",
        page.len()
    )?;
    w.flush()
}

/// `Arc` wrapper so the ChatGPT flow can hand itself to its listener thread.
pub struct SignIn(pub Arc<AccountAuth>);

impl SignInService for SignIn {
    fn start(&self, kind: ProviderKind) -> AppResult<SignInStart> {
        match kind {
            ProviderKind::GithubCopilot => self.0.start_github(),
            ProviderKind::Chatgpt => self.0.start_chatgpt(),
            _ => Err(AppError::validation("this provider uses an API key")),
        }
    }

    fn poll(&self, flow_id: &str) -> AppResult<SignInPoll> {
        let chatgpt = {
            let flows = self.0.flows.lock().unwrap();
            match flows.get(flow_id) {
                None => return Ok(SignInPoll::Expired),
                Some(Flow::Github { .. }) => None,
                Some(Flow::Chatgpt { outcome, expires, .. }) => Some((Arc::clone(outcome), *expires)),
            }
        };
        match chatgpt {
            None => self.0.poll_github(flow_id),
            Some((outcome, expires)) => {
                let done = outcome.lock().unwrap().take();
                match done {
                    Some(result) => {
                        self.0.flows.lock().unwrap().remove(flow_id);
                        Ok(result)
                    }
                    None if Instant::now() > expires => {
                        self.cancel(flow_id);
                        Ok(SignInPoll::Expired)
                    }
                    None => Ok(SignInPoll::Pending),
                }
            }
        }
    }

    fn cancel(&self, flow_id: &str) {
        if let Some(Flow::Chatgpt { cancel, .. }) = self.0.flows.lock().unwrap().remove(flow_id) {
            cancel.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_roundtrip_and_query_parsing() {
        let v = "a b&c=d/é";
        assert_eq!(pct_decode(&pct(v)), v);
        let q = query_params("code=abc&state=x%2By&client_id=oaiapp_1&state=dup");
        assert_eq!(q["code"], vec!["abc"]);
        assert_eq!(q["state"].len(), 2);
    }

    #[test]
    fn pkce_challenge_is_base64url_sha256() {
        // RFC 7636 appendix B test vector.
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        assert_eq!(b64url(&Sha256::digest(verifier.as_bytes())), "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
    }
}
