//! The embedded single-page app, PWA files and `/.well-known/assetlinks.json`
//! (see docs/architecture.md section 8, "Non-API routes").
//!
//! `web/dist` is embedded into release builds with rust-embed. Debug builds
//! read the same folder from disk at request time, so `npm --prefix web run
//! build` is picked up without recompiling. When the folder is missing the
//! server still runs and answers page requests with a short note explaining
//! how to build the UI.

use std::borrow::Cow;

use axum::Router;
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use rust_embed::RustEmbed;

use crate::error::ApiError;
use crate::state::AppState;

/// Hashed build assets never change under the same name.
const IMMUTABLE: &str = "public, max-age=31536000, immutable";
/// Entry points must always be revalidated so a new build is picked up.
const NO_CACHE: &str = "no-cache";
/// Other top-level files (icons, favicon): cached briefly.
const SHORT_CACHE: &str = "public, max-age=3600";

#[derive(RustEmbed)]
#[folder = "../../web/dist"]
#[allow_missing = true]
struct Dist;

/// One file of the built UI.
#[derive(Debug, Clone)]
struct WebFile {
    data: Cow<'static, [u8]>,
    /// Content hash, used as the ETag.
    sha256: [u8; 32],
}

/// Where UI files come from: the embedded build, or a fixed map in tests.
type Lookup<'a> = &'a (dyn Fn(&str) -> Option<WebFile> + Send + Sync);

fn embedded(path: &str) -> Option<WebFile> {
    Dist::get(path).map(|file| WebFile {
        sha256: file.metadata.sha256_hash(),
        data: file.data,
    })
}

/// Fallback routes for everything that is not `/api`.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/assets/{*path}", get(asset))
        .route("/manifest.webmanifest", get(manifest))
        .route("/sw.js", get(service_worker))
        .route("/.well-known/assetlinks.json", get(assetlinks))
        .fallback(fallback)
}

async fn asset(Path(path): Path<String>, headers: HeaderMap) -> Response {
    serve_asset(&path, &headers, &embedded)
}

async fn manifest(headers: HeaderMap) -> Response {
    match embedded("manifest.webmanifest") {
        Some(file) => file_response(file, "application/manifest+json", NO_CACHE, &headers),
        None => plain_not_found(),
    }
}

async fn service_worker(headers: HeaderMap) -> Response {
    match embedded("sw.js") {
        Some(file) => {
            let mut response =
                file_response(file, "text/javascript; charset=utf-8", NO_CACHE, &headers);
            response.headers_mut().insert(
                HeaderName::from_static("service-worker-allowed"),
                HeaderValue::from_static("/"),
            );
            response
        }
        None => plain_not_found(),
    }
}

async fn assetlinks(State(state): State<AppState>) -> Response {
    match &state.config.android_assetlinks {
        Some(body) => (
            [
                (header::CONTENT_TYPE, "application/json"),
                (header::CACHE_CONTROL, SHORT_CACHE),
            ],
            body.clone(),
        )
            .into_response(),
        None => ApiError::not_found("No Android app is linked to this server.").into_response(),
    }
}

async fn fallback(method: Method, uri: Uri, headers: HeaderMap) -> Response {
    route_fallback(&method, uri.path(), &headers, &embedded)
}

fn is_api(path: &str) -> bool {
    path == "/api" || path.starts_with("/api/")
}

fn route_fallback(
    method: &Method,
    path: &str,
    headers: &HeaderMap,
    lookup: Lookup<'_>,
) -> Response {
    if is_api(path) {
        return ApiError::not_found("There is nothing at this address.").into_response();
    }
    if method != Method::GET && method != Method::HEAD {
        let mut response = (StatusCode::METHOD_NOT_ALLOWED, "Method not allowed").into_response();
        response
            .headers_mut()
            .insert(header::ALLOW, HeaderValue::from_static("GET, HEAD"));
        return response;
    }
    serve_page(path, headers, lookup)
}

/// A request path as a relative file name inside `web/dist`, or None when it
/// cannot name a file there (traversal, hidden names, encoded separators).
fn dist_name(path: &str) -> Option<String> {
    let decoded = percent_encoding::percent_decode_str(path)
        .decode_utf8()
        .ok()?;
    let rel = decoded.trim_start_matches('/');
    if rel.is_empty() || rel.contains('\\') || rel.contains('\0') {
        return None;
    }
    let ok = rel
        .split('/')
        .all(|part| !part.is_empty() && !part.starts_with('.'));
    ok.then(|| rel.to_string())
}

/// Top-level files from the build (favicon, icons) or the SPA's index.html.
fn serve_page(path: &str, headers: &HeaderMap, lookup: Lookup<'_>) -> Response {
    if let Some(name) = dist_name(path).filter(|n| n != "index.html") {
        if let Some(file) = lookup(&name) {
            return file_response(file, &content_type(&name), SHORT_CACHE, headers);
        }
    }
    match lookup("index.html") {
        Some(file) => file_response(file, "text/html; charset=utf-8", NO_CACHE, headers),
        None => not_built(),
    }
}

fn serve_asset(path: &str, headers: &HeaderMap, lookup: Lookup<'_>) -> Response {
    let file =
        dist_name(path).and_then(|name| lookup(&format!("assets/{name}")).map(|file| (name, file)));
    match file {
        Some((name, file)) => file_response(file, &content_type(&name), IMMUTABLE, headers),
        // Never fall back to index.html here: the browser would try to run HTML as a script.
        None => plain_not_found(),
    }
}

fn content_type(name: &str) -> String {
    let lower = name.to_ascii_lowercase();
    let ext = lower.rsplit('.').next().unwrap_or_default();
    match ext {
        "html" | "htm" => "text/html; charset=utf-8".into(),
        "js" | "mjs" => "text/javascript; charset=utf-8".into(),
        "css" => "text/css; charset=utf-8".into(),
        "json" | "map" => "application/json".into(),
        "webmanifest" => "application/manifest+json".into(),
        "svg" => "image/svg+xml".into(),
        "txt" => "text/plain; charset=utf-8".into(),
        "wasm" => "application/wasm".into(),
        _ => mime_guess::from_path(&lower)
            .first_or_octet_stream()
            .essence_str()
            .to_string(),
    }
}

fn etag(file: &WebFile) -> String {
    format!("\"{}\"", hex::encode(&file.sha256[..16]))
}

fn file_response(
    file: WebFile,
    content_type: &str,
    cache_control: &'static str,
    headers: &HeaderMap,
) -> Response {
    let tag = etag(&file);
    let fresh = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|list| {
            list.split(',')
                .map(|t| t.trim().trim_start_matches("W/"))
                .any(|t| t == "*" || t == tag)
        });
    let mut response = if fresh {
        StatusCode::NOT_MODIFIED.into_response()
    } else {
        let mut response = Body::from(file.data).into_response();
        if let Ok(value) = HeaderValue::from_str(content_type) {
            response.headers_mut().insert(header::CONTENT_TYPE, value);
        }
        response
    };
    let out = response.headers_mut();
    out.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static(cache_control),
    );
    if let Ok(value) = HeaderValue::from_str(&tag) {
        out.insert(header::ETAG, value);
    }
    out.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    response
}

fn plain_not_found() -> Response {
    (
        StatusCode::NOT_FOUND,
        [(header::CACHE_CONTROL, NO_CACHE)],
        "Not found",
    )
        .into_response()
}

const NOT_BUILT: &str = r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>PopcornStash</title>
<style>
  :root { color-scheme: light dark; }
  body { font: 16px/1.5 system-ui, -apple-system, "Segoe UI", sans-serif; max-width: 34rem; margin: 15vh auto; padding: 0 1.5rem; }
  h1 { font-size: 1.25rem; margin: 0 0 .5rem; }
  code { font: 14px/1.4 ui-monospace, SFMono-Regular, Menlo, monospace; padding: .1rem .3rem; border-radius: 4px; background: rgba(127,127,127,.15); }
  p { margin: .75rem 0; }
</style>
</head>
<body>
<h1>PopcornStash is running</h1>
<p>The web interface has not been built into this server yet.</p>
<p>Build it with <code>npm --prefix web ci</code> and <code>npm --prefix web run build</code>, then rebuild the server. The Docker image does this for you.</p>
<p>The API is available under <code>/api</code>.</p>
</body>
</html>
"#;

fn not_built() -> Response {
    (
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::CACHE_CONTROL, NO_CACHE),
        ],
        NOT_BUILT,
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use axum::http::StatusCode;
    use http_body_util::BodyExt;
    use sha2::{Digest, Sha256};

    use super::*;
    use crate::api::library::testing::{TestApp, request};

    fn memory(files: &[(&str, &str)]) -> impl Fn(&str) -> Option<WebFile> + Send + Sync + use<> {
        let map: HashMap<String, Vec<u8>> = files
            .iter()
            .map(|(k, v)| (k.to_string(), v.as_bytes().to_vec()))
            .collect();
        move |path| {
            map.get(path).map(|data| WebFile {
                data: Cow::Owned(data.clone()),
                sha256: Sha256::digest(data).into(),
            })
        }
    }

    async fn body_text(response: Response) -> String {
        String::from_utf8(
            response
                .into_body()
                .collect()
                .await
                .unwrap()
                .to_bytes()
                .to_vec(),
        )
        .unwrap()
    }

    #[tokio::test]
    async fn spa_fallback_serves_index_for_client_routes() {
        let lookup = memory(&[
            ("index.html", "<html>app</html>"),
            ("favicon.svg", "<svg/>"),
            ("assets/app-1a2b.js", "x()"),
        ]);
        let none = HeaderMap::new();
        for path in [
            "/",
            "/movies",
            "/shows/Doctor%20Who",
            "/share",
            "/index.html",
            "/nope.txt",
        ] {
            let response = route_fallback(&Method::GET, path, &none, &lookup);
            assert_eq!(response.status(), StatusCode::OK, "{path}");
            assert_eq!(
                response.headers()[header::CACHE_CONTROL],
                "no-cache",
                "{path}"
            );
            assert_eq!(
                response.headers()[header::CONTENT_TYPE],
                "text/html; charset=utf-8"
            );
            assert_eq!(body_text(response).await, "<html>app</html>");
        }
        let response = route_fallback(&Method::GET, "/favicon.svg", &none, &lookup);
        assert_eq!(response.headers()[header::CONTENT_TYPE], "image/svg+xml");
        assert_eq!(response.headers()[header::CACHE_CONTROL], SHORT_CACHE);

        let response = serve_asset("app-1a2b.js", &none, &lookup);
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CACHE_CONTROL], IMMUTABLE);
        assert_eq!(
            response.headers()[header::CONTENT_TYPE],
            "text/javascript; charset=utf-8"
        );
        assert_eq!(
            serve_asset("missing.js", &none, &lookup).status(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            serve_asset("../index.html", &none, &lookup).status(),
            StatusCode::NOT_FOUND
        );
    }

    #[tokio::test]
    async fn api_paths_never_get_the_spa() {
        let lookup = memory(&[("index.html", "<html>app</html>")]);
        for path in ["/api", "/api/nope", "/api/items/x/y/z"] {
            let response = route_fallback(&Method::GET, path, &HeaderMap::new(), &lookup);
            assert_eq!(response.status(), StatusCode::NOT_FOUND);
            let json: serde_json::Value = serde_json::from_str(&body_text(response).await).unwrap();
            assert_eq!(json["error"]["code"], "not_found");
        }
        let response = route_fallback(&Method::POST, "/movies", &HeaderMap::new(), &lookup);
        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    }

    #[tokio::test]
    async fn etag_revalidation() {
        let lookup = memory(&[("index.html", "<html>app</html>")]);
        let first = route_fallback(&Method::GET, "/", &HeaderMap::new(), &lookup);
        let tag = first.headers()[header::ETAG].clone();
        let mut headers = HeaderMap::new();
        headers.insert(header::IF_NONE_MATCH, tag);
        let second = route_fallback(&Method::GET, "/movies", &headers, &lookup);
        assert_eq!(second.status(), StatusCode::NOT_MODIFIED);
        assert_eq!(second.headers()[header::CACHE_CONTROL], "no-cache");
    }

    #[tokio::test]
    async fn missing_build_explains_itself() {
        let lookup = memory(&[]);
        let response = route_fallback(&Method::GET, "/movies", &HeaderMap::new(), &lookup);
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-cache");
        let text = body_text(response).await;
        assert!(text.contains("npm --prefix web run build"), "{text}");
    }

    #[test]
    fn dist_names_refuse_traversal() {
        assert_eq!(dist_name("/favicon.svg").as_deref(), Some("favicon.svg"));
        assert_eq!(
            dist_name("/icons/icon-192.png").as_deref(),
            Some("icons/icon-192.png")
        );
        assert_eq!(dist_name("/"), None);
        assert_eq!(dist_name("/../secret"), None);
        assert_eq!(dist_name("/%2e%2e/secret"), None);
        assert_eq!(dist_name("/a/..%2fb"), None);
        assert_eq!(dist_name("/.env"), None);
        assert_eq!(dist_name("/a\\b"), None);
    }

    #[tokio::test]
    async fn router_serves_pages_and_json_404s() {
        let app = TestApp::new().await;
        let (status, headers, _) = app.get("/movies", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers[header::CACHE_CONTROL], "no-cache");
        assert!(
            headers[header::CONTENT_TYPE]
                .to_str()
                .unwrap()
                .starts_with("text/html")
        );
        let (status, headers, _) = app.send(request("HEAD", "/movies", None, None)).await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            headers[header::CONTENT_TYPE]
                .to_str()
                .unwrap()
                .starts_with("text/html")
        );

        let (status, _, body) = app.get("/api/nope", None).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"]["code"], "not_found");

        let (status, _, _) = app.get("/.well-known/assetlinks.json", None).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, _, _) = app.get("/assets/definitely-missing.js", None).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn assetlinks_come_from_config() {
        let dir = tempfile::tempdir().unwrap();
        let stash = dir.path().join("stash");
        std::fs::create_dir_all(&stash).unwrap();
        let mut config = stash_core::config::Config::for_tests(&stash, &dir.path().join("config"));
        config.android_assetlinks =
            Some(r#"[{"relation":["delegate_permission/common.handle_all_urls"]}]"#.into());
        let state = crate::app::build_state(config).await.unwrap();
        let response = tower::ServiceExt::oneshot(
            crate::app::router(state),
            request("GET", "/.well-known/assetlinks.json", None, None),
        )
        .await
        .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
        assert!(body_text(response).await.contains("delegate_permission"));
    }
}
