//! Shared pooled HTTP clients with HTTP/2, gzip, and brotli support.
//!
//! All general and non-redirecting HTTP requests reuse these `OnceLock`-backed
//! clients so TCP connections, HTTP/2 multiplex streams, and TLS sessions are
//! pooled across calls. Call sites override per-request timeouts via
//! `RequestBuilder::timeout(...)`.

use std::sync::OnceLock;
use std::time::Duration;

const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
const NO_REDIRECT_TIMEOUT: Duration = Duration::from_secs(15);
const FAST_CONNECT_TIMEOUT: Duration = Duration::from_secs(4);
const FAST_TIMEOUT: Duration = Duration::from_secs(6);
const SHARED_IDLE_PER_HOST: usize = 16;
const NO_REDIRECT_IDLE_PER_HOST: usize = 32;

static SHARED_CLIENT: OnceLock<reqwest::blocking::Client> = OnceLock::new();
static NO_REDIRECT_CLIENT: OnceLock<reqwest::blocking::Client> = OnceLock::new();
static NO_REDIRECT_FAST_CLIENT: OnceLock<reqwest::blocking::Client> = OnceLock::new();

fn build_client(
    connect_timeout: Duration,
    timeout: Duration,
    pool_max_idle_per_host: usize,
    follow_redirects: bool,
) -> reqwest::blocking::Client {
    let mut builder = reqwest::blocking::Client::builder()
        .connect_timeout(connect_timeout)
        .timeout(timeout)
        .pool_max_idle_per_host(pool_max_idle_per_host);
    if !follow_redirects {
        builder = builder.redirect(reqwest::redirect::Policy::none());
    }
    builder
        .build()
        .expect("failed to initialize shared reqwest HTTP client")
}

/// Shared pooled client with standard redirect handling, HTTP/2, gzip, and brotli.
pub(crate) fn shared() -> &'static reqwest::blocking::Client {
    SHARED_CLIENT.get_or_init(|| {
        build_client(
            DEFAULT_CONNECT_TIMEOUT,
            DEFAULT_TIMEOUT,
            SHARED_IDLE_PER_HOST,
            true,
        )
    })
}

/// Shared pooled client with redirects disabled (`Policy::none()`), 15s default timeout.
pub(crate) fn no_redirect() -> &'static reqwest::blocking::Client {
    NO_REDIRECT_CLIENT.get_or_init(|| {
        build_client(
            DEFAULT_CONNECT_TIMEOUT,
            NO_REDIRECT_TIMEOUT,
            NO_REDIRECT_IDLE_PER_HOST,
            false,
        )
    })
}

/// Shared pooled client with redirects disabled and tight connect/request timeouts
/// for parallel RSS and short-probe fan-outs.
pub(crate) fn no_redirect_fast() -> &'static reqwest::blocking::Client {
    NO_REDIRECT_FAST_CLIENT.get_or_init(|| {
        build_client(
            FAST_CONNECT_TIMEOUT,
            FAST_TIMEOUT,
            NO_REDIRECT_IDLE_PER_HOST,
            false,
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_clients_return_stable_singletons() {
        assert!(std::ptr::eq(shared(), shared()));
        assert!(std::ptr::eq(no_redirect(), no_redirect()));
        assert!(std::ptr::eq(no_redirect_fast(), no_redirect_fast()));
        assert!(!std::ptr::eq(shared(), no_redirect()));
        assert!(!std::ptr::eq(no_redirect(), no_redirect_fast()));
    }

    #[test]
    fn cargo_toml_enables_http_pooling_features_and_safe_profiles() {
        let manifest = include_str!("../Cargo.toml");
        assert!(manifest.contains("'gzip'"));
        assert!(manifest.contains("'brotli'"));
        assert!(manifest.contains("'http2'"));
        assert!(manifest.contains("[profile.release]"));
        assert!(manifest.contains("lto = \"thin\""));
        assert!(manifest.contains("codegen-units = 1"));
        assert!(manifest.contains("[profile.dev.package.\"*\"]"));
        assert!(manifest.contains("opt-level = 2"));
        assert!(
            !manifest.contains("panic = \"abort\"") && !manifest.contains("panic = 'abort'"),
            "Cargo.toml must never set panic = \"abort\""
        );
    }
}
