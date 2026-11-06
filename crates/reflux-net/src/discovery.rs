//! Instance discovery: `GET /.well-known/fluxer` (`http-api/instance.mdx`), with the candidate
//! order of `packages/instance_bootstrap` (origin, then the entered path, then `/api`).

use reflux_model::InstanceDiscovery;
use url::Url;

use crate::{Error, Result};

const WELL_KNOWN: &str = "/.well-known/fluxer";
/// The protocol generation this client speaks; any other `codename` needs a newer client.
const CODENAME: &str = "tungsten";

/// Normalizes user input (`fluxer.app`, `https://example.com/api/`) to `scheme://host[:port][/path]`.
/// Only http(s), no credentials, query or fragment; `https` is assumed when no scheme is given.
pub fn normalize(input: &str) -> Option<String> {
    let input = input.trim();
    if input.is_empty() || input.starts_with('/') {
        return None;
    }
    let candidate = if input.contains("://") { input.to_owned() } else { format!("https://{input}") };
    let url = Url::parse(&candidate).ok()?;
    let host = url.host_str().filter(|h| !h.is_empty())?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port() == Some(0)
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return None;
    }
    let port = url.port().map(|p| format!(":{p}")).unwrap_or_default();
    Some(format!("{}://{host}{port}{}", url.scheme(), url.path().trim_end_matches('/')))
}

/// Discovery URLs to try in order.
pub fn candidates(input: &str) -> Vec<String> {
    let Some(base) = normalize(input) else { return Vec::new() };
    let path_start = base.find("://").map_or(0, |i| i + 3);
    let origin = base[..base[path_start..].find('/').map_or(base.len(), |i| path_start + i)].to_owned();
    let mut out = vec![format!("{origin}{WELL_KNOWN}")];
    for c in [format!("{base}{WELL_KNOWN}"), format!("{origin}/api{WELL_KNOWN}")] {
        if !out.contains(&c) {
            out.push(c);
        }
    }
    out
}

/// Fetches the discovery document for a user-entered host or URL.
pub async fn discover(http: &reqwest::Client, input: &str) -> Result<InstanceDiscovery> {
    let urls = candidates(input);
    if urls.is_empty() {
        return Err(Error::Url(input.to_owned()));
    }
    let mut attempts = Vec::new();
    for url in urls {
        match fetch(http, &url).await {
            Ok(d) => {
                if let Some(c) = d.codename.as_deref().filter(|c| !c.is_empty() && *c != CODENAME) {
                    return Err(Error::Discovery(format!("instance speaks {c}; a newer client is needed")));
                }
                return Ok(d);
            }
            Err(e) => attempts.push(format!("{url}: {e}")),
        }
    }
    Err(Error::Discovery(attempts.join("; ")))
}

async fn fetch(http: &reqwest::Client, url: &str) -> Result<InstanceDiscovery> {
    let bytes = http.get(url).send().await?.error_for_status()?.bytes().await?;
    let d: InstanceDiscovery = reflux_model::from_slice(&bytes)?;
    if d.endpoints.gateway.is_empty() || (d.endpoints.api.is_empty() && d.endpoints.api_client.is_empty()) {
        return Err(Error::Discovery("document has no api or gateway endpoint".into()));
    }
    Ok(d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalization() {
        assert_eq!(normalize(" fluxer.app ").as_deref(), Some("https://fluxer.app"));
        assert_eq!(normalize("HTTPS://Fluxer.App/").as_deref(), Some("https://fluxer.app"));
        assert_eq!(normalize("http://localhost:8080/api//").as_deref(), Some("http://localhost:8080/api"));
        assert_eq!(normalize("https://example.com:443").as_deref(), Some("https://example.com"));
        for bad in ["", "/api", "ftp://x.org", "https://u:p@x.org", "https://x.org/?a=1", "https://x.org#f", "x.org:0"] {
            assert_eq!(normalize(bad), None, "{bad}");
        }
    }

    #[test]
    fn candidate_order() {
        assert_eq!(candidates("fluxer.app"), ["https://fluxer.app/.well-known/fluxer", "https://fluxer.app/api/.well-known/fluxer"]);
        assert_eq!(
            candidates("http://h:1/chat/"),
            ["http://h:1/.well-known/fluxer", "http://h:1/chat/.well-known/fluxer", "http://h:1/api/.well-known/fluxer"]
        );
        assert_eq!(candidates("https://h/api"), ["https://h/.well-known/fluxer", "https://h/api/.well-known/fluxer"]);
        assert!(candidates("ws://h").is_empty());
    }
}
