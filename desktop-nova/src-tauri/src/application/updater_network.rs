//! One transport policy for the official updater and explicit headless probes.
//! No probe is run as part of the normal update check/download.
use serde::Serialize;
use std::{
    error::Error,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use updater_http::{ClientBuilder, Url};

pub(super) fn origin(url: &Url) -> String {
    url.origin().ascii_serialization()
}

pub(super) fn source_name(url: &Url) -> String {
    match url.host_str().unwrap_or("") {
        "github.com" | "raw.githubusercontent.com" | "api.github.com" => "GitHub".into(),
        host if host.ends_with(".githubusercontent.com") => "GitHub".into(),
        _ => origin(url),
    }
}

// Never record URL paths/query strings: GitHub asset redirects can carry signed
// query credentials. Proxy environment values are intentionally not recorded.
pub(super) fn error_details(error: &(dyn Error + 'static)) -> String {
    let mut chain = Vec::new();
    let mut current = Some(error);
    while let Some(error) = current {
        chain.push(redact_urls(&error.to_string()));
        if chain.len() == 8 {
            break;
        }
        current = error.source();
    }
    chain.join(": ")
}

fn redact_urls(text: &str) -> String {
    let mut result = String::new();
    let mut rest = text;
    while let Some(scheme) = rest.find("://") {
        let start = rest[..scheme]
            .rfind(|c: char| !c.is_ascii_alphanumeric() && c != '+' && c != '-')
            .map_or(0, |i| i + 1);
        let end = rest[scheme + 3..]
            .find(|c: char| c.is_whitespace() || matches!(c, ')' | ']' | '"' | '\''))
            .map_or(rest.len(), |i| scheme + 3 + i);
        result.push_str(&rest[..start]);
        result.push_str(
            &Url::parse(&rest[start..end])
                .ok()
                .map(|url| origin(&url))
                .unwrap_or_else(|| "[redacted-url]".into()),
        );
        rest = &rest[end..];
    }
    result.push_str(rest);
    result
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Observation {
    stage: &'static str,
    requested_origin: String,
    actual_host: String,
    status: Option<u16>,
    error: Option<String>,
    elapsed_ms: u128,
}

struct Active {
    started: Instant,
    observation: Observation,
}

#[derive(Clone)]
pub(super) struct Trace(Arc<Mutex<Active>>);

impl Trace {
    pub(super) fn new(stage: &'static str, url: &Url) -> Self {
        let trace = Self(Arc::new(Mutex::new(Active {
            started: Instant::now(),
            observation: Observation {
                stage,
                requested_origin: origin(url),
                actual_host: url.host_str().unwrap_or("").into(),
                status: None,
                error: None,
                elapsed_ms: 0,
            },
        })));
        trace.begin(stage, url);
        trace
    }

    pub(super) fn begin(&self, stage: &'static str, url: &Url) {
        let mut active = self.0.lock().unwrap();
        *active = Active {
            started: Instant::now(),
            observation: Observation {
                stage,
                requested_origin: origin(url),
                actual_host: url.host_str().unwrap_or("").into(),
                status: None,
                error: None,
                elapsed_ms: 0,
            },
        };
        let names = [
            "HTTPS_PROXY",
            "https_proxy",
            "ALL_PROXY",
            "all_proxy",
            "NO_PROXY",
            "no_proxy",
        ]
        .into_iter()
        .filter(|key| std::env::var_os(key).is_some())
        .collect::<Vec<_>>()
        .join(",");
        eprintln!("[NOVA_UPDATE] stage={stage} requested=true origin={} proxy_policy=environment_then_system application_proxy=unset proxy_env_names={names} system_proxy_feature=true", origin(url));
    }

    pub(super) fn configure(&self, client: ClientBuilder) -> ClientBuilder {
        // Match the official updater provider for explicit CLI probes too.
        if rustls::crypto::CryptoProvider::get_default().is_none() {
            let _ = rustls::crypto::ring::default_provider().install_default();
        }
        let response_trace = self.clone();
        let redirect_trace = self.clone();
        // Reqwest exposes the real response through its classifier. We observe
        // it and never request a retry, including when the updater later rejects
        // a non-2xx status. This neither duplicates requests nor bypasses the
        // official manifest parser/download/signature/install implementation.
        let observer = updater_http::retry::for_host("").classify_fn(move |response| {
            let mut active = response_trace.0.lock().unwrap();
            active.observation.actual_host = response.uri().host().unwrap_or("").into();
            active.observation.status = response.status().map(|status| status.as_u16());
            active.observation.error = response.error().map(error_details);
            active.observation.elapsed_ms = active.started.elapsed().as_millis();
            eprintln!("[NOVA_UPDATE] stage={} actual_host={} http_status={:?} elapsed_ms={} transport_error={:?}",
                active.observation.stage, active.observation.actual_host, active.observation.status,
                active.observation.elapsed_ms, active.observation.error);
            response.success()
        });
        client.https_only(true).retry(observer).redirect(updater_http::redirect::Policy::custom(move |attempt| {
            let active = redirect_trace.0.lock().unwrap();
            eprintln!("[NOVA_UPDATE] stage={} redirect_status={} from_host={} to_host={} elapsed_ms={}",
                active.observation.stage, attempt.status(), attempt.previous().last().and_then(Url::host_str).unwrap_or(""),
                attempt.url().host_str().unwrap_or(""), active.started.elapsed().as_millis());
            drop(active);
            if attempt.url().scheme() != "https" || !attempt.url().username().is_empty() || attempt.url().password().is_some() {
                return attempt.error("update redirect must be credential-free HTTPS");
            }
            updater_http::redirect::Policy::default().redirect(attempt)
        }))
    }

    pub(super) fn finish(&self, result: &str) -> Observation {
        let mut active = self.0.lock().unwrap();
        active.observation.elapsed_ms = active.started.elapsed().as_millis();
        eprintln!("[NOVA_UPDATE] stage={} origin={} actual_host={} http_status={:?} elapsed_ms={} result={result}",
            active.observation.stage, active.observation.requested_origin, active.observation.actual_host,
            active.observation.status, active.observation.elapsed_ms);
        active.observation.clone()
    }

    pub(super) fn failure(&self, fallback: String) -> String {
        let active = self.0.lock().unwrap();
        let detail = active.observation.error.as_deref().unwrap_or(&fallback);
        format!(
            "{} (HTTP {:?})：{detail}",
            active.observation.requested_origin, active.observation.status
        )
    }
}

/// Explicit CLI only. It reuses the same HTTP client policy and official
/// RemoteRelease parser, but does not claim to install/restart an application.
pub(super) async fn probe(urls: &[Url], target: &str) -> Result<serde_json::Value, String> {
    let mut observations = Vec::new();
    for (index, url) in urls.iter().enumerate() {
        let trace = Trace::new("diagnostic_manifest", url);
        let result = async {
            let response = trace.configure(updater_http::Client::builder()).timeout(Duration::from_secs(15))
                .build().map_err(|error| error_details(&error))?.get(url.clone()).header("Accept", "application/json")
                .send().await.map_err(|error| error_details(&error))?;
            let status = response.status();
            if status.as_u16() == 204 { return Ok(None); }
            if !status.is_success() { return Err(format!("HTTP {status}")); }
            let release: tauri_plugin_updater::RemoteRelease = response.json().await.map_err(|error| error_details(&error))?;
            let download = release.download_url(target).map_err(|error| error_details(&error))?;
            super::validate_https(download.as_str())?;
            if release.signature(target).map_err(|error| error_details(&error))?.trim().is_empty() { return Err("更新包缺少签名。".into()); }
            Ok(Some(serde_json::json!({ "version": release.version.to_string(), "target": target,
                "manifestSource": source_name(url), "downloadOrigin": origin(download), "signaturePresent": true })))
        }.await;
        observations.push(trace.finish(if result.is_ok() { "success" } else { "failed" }));
        match result {
            Ok(release) => {
                return Ok(
                    serde_json::json!({ "release": release, "requests": observations,
                "unrequestedSources": urls[index + 1..].iter().map(origin).collect::<Vec<_>>(),
                "installationExecuted": false, "restartVerified": false }),
                )
            }
            Err(error) => eprintln!(
                "[NOVA_UPDATE] stage=diagnostic_fallback failed_origin={} reason={}",
                origin(url),
                trace.failure(error)
            ),
        }
    }
    Err(serde_json::to_string(&observations).unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn logs_never_keep_url_credentials_paths_or_signed_queries() {
        let error = "request https://name:password@github.com/private?token=secret failed (https://objects.githubusercontent.com/asset?sig=hidden)";
        let result = redact_urls(error);
        assert_eq!(
            result,
            "request https://github.com failed (https://objects.githubusercontent.com)"
        );
        for secret in ["name", "password", "private", "secret", "hidden"] {
            assert!(!result.contains(secret));
        }
    }
    #[test]
    fn source_uses_real_hostname_not_endpoint_position() {
        assert_eq!(
            source_name(&Url::parse("https://raw.githubusercontent.com/a").unwrap()),
            "GitHub"
        );
        assert_ne!(
            source_name(&Url::parse("https://mirror.example/a").unwrap()),
            "GitHub"
        );
        assert_ne!(
            source_name(&Url::parse("https://github.com.attacker.example/a").unwrap()),
            "GitHub"
        );
    }

    #[tokio::test]
    async fn updater_observer_records_real_http_status_without_second_request() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut request = [0u8; 2048];
            stream.read(&mut request).unwrap();
            stream.write_all(b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
            drop(stream);
            listener.set_nonblocking(true).unwrap();
            assert!(
                listener.accept().is_err(),
                "observer must not issue another request"
            );
        });
        let url = Url::parse(&format!("http://{address}/fixture")).unwrap();
        let trace = Trace::new("test_fixture", &url);
        // Loopback transport fixture only; the production configure() always
        // enables HTTPS. This does not fetch a real manifest or signed package.
        let response = trace
            .configure(updater_http::Client::builder())
            .https_only(false)
            .no_proxy()
            .build()
            .unwrap()
            .get(url)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), 503);
        let observation = trace.finish("expected_http_failure");
        assert_eq!(observation.status, Some(503));
        assert_eq!(observation.actual_host, "127.0.0.1");
        server.join().unwrap();
    }

    #[tokio::test]
    async fn updater_observer_preserves_tls_redirect_gate_and_status() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0u8; 2048];
            stream.read(&mut request).unwrap();
            stream.write_all(b"HTTP/1.1 302 Found\r\nLocation: http://plaintext.example/asset\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
        });
        let url = Url::parse(&format!("http://{address}/fixture")).unwrap();
        let trace = Trace::new("test_fixture", &url);
        let error = trace
            .configure(updater_http::Client::builder())
            .https_only(false)
            .no_proxy()
            .build()
            .unwrap()
            .get(url)
            .send()
            .await
            .unwrap_err();
        assert!(error_details(&error).contains("credential-free HTTPS"));
        // The classifier observes the actual HTTP response before the redirect
        // policy rejects the plaintext destination.
        assert_eq!(trace.finish("expected_redirect_failure").status, Some(302));
        server.join().unwrap();
    }
}
