use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::{rngs::OsRng, RngCore};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt;
use std::io::{Read, Write};
use std::net::{IpAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
#[cfg(test)]
use std::sync::Mutex;
use std::time::{Duration, Instant};
use thiserror::Error;
use url::Url;
use zeroize::Zeroize;

const MAX_CALLBACK_BYTES: usize = 8 * 1024;

/// Serializes tests that bind loopback listeners, because the OS can hand a
/// just-released ephemeral port to a parallel test's listener and corrupt its
/// closed-port assertions. Product code never touches this gate.
#[cfg(test)]
pub(crate) static LOOPBACK_TEST_GATE: Mutex<()> = Mutex::new(());

#[derive(Clone)]
pub struct NativeOAuthConfig {
    pub authorization_endpoint: String,
    pub client_id: String,
    pub scopes: Vec<String>,
    pub extra_params: BTreeMap<String, String>,
    pub callback_path: String,
    pub timeout: Duration,
    pub include_nonce: bool,
    /// Optional fixed ports for providers that require pre-registered loopback
    /// ranges. An empty list requests an OS-selected random port.
    pub preferred_ports: Vec<u16>,
}

impl fmt::Debug for NativeOAuthConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NativeOAuthConfig")
            .field("authorization_endpoint", &self.authorization_endpoint)
            .field("client_id", &"[REDACTED]")
            .field("scopes", &self.scopes)
            .field("extra_params", &"[REDACTED]")
            .field("callback_path", &self.callback_path)
            .field("timeout", &self.timeout)
            .field("include_nonce", &self.include_nonce)
            .field("preferred_ports", &self.preferred_ports)
            .finish()
    }
}

impl NativeOAuthConfig {
    pub fn new(authorization_endpoint: String, client_id: String, scopes: Vec<String>) -> Self {
        Self {
            authorization_endpoint,
            client_id,
            scopes,
            extra_params: BTreeMap::new(),
            callback_path: "/oauth/callback".into(),
            timeout: Duration::from_secs(180),
            include_nonce: false,
            preferred_ports: Vec::new(),
        }
    }
}

pub struct OAuthAttemptContext {
    pub verifier: String,
    pub redirect_uri: String,
    pub nonce: Option<String>,
    state: String,
}

impl fmt::Debug for OAuthAttemptContext {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OAuthAttemptContext")
            .field("verifier", &"[REDACTED]")
            .field("redirect_uri", &self.redirect_uri)
            .field("nonce", &self.nonce.as_ref().map(|_| "[REDACTED]"))
            .field("state", &"[REDACTED]")
            .finish()
    }
}

impl Drop for OAuthAttemptContext {
    fn drop(&mut self) {
        self.verifier.zeroize();
        self.nonce.zeroize();
        self.state.zeroize();
    }
}

pub struct NativeOAuthAttempt {
    authorization_url: Url,
    listener: TcpListener,
    callback_path: String,
    context: OAuthAttemptContext,
    timeout: Duration,
}

impl fmt::Debug for NativeOAuthAttempt {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NativeOAuthAttempt")
            .field(
                "authorization_origin",
                &self.authorization_url.origin().ascii_serialization(),
            )
            .field("redirect_uri", &self.context.redirect_uri)
            .field("timeout", &self.timeout)
            .finish()
    }
}

#[derive(Debug)]
pub struct VerifiedAuthorizationCode {
    pub code: String,
    pub context: OAuthAttemptContext,
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum NativeOAuthError {
    #[error("authorization endpoint is invalid")]
    InvalidAuthorizationEndpoint,
    #[error("authorization parameters are invalid")]
    InvalidAuthorizationParameters,
    #[error("loopback callback path is invalid")]
    InvalidCallbackPath,
    #[error("could not bind the loopback callback")]
    CallbackUnavailable,
    #[error("the authorization attempt timed out")]
    Timeout,
    #[error("the authorization callback was invalid")]
    InvalidCallback,
    #[error("the authorization callback state did not match")]
    StateMismatch,
    #[error("authorization was not completed")]
    AuthorizationDenied,
    #[error("the authorization attempt was cancelled")]
    Cancelled,
}

impl NativeOAuthAttempt {
    pub fn start(config: NativeOAuthConfig) -> Result<Self, NativeOAuthError> {
        if !config.callback_path.starts_with('/')
            || config.callback_path.contains('?')
            || config.callback_path.contains('#')
        {
            return Err(NativeOAuthError::InvalidCallbackPath);
        }
        let listener = bind_loopback(&config.preferred_ports)?;
        listener
            .set_nonblocking(true)
            .map_err(|_| NativeOAuthError::CallbackUnavailable)?;
        let port = listener
            .local_addr()
            .map_err(|_| NativeOAuthError::CallbackUnavailable)?
            .port();
        let redirect_uri = format!("http://127.0.0.1:{port}{}", config.callback_path);
        let verifier = random_urlsafe(32);
        let state = random_urlsafe(32);
        let nonce = config.include_nonce.then(|| random_urlsafe(32));
        let challenge = derive_pkce_challenge(&verifier);

        let mut authorization_url = Url::parse(&config.authorization_endpoint)
            .map_err(|_| NativeOAuthError::InvalidAuthorizationEndpoint)?;
        if authorization_url.scheme() != "https" || authorization_url.cannot_be_a_base() {
            return Err(NativeOAuthError::InvalidAuthorizationEndpoint);
        }
        const RESERVED: [&str; 9] = [
            "response_type",
            "client_id",
            "redirect_uri",
            "scope",
            "state",
            "code_challenge",
            "code_challenge_method",
            "nonce",
            "code",
        ];
        if config
            .extra_params
            .keys()
            .any(|key| RESERVED.contains(&key.as_str()))
        {
            return Err(NativeOAuthError::InvalidAuthorizationParameters);
        }
        {
            let mut query = authorization_url.query_pairs_mut();
            query
                .append_pair("response_type", "code")
                .append_pair("client_id", &config.client_id)
                .append_pair("redirect_uri", &redirect_uri)
                .append_pair("scope", &config.scopes.join(" "))
                .append_pair("state", &state)
                .append_pair("code_challenge", &challenge)
                .append_pair("code_challenge_method", "S256");
            if let Some(nonce) = nonce.as_deref() {
                query.append_pair("nonce", nonce);
            }
            for (key, value) in config.extra_params {
                query.append_pair(&key, &value);
            }
        }

        Ok(Self {
            authorization_url,
            listener,
            callback_path: config.callback_path,
            context: OAuthAttemptContext {
                verifier,
                redirect_uri,
                nonce,
                state,
            },
            timeout: config.timeout,
        })
    }

    pub fn authorization_url(&self) -> &Url {
        &self.authorization_url
    }

    pub fn redirect_uri(&self) -> &str {
        &self.context.redirect_uri
    }

    pub fn wait_for_callback(self) -> Result<VerifiedAuthorizationCode, NativeOAuthError> {
        self.wait_for_callback_cancellable(Arc::new(AtomicBool::new(false)))
    }

    /// Like [`Self::wait_for_callback`] but returns [`NativeOAuthError::Cancelled`]
    /// when the shared flag is set, so a provider can abandon an in-flight
    /// attempt without closing the whole process.
    pub fn wait_for_callback_cancellable(
        self,
        cancelled: Arc<AtomicBool>,
    ) -> Result<VerifiedAuthorizationCode, NativeOAuthError> {
        let started = Instant::now();
        while started.elapsed() < self.timeout {
            if cancelled.load(Ordering::SeqCst) {
                return Err(NativeOAuthError::Cancelled);
            }
            match self.listener.accept() {
                Ok((mut stream, peer)) => {
                    if !is_loopback(peer.ip()) {
                        let _ = respond(&mut stream, 400, "Invalid authorization callback.");
                        return Err(NativeOAuthError::InvalidCallback);
                    }
                    return self.accept_callback(&mut stream);
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(_) => return Err(NativeOAuthError::CallbackUnavailable),
            }
        }
        Err(NativeOAuthError::Timeout)
    }

    fn accept_callback(
        self,
        stream: &mut TcpStream,
    ) -> Result<VerifiedAuthorizationCode, NativeOAuthError> {
        // BSD accept() hands the accepted socket the listener's non-blocking
        // flag, so restore blocking mode before the timed read.
        stream
            .set_nonblocking(false)
            .map_err(|_| NativeOAuthError::InvalidCallback)?;
        let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
        let mut buffer = [0_u8; MAX_CALLBACK_BYTES];
        let mut received = 0;
        while received < buffer.len()
            && !buffer[..received].windows(4).any(|window| window == b"\r\n\r\n")
        {
            let count = stream
                .read(&mut buffer[received..])
                .map_err(|_| NativeOAuthError::InvalidCallback)?;
            if count == 0 {
                break;
            }
            received += count;
        }
        let request =
            std::str::from_utf8(&buffer[..received]).map_err(|_| NativeOAuthError::InvalidCallback)?;
        let request_line = request
            .lines()
            .next()
            .ok_or(NativeOAuthError::InvalidCallback)?;
        let mut pieces = request_line.split_whitespace();
        if pieces.next() != Some("GET") {
            let _ = respond(stream, 405, "Invalid authorization callback.");
            return Err(NativeOAuthError::InvalidCallback);
        }
        let target = pieces.next().ok_or(NativeOAuthError::InvalidCallback)?;
        let callback = Url::parse(&format!("http://127.0.0.1{target}"))
            .map_err(|_| NativeOAuthError::InvalidCallback)?;
        if callback.path() != self.callback_path {
            let _ = respond(stream, 404, "Invalid authorization callback.");
            return Err(NativeOAuthError::InvalidCallback);
        }
        let pairs = callback.query_pairs().collect::<Vec<_>>();
        let state_values = pairs
            .iter()
            .filter(|(key, _)| key == "state")
            .map(|(_, value)| value.as_ref())
            .collect::<Vec<_>>();
        if state_values.len() != 1 || state_values[0] != self.context.state {
            let _ = respond(stream, 400, "Authorization could not be verified.");
            return Err(NativeOAuthError::StateMismatch);
        }
        if pairs.iter().any(|(key, _)| key == "error") {
            let _ = respond(stream, 400, "Authorization was not completed.");
            return Err(NativeOAuthError::AuthorizationDenied);
        }
        let codes = pairs
            .iter()
            .filter(|(key, _)| key == "code")
            .map(|(_, value)| value.as_ref())
            .collect::<Vec<_>>();
        if codes.len() != 1 || codes[0].is_empty() {
            let _ = respond(stream, 400, "Authorization was not completed.");
            return Err(NativeOAuthError::AuthorizationDenied);
        }
        let code = codes[0].to_owned();
        let _ = respond(
            stream,
            200,
            "Authorization complete. You can close this window.",
        );
        Ok(VerifiedAuthorizationCode {
            code,
            context: self.context,
        })
    }
}

fn bind_loopback(preferred_ports: &[u16]) -> Result<TcpListener, NativeOAuthError> {
    if preferred_ports.is_empty() {
        return TcpListener::bind(("127.0.0.1", 0))
            .map_err(|_| NativeOAuthError::CallbackUnavailable);
    }
    for port in preferred_ports.iter().copied().chain(std::iter::once(0)) {
        if let Ok(listener) = TcpListener::bind(("127.0.0.1", port)) {
            return Ok(listener);
        }
    }
    Err(NativeOAuthError::CallbackUnavailable)
}

fn is_loopback(ip: IpAddr) -> bool {
    ip.is_loopback()
}

fn random_urlsafe(bytes: usize) -> String {
    let mut value = vec![0_u8; bytes];
    OsRng.fill_bytes(&mut value);
    URL_SAFE_NO_PAD.encode(value)
}

pub fn derive_pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

fn respond(stream: &mut TcpStream, status: u16, message: &str) -> std::io::Result<()> {
    let label = match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "Error",
    };
    let body = format!("<!doctype html><meta charset=utf-8><title>Alfred</title><p>{message}</p>");
    write!(
        stream,
        "HTTP/1.1 {status} {label}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::SocketAddr;
    use std::thread;

    fn config() -> NativeOAuthConfig {
        NativeOAuthConfig {
            authorization_endpoint: "https://example.test/authorize".into(),
            client_id: "public-client".into(),
            scopes: vec!["read".into()],
            extra_params: BTreeMap::new(),
            callback_path: "/oauth/callback".into(),
            timeout: Duration::from_secs(2),
            include_nonce: true,
            preferred_ports: vec![],
        }
    }

    fn callback(
        attempt: NativeOAuthAttempt,
        state: String,
    ) -> Result<VerifiedAuthorizationCode, NativeOAuthError> {
        let redirect = Url::parse(attempt.redirect_uri()).expect("redirect");
        let address = SocketAddr::from(([127, 0, 0, 1], redirect.port().expect("port")));
        let waiter = thread::spawn(move || attempt.wait_for_callback());
        let mut stream = loop {
            if let Ok(stream) = TcpStream::connect(address) {
                break stream;
            }
            thread::yield_now();
        };
        write!(
            stream,
            "GET /oauth/callback?code=authorization-code&state={state} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n"
        )
        .expect("write callback");
        waiter.join().expect("join")
    }

    #[test]
    fn derives_rfc_7636_s256_challenge() {
        assert_eq!(
            derive_pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn rejects_state_mismatch() {
        let _gate = LOOPBACK_TEST_GATE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let result = callback(
            NativeOAuthAttempt::start(config()).expect("start"),
            "wrong".into(),
        );
        assert!(
            matches!(result, Err(NativeOAuthError::StateMismatch)),
            "unexpected result: {result:?}"
        );
    }

    #[test]
    fn returns_code_and_keeps_attempt_context_in_memory() {
        let _gate = LOOPBACK_TEST_GATE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let attempt = NativeOAuthAttempt::start(config()).expect("start");
        let state = attempt.context.state.clone();
        let result = callback(attempt, state).expect("callback");
        assert_eq!(result.code, "authorization-code");
        assert!(!result.context.verifier.is_empty());
        assert!(result.context.nonce.is_some());
    }

    #[test]
    fn listener_is_one_shot() {
        let _gate = LOOPBACK_TEST_GATE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let attempt = NativeOAuthAttempt::start(config()).expect("start");
        let redirect = Url::parse(attempt.redirect_uri()).expect("redirect");
        let address = SocketAddr::from(([127, 0, 0, 1], redirect.port().expect("port")));
        let state = attempt.context.state.clone();
        callback(attempt, state.clone()).expect("first callback");
        // The kernel may hand the freed port to a parallel fixture server, so
        // assert functionally: nothing may still answer an authorization
        // callback on this address.
        let mut stream = match TcpStream::connect(address) {
            Ok(stream) => stream,
            Err(_) => return,
        };
        let _ = stream.set_read_timeout(Some(Duration::from_millis(200)));
        write!(
            stream,
            "GET /oauth/callback?code=leftover&state={state} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n"
        )
        .expect("write probe");
        let mut response = Vec::new();
        let mut chunk = [0_u8; 512];
        loop {
            match stream.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(count) => response.extend_from_slice(&chunk[..count]),
            }
        }
        assert!(!String::from_utf8_lossy(&response).contains("Authorization complete"));
    }

    #[test]
    fn retries_after_preferred_port_collision() {
        let _gate = LOOPBACK_TEST_GATE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let occupied = TcpListener::bind(("127.0.0.1", 0)).expect("occupy port");
        let occupied_port = occupied.local_addr().expect("address").port();
        let mut request = config();
        request.preferred_ports = vec![occupied_port];
        let attempt = NativeOAuthAttempt::start(request).expect("fallback port");
        assert_ne!(
            Url::parse(attempt.redirect_uri()).expect("redirect").port(),
            Some(occupied_port)
        );
    }

    #[test]
    fn expires_attempt_after_timeout() {
        let _gate = LOOPBACK_TEST_GATE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut request = config();
        request.timeout = Duration::from_millis(20);
        let attempt = NativeOAuthAttempt::start(request).expect("start");
        assert!(matches!(
            attempt.wait_for_callback(),
            Err(NativeOAuthError::Timeout)
        ));
    }

    #[test]
    fn a_cancelled_attempt_abandons_without_waiting_for_the_timeout() {
        let _gate = LOOPBACK_TEST_GATE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let attempt = NativeOAuthAttempt::start(config()).expect("start");
        let cancelled = Arc::new(AtomicBool::new(true));
        assert!(matches!(
            attempt.wait_for_callback_cancellable(cancelled),
            Err(NativeOAuthError::Cancelled)
        ));
    }
}
