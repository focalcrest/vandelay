/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: Apache-2.0 OR MIT
 */

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use ureq::Agent;
use ureq::config::{Config, RedirectAuthHeaders};
use ureq::tls::{RootCerts, TlsConfig};

use crate::exchange_ews::error::EwsError;
use crate::exchange_ews::parse::{EnvelopeKind, read_envelope_summary};
use crate::exchange_ews::retry::{FaultDisposition, classify_fault, classify_http_status};
use crate::exchange_ews::soap::{EnvelopeOptions, soap_action, wrap_envelope};
use crate::exchange_ews::types::ServerVersion;
use crate::jmap::http::{Auth, RetryPolicy, retry_after_header};
use crate::jmap::retry::{self, Disposition, RateLimitState};
use crate::logging::{LEVEL_BODIES, LEVEL_DEFAULT, LEVEL_PROGRESS, Logger};

const MAX_BODY: u64 = 2 * 1024 * 1024 * 1024;
const LONG_RETRY_THRESHOLD: Duration = Duration::from_secs(10);

struct Inner {
    agent: Agent,
    auth: Mutex<Auth>,
    impersonated_smtp: Mutex<Option<String>>,
    anchor_mailbox: Mutex<Option<String>>,
    retry: RetryPolicy,
    rate_limit: RateLimitState,
    version: Mutex<ServerVersion>,
    log_level: AtomicU8,
    retries_total: AtomicU64,
    retry_after_sleeps: AtomicU64,
    soap_calls: AtomicU64,
    user_agent: String,
}

#[derive(Clone)]
pub struct EwsClient {
    inner: Arc<Inner>,
}

#[derive(Debug, Clone)]
pub struct SoapResponse {
    pub body: Vec<u8>,
    pub server_version: Option<ServerVersion>,
}

impl EwsClient {
    pub fn new(auth: Auth, retry: RetryPolicy, allow_invalid_certs: bool) -> EwsClient {
        let config: Config = Config::builder()
            .http_status_as_error(false)
            .redirect_auth_headers(RedirectAuthHeaders::SameHost)
            .tls_config(
                TlsConfig::builder()
                    .root_certs(RootCerts::PlatformVerifier)
                    .disable_verification(allow_invalid_certs)
                    .build(),
            )
            .build();
        EwsClient {
            inner: Arc::new(Inner {
                agent: config.new_agent(),
                auth: Mutex::new(auth),
                impersonated_smtp: Mutex::new(None),
                anchor_mailbox: Mutex::new(None),
                retry,
                rate_limit: RateLimitState::new(),
                version: Mutex::new(ServerVersion::Exchange2013Sp1),
                log_level: AtomicU8::new(LEVEL_DEFAULT),
                retries_total: AtomicU64::new(0),
                retry_after_sleeps: AtomicU64::new(0),
                soap_calls: AtomicU64::new(0),
                user_agent: format!("vandelay/{}", env!("CARGO_PKG_VERSION")),
            }),
        }
    }

    pub fn set_auth(&self, auth: Auth) {
        if let Ok(mut g) = self.inner.auth.lock() {
            *g = auth;
        }
    }

    pub fn set_anchor_mailbox(&self, smtp: Option<String>) {
        if let Ok(mut g) = self.inner.anchor_mailbox.lock() {
            *g = smtp;
        }
    }

    pub fn set_impersonation(&self, smtp: Option<String>) {
        if let Ok(mut g) = self.inner.impersonated_smtp.lock() {
            *g = smtp;
        }
    }

    pub fn set_logger(&self, logger: Logger) {
        self.inner
            .log_level
            .store(logger.level(), Ordering::Relaxed);
    }

    pub fn logger(&self) -> Logger {
        Logger::new(self.inner.log_level.load(Ordering::Relaxed))
    }

    pub fn set_server_version(&self, version: ServerVersion) {
        if let Ok(mut g) = self.inner.version.lock() {
            *g = version;
        }
    }

    pub fn server_version(&self) -> ServerVersion {
        self.inner
            .version
            .lock()
            .map(|g| *g)
            .unwrap_or(ServerVersion::Exchange2013Sp1)
    }

    pub fn retries_observed(&self) -> u64 {
        self.inner.retries_total.load(Ordering::Relaxed)
    }

    pub fn retry_after_sleeps(&self) -> u64 {
        self.inner.retry_after_sleeps.load(Ordering::Relaxed)
    }

    pub fn soap_calls(&self) -> u64 {
        self.inner.soap_calls.load(Ordering::Relaxed)
    }

    pub fn rate_limit(&self) -> &RateLimitState {
        &self.inner.rate_limit
    }

    pub fn call(&self, url: &str, operation: &str, body: &str) -> Result<SoapResponse, EwsError> {
        let envelope = self.wrap(body);
        let action = soap_action(operation);
        self.inner.soap_calls.fetch_add(1, Ordering::Relaxed);
        self.execute(url, &envelope, &action)
    }

    fn wrap(&self, body: &str) -> String {
        let version = self.server_version();
        let smtp = self
            .inner
            .impersonated_smtp
            .lock()
            .ok()
            .and_then(|g| g.clone());
        let opts = EnvelopeOptions {
            version,
            impersonated_smtp: smtp.as_deref(),
        };
        wrap_envelope(opts, body)
    }

    fn auth_header(&self) -> String {
        let auth = self.inner.auth.lock().ok().map(|g| g.clone());
        match auth {
            Some(Auth::Basic { user, password }) => {
                format!("Basic {}", STANDARD.encode(format!("{user}:{password}")))
            }
            Some(Auth::Bearer { token }) => format!("Bearer {token}"),
            None => String::new(),
        }
    }

    fn anchor_header(&self) -> Option<String> {
        self.inner
            .anchor_mailbox
            .lock()
            .ok()
            .and_then(|g| g.clone())
    }

    fn execute(&self, url: &str, body: &str, action: &str) -> Result<SoapResponse, EwsError> {
        let logger = self.logger();
        let policy = self.inner.retry;
        let mut attempt: u32 = 0;
        loop {
            self.inner.rate_limit.cooldown().wait();
            let outcome = self.one_attempt(url, body, action);
            match outcome {
                AttemptOutcome::Ok {
                    status,
                    body: bytes,
                    retry_after,
                } => {
                    if (200..300).contains(&status) {
                        self.inner.rate_limit.on_success();
                        match read_envelope_summary(&bytes) {
                            Ok(EnvelopeKind::Body { version }) => {
                                if let Some(v) = version.to_server_version() {
                                    self.set_server_version(v);
                                }
                                return Ok(SoapResponse {
                                    body: bytes,
                                    server_version: version.to_server_version(),
                                });
                            }
                            Ok(EnvelopeKind::Fault { fault, version }) => {
                                if let Some(v) = version.to_server_version() {
                                    self.set_server_version(v);
                                }
                                match classify_fault(&fault.response_code) {
                                    FaultDisposition::Fatal => {
                                        return Err(EwsError::SoapFault {
                                            code: fault.response_code,
                                            reason: fault.fault_string,
                                        });
                                    }
                                    FaultDisposition::Auth => {
                                        return Err(EwsError::Auth(fault.fault_string));
                                    }
                                    FaultDisposition::Retryable { delay } => {
                                        attempt += 1;
                                        self.inner.retries_total.fetch_add(1, Ordering::Relaxed);
                                        if attempt > policy.max_retries {
                                            return Err(EwsError::RetriesExhausted(format!(
                                                "{operation} kept returning {code}",
                                                operation = action,
                                                code = fault.response_code
                                            )));
                                        }
                                        let chosen =
                                            self.inner.rate_limit.on_throttle(&policy, delay);
                                        if chosen >= LONG_RETRY_THRESHOLD {
                                            logger.warn(&format!(
                                                "EWS soap fault {} ({}); waiting {}s before retry {}/{}",
                                                fault.response_code,
                                                fault.fault_string,
                                                chosen.as_secs(),
                                                attempt,
                                                policy.max_retries
                                            ));
                                        }
                                        if logger.enabled(LEVEL_BODIES) {
                                            eprintln!(
                                                "retry {}/{} {} after {:?} ({})",
                                                attempt,
                                                policy.max_retries,
                                                action,
                                                chosen,
                                                fault.response_code,
                                            );
                                        }
                                        std::thread::sleep(chosen);
                                        continue;
                                    }
                                }
                            }
                            Err(e) => return Err(e),
                        }
                    }
                    if status == 401 {
                        return Err(EwsError::Auth(format!(
                            "server returned 401: {}",
                            truncate(&bytes)
                        )));
                    }
                    if status == 403 {
                        return Err(EwsError::HttpStatus {
                            status: 403,
                            body: truncate(&bytes),
                        });
                    }
                    match classify_http_status(status) {
                        Disposition::Fatal => {
                            return Err(EwsError::HttpStatus {
                                status,
                                body: truncate(&bytes),
                            });
                        }
                        Disposition::Retryable => {
                            attempt += 1;
                            self.inner.retries_total.fetch_add(1, Ordering::Relaxed);
                            if attempt > policy.max_retries {
                                return Err(EwsError::RetriesExhausted(format!(
                                    "{action} kept returning http {status}"
                                )));
                            }
                            if retry_after.is_some() {
                                self.inner
                                    .retry_after_sleeps
                                    .fetch_add(1, Ordering::Relaxed);
                            }
                            let chosen = self.inner.rate_limit.on_throttle(&policy, retry_after);
                            if chosen >= LONG_RETRY_THRESHOLD {
                                logger.warn(&format!(
                                    "EWS {action} rate-limited (http {status}); waiting {}s",
                                    chosen.as_secs()
                                ));
                            }
                            if logger.enabled(LEVEL_PROGRESS) {
                                eprintln!(
                                    "retry {}/{} (http {status})",
                                    attempt, policy.max_retries
                                );
                            }
                            std::thread::sleep(chosen);
                        }
                    }
                }
                AttemptOutcome::Transport(err) => {
                    attempt += 1;
                    self.inner.retries_total.fetch_add(1, Ordering::Relaxed);
                    if attempt > policy.max_retries {
                        return Err(err);
                    }
                    let delay = retry::backoff_delay(&policy, attempt);
                    if delay >= LONG_RETRY_THRESHOLD {
                        logger.warn(&format!(
                            "transient transport failure ({err}); waiting {}s before retry {}/{}",
                            delay.as_secs(),
                            attempt,
                            policy.max_retries
                        ));
                    }
                    std::thread::sleep(delay);
                }
            }
        }
    }

    fn one_attempt(&self, url: &str, body: &str, action: &str) -> AttemptOutcome {
        let mut req = self
            .inner
            .agent
            .post(url)
            .header("Authorization", self.auth_header())
            .header("Content-Type", "text/xml; charset=utf-8")
            .header("Accept", "text/xml, application/soap+xml, application/xml")
            .header("Accept-Encoding", "gzip")
            .header("SOAPAction", action.to_owned())
            .header("User-Agent", self.inner.user_agent.as_str());
        if let Some(anchor) = self.anchor_header() {
            req = req.header("X-AnchorMailbox", anchor);
        }
        let result = req.send(body.as_bytes());
        match result {
            Ok(mut resp) => {
                let status = resp.status().as_u16();
                let retry_after = resp
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
                    .and_then(retry_after_header);
                match resp.body_mut().with_config().limit(MAX_BODY).read_to_vec() {
                    Ok(bytes) => AttemptOutcome::Ok {
                        status,
                        body: bytes,
                        retry_after,
                    },
                    Err(e) => AttemptOutcome::Transport(EwsError::Transport(format!(
                        "reading response body: {e}"
                    ))),
                }
            }
            Err(e) => AttemptOutcome::Transport(map_ureq_error(e)),
        }
    }
}

enum AttemptOutcome {
    Ok {
        status: u16,
        body: Vec<u8>,
        retry_after: Option<Duration>,
    },
    Transport(EwsError),
}

fn map_ureq_error(err: ureq::Error) -> EwsError {
    match err {
        ureq::Error::Io(e) => EwsError::Transport(format!("io: {e}")),
        ureq::Error::Timeout(t) => EwsError::Transport(format!("timeout: {t}")),
        ureq::Error::HostNotFound => EwsError::Transport("host not found".to_owned()),
        ureq::Error::ConnectionFailed => EwsError::Transport("connection failed".to_owned()),
        ureq::Error::BodyStalled => EwsError::Transport("body stalled".to_owned()),
        ureq::Error::Tls(m) => EwsError::Transport(format!("tls: {m}")),
        ureq::Error::TooManyRedirects => EwsError::Connect("too many redirects".to_owned()),
        ureq::Error::RedirectFailed => EwsError::Connect("redirect failed".to_owned()),
        ureq::Error::TlsRequired => {
            EwsError::Connect("server requires TLS but transport is unsecured".to_owned())
        }
        other => EwsError::Connect(other.to_string()),
    }
}

fn truncate(body: &[u8]) -> String {
    let text = String::from_utf8_lossy(body);
    if text.len() <= 512 {
        return text.into_owned();
    }
    let end = text.floor_char_boundary(512);
    format!("{}...", &text[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_constructs_with_defaults() {
        let c = EwsClient::new(
            Auth::Bearer { token: "t".into() },
            RetryPolicy::new(3),
            false,
        );
        assert_eq!(c.server_version(), ServerVersion::Exchange2013Sp1);
        assert_eq!(c.retries_observed(), 0);
        assert_eq!(c.retry_after_sleeps(), 0);
    }

    #[test]
    fn server_version_can_be_pinned() {
        let c = EwsClient::new(
            Auth::Bearer { token: "t".into() },
            RetryPolicy::new(0),
            false,
        );
        c.set_server_version(ServerVersion::Exchange2019);
        assert_eq!(c.server_version(), ServerVersion::Exchange2019);
    }

    #[test]
    fn anchor_mailbox_round_trips() {
        let c = EwsClient::new(
            Auth::Bearer { token: "t".into() },
            RetryPolicy::new(0),
            false,
        );
        c.set_anchor_mailbox(Some("alice@x".to_owned()));
        assert_eq!(c.anchor_header().as_deref(), Some("alice@x"));
    }
}
