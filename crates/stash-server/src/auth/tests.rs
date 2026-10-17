//! End-to-end tests through the router with a software passkey.

use axum::Router;
use axum::body::Body;
use axum::http::{HeaderMap, Method, Request, StatusCode, header};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;
use webauthn_authenticator_rs::WebauthnAuthenticator;
use webauthn_authenticator_rs::prelude::{
    CreationChallengeResponse, RequestChallengeResponse, Url,
};
use webauthn_authenticator_rs::softpasskey::SoftPasskey;

use crate::state::AppState;

const ORIGIN: &str = "http://localhost:8788";

struct Harness {
    _dir: tempfile::TempDir,
    state: AppState,
    app: Router,
}

async fn harness() -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let state = crate::app::test_state(dir.path()).await.unwrap();
    let app = crate::app::router(state.clone());
    Harness {
        _dir: dir,
        state,
        app,
    }
}

/// How a request authenticates.
#[derive(Clone, Copy)]
enum As<'a> {
    Nobody,
    Cookie(&'a str),
    Bearer(&'a str),
}

struct Reply {
    status: StatusCode,
    headers: HeaderMap,
    body: Value,
}

impl Reply {
    /// The session token from `Set-Cookie`, if one was set.
    fn cookie(&self) -> Option<String> {
        self.headers
            .get_all(header::SET_COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .find_map(|v| {
                v.strip_prefix("stash=")
                    .map(|rest| rest.split(';').next().unwrap_or_default().to_string())
            })
            .filter(|v| !v.is_empty())
    }
}

impl Harness {
    async fn send(
        &self,
        method: Method,
        path: &str,
        who: As<'_>,
        body: Option<Value>,
        origin: bool,
    ) -> Reply {
        let mut req = Request::builder().method(method).uri(path);
        if origin {
            req = req.header(header::ORIGIN, ORIGIN);
        }
        match who {
            As::Nobody => {}
            As::Cookie(token) => req = req.header(header::COOKIE, format!("stash={token}")),
            As::Bearer(token) => req = req.header(header::AUTHORIZATION, format!("Bearer {token}")),
        }
        let body = match body {
            Some(v) => {
                req = req.header(header::CONTENT_TYPE, "application/json");
                Body::from(v.to_string())
            }
            None => Body::empty(),
        };
        let response = self
            .app
            .clone()
            .oneshot(req.body(body).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let body = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap_or(Value::Null)
        };
        Reply {
            status,
            headers,
            body,
        }
    }

    /// Send a request with exactly these headers and this raw body.
    async fn send_raw(
        &self,
        method: Method,
        path: &str,
        headers: &[(header::HeaderName, &str)],
        body: impl Into<Body>,
    ) -> Reply {
        let mut req = Request::builder().method(method).uri(path);
        for (name, value) in headers {
            req = req.header(name, *value);
        }
        let response = self
            .app
            .clone()
            .oneshot(req.body(body.into()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        Reply {
            status,
            headers,
            body: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        }
    }

    /// Invite and register a member; returns `(cookie, user_id)`.
    async fn add_member(&self, owner: &str, device: &mut Device, name: &str) -> (String, String) {
        let invite = self
            .post("/api/invites", As::Cookie(owner), json!({"role": "member"}))
            .await;
        assert_eq!(invite.status, StatusCode::OK, "{:?}", invite.body);
        let start = self
            .post(
                "/api/auth/invite/start",
                As::Nobody,
                json!({"code": invite.body["code"], "display_name": name}),
            )
            .await;
        assert_eq!(start.status, StatusCode::OK, "{:?}", start.body);
        let credential = device.register(&start.body["options"]);
        let finish = self
            .post(
                "/api/auth/invite/finish",
                As::Nobody,
                json!({"flow_id": start.body["flow_id"], "credential": credential}),
            )
            .await;
        assert_eq!(finish.status, StatusCode::OK, "{:?}", finish.body);
        let id = finish.body["user"]["id"].as_str().unwrap().to_string();
        (finish.cookie().unwrap(), id)
    }

    /// Start pairing as a device; returns the request reply body.
    async fn pairing_request(&self, kind: &str) -> Value {
        let reply = self
            .send(
                Method::POST,
                "/api/pairing/request",
                As::Nobody,
                Some(json!({"client_name": "Phone", "client_kind": kind})),
                false,
            )
            .await;
        assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
        reply.body
    }

    async fn get(&self, path: &str, who: As<'_>) -> Reply {
        self.send(Method::GET, path, who, None, false).await
    }

    async fn post(&self, path: &str, who: As<'_>, body: Value) -> Reply {
        self.send(Method::POST, path, who, Some(body), true).await
    }

    async fn delete(&self, path: &str, who: As<'_>) -> Reply {
        self.send(Method::DELETE, path, who, None, true).await
    }

    /// Run setup with a fresh owner passkey; returns the owner's cookie token.
    async fn setup_owner(&self, device: &mut Device) -> String {
        let code = self.state.auth.ensure_setup_code().await.unwrap().unwrap();
        let start = self
            .post(
                "/api/auth/setup/start",
                As::Nobody,
                json!({"setup_code": code, "display_name": "Ana"}),
            )
            .await;
        assert_eq!(start.status, StatusCode::OK, "{:?}", start.body);
        let credential = device.register(&start.body["options"]);
        let finish = self
            .post(
                "/api/auth/setup/finish",
                As::Nobody,
                json!({"flow_id": start.body["flow_id"], "credential": credential, "passkey_name": "Laptop"}),
            )
            .await;
        assert_eq!(finish.status, StatusCode::OK, "{:?}", finish.body);
        assert_eq!(finish.body["user"]["role"], "owner");
        finish.cookie().expect("session cookie")
    }

    /// Sign in with `device`; returns the reply of login/finish.
    async fn login(&self, device: &mut Device, extra: Value) -> Reply {
        let start = self
            .post("/api/auth/login/start", As::Nobody, json!({}))
            .await;
        assert_eq!(start.status, StatusCode::OK, "{:?}", start.body);
        let credential = device.authenticate(&start.body["options"]);
        let mut body = json!({"flow_id": start.body["flow_id"], "credential": credential});
        if let (Some(map), Some(extra)) = (body.as_object_mut(), extra.as_object()) {
            map.extend(extra.clone());
        }
        self.post("/api/auth/login/finish", As::Nobody, body).await
    }
}

/// A software passkey that behaves like a synced platform passkey.
struct Device {
    authenticator: WebauthnAuthenticator<SoftPasskey>,
    credential_id: Option<String>,
    user_handle: Option<String>,
}

impl Device {
    fn new() -> Self {
        Self {
            authenticator: WebauthnAuthenticator::new(SoftPasskey::new(true)),
            credential_id: None,
            user_handle: None,
        }
    }

    fn register(&mut self, options: &Value) -> Value {
        // The server must ask for a discoverable, user-verified credential.
        let selection = &options["publicKey"]["authenticatorSelection"];
        assert_eq!(selection["residentKey"], "required");
        assert_eq!(selection["requireResidentKey"], true);
        assert_eq!(selection["userVerification"], "required");
        // SoftPasskey cannot store resident keys; it keeps the key itself and we add the user handle on sign-in.
        let mut options = options.clone();
        options["publicKey"]["authenticatorSelection"]["requireResidentKey"] = json!(false);
        options["publicKey"]["authenticatorSelection"]["residentKey"] = json!("discouraged");
        self.user_handle = options["publicKey"]["user"]["id"]
            .as_str()
            .map(str::to_string);
        let ccr: CreationChallengeResponse = serde_json::from_value(options).unwrap();
        let credential = self
            .authenticator
            .do_registration(Url::parse(ORIGIN).unwrap(), ccr)
            .unwrap();
        self.credential_id = Some(URL_SAFE_NO_PAD.encode(credential.raw_id.as_slice()));
        serde_json::to_value(credential).unwrap()
    }

    fn authenticate(&mut self, options: &Value) -> Value {
        let mut options = options.clone();
        assert!(options.get("mediation").is_none());
        // Discoverable sign-in: the server names no credentials. Tell the software key which one to use.
        options["publicKey"]["allowCredentials"] =
            json!([{"type": "public-key", "id": self.credential_id}]);
        let rcr: RequestChallengeResponse = serde_json::from_value(options).unwrap();
        let credential = self
            .authenticator
            .do_authentication(Url::parse(ORIGIN).unwrap(), rcr)
            .unwrap();
        let mut value = serde_json::to_value(credential).unwrap();
        // A real passkey returns the user handle stored at registration.
        value["response"]["userHandle"] = json!(self.user_handle);
        value
    }
}

#[tokio::test]
async fn auth_state_before_and_after_setup() {
    let h = harness().await;
    let state = h.get("/api/auth/state", As::Nobody).await;
    assert_eq!(state.status, StatusCode::OK);
    assert_eq!(state.body["setup_required"], true);
    assert_eq!(state.body["authenticated"], false);
    assert_eq!(state.body["user"], Value::Null);
    assert_eq!(state.body["passkeys_ready"], true);
    assert_eq!(state.body["rp_id"], "localhost");
    assert_eq!(state.body["public_url"], ORIGIN);
    assert_eq!(state.body["reason"], Value::Null);
    // Security headers on every response.
    assert!(
        state.headers[header::CONTENT_SECURITY_POLICY]
            .to_str()
            .unwrap()
            .contains("frame-ancestors 'none'")
    );
    assert_eq!(state.headers[header::X_FRAME_OPTIONS], "DENY");
    assert!(
        !state
            .headers
            .contains_key(header::STRICT_TRANSPORT_SECURITY)
    );

    // No one can sign in before setup.
    let login = h.post("/api/auth/login/start", As::Nobody, json!({})).await;
    assert_eq!(login.status, StatusCode::CONFLICT);
    assert_eq!(login.body["error"]["code"], "setup_required");

    let mut laptop = Device::new();
    let cookie = h.setup_owner(&mut laptop).await;
    let state = h.get("/api/auth/state", As::Nobody).await;
    assert_eq!(state.body["setup_required"], false);
    assert_eq!(state.body["authenticated"], false);
    let state = h.get("/api/auth/state", As::Cookie(&cookie)).await;
    assert_eq!(state.body["authenticated"], true);
    assert_eq!(state.body["user"]["display_name"], "Ana");

    // Setup cannot run twice, even with a fresh code.
    sqlx::query("DELETE FROM recovery_codes")
        .execute(&h.state.db)
        .await
        .unwrap();
    assert!(h.state.auth.ensure_setup_code().await.unwrap().is_none());
    let again = h
        .post(
            "/api/auth/setup/start",
            As::Nobody,
            json!({"setup_code": "AAAA-BBBB-CCCC", "display_name": "Eve"}),
        )
        .await;
    assert_eq!(again.status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn setup_with_wrong_code_fails() {
    let h = harness().await;
    let _code = h.state.auth.ensure_setup_code().await.unwrap().unwrap();
    let reply = h
        .post(
            "/api/auth/setup/start",
            As::Nobody,
            json!({"setup_code": "AAAA-BBBB-CCCC", "display_name": "Eve"}),
        )
        .await;
    assert_eq!(reply.status, StatusCode::FORBIDDEN);
    assert_eq!(reply.body["error"]["code"], "forbidden");
    let (failures,): (i64,) =
        sqlx::query_as("SELECT count(*) FROM audit_log WHERE event = 'auth_failed'")
            .fetch_one(&h.state.db)
            .await
            .unwrap();
    assert_eq!(failures, 1);
    let state = h.get("/api/auth/state", As::Nobody).await;
    assert_eq!(state.body["setup_required"], true);
}

#[tokio::test]
async fn passkey_registration_and_login() {
    let h = harness().await;
    let mut laptop = Device::new();
    let setup_cookie = h.setup_owner(&mut laptop).await;
    let me = h.get("/api/me", As::Cookie(&setup_cookie)).await;
    assert_eq!(me.status, StatusCode::OK);
    assert_eq!(me.body["display_name"], "Ana");

    // No setup code once an owner exists.
    assert!(h.state.auth.ensure_setup_code().await.unwrap().is_none());

    // Browser sign-in sets a cookie.
    let login = h.login(&mut laptop, json!({})).await;
    assert_eq!(login.status, StatusCode::OK, "{:?}", login.body);
    assert_eq!(login.body["user"]["display_name"], "Ana");
    assert!(login.body.get("token").is_none());
    let set_cookie = login.headers[header::SET_COOKIE].to_str().unwrap();
    assert!(
        set_cookie.contains("HttpOnly")
            && set_cookie.contains("SameSite=Strict")
            && set_cookie.contains("Path=/")
    );
    let cookie = login.cookie().unwrap();
    assert!(cookie.starts_with("pst_"));
    assert_eq!(
        h.get("/api/me", As::Cookie(&cookie)).await.status,
        StatusCode::OK
    );

    // last_used_at recorded on the passkey.
    let keys = h.get("/api/me/passkeys", As::Cookie(&cookie)).await;
    assert_eq!(keys.body.as_array().unwrap().len(), 1);
    assert_eq!(keys.body[0]["name"], "Laptop");
    assert!(keys.body[0]["last_used_at"].is_i64());

    // The Android app asks for a bearer token instead of a cookie.
    let app_login = h
        .login(
            &mut laptop,
            json!({"want_token": true, "device_name": "Pixel"}),
        )
        .await;
    assert_eq!(app_login.status, StatusCode::OK, "{:?}", app_login.body);
    assert!(app_login.headers.get(header::SET_COOKIE).is_none());
    assert_eq!(app_login.headers[header::CACHE_CONTROL], "no-store");
    let token = app_login.body["token"].as_str().unwrap().to_string();
    assert!(token.starts_with("pst_") && token.len() == 47);
    assert_eq!(
        h.get("/api/me", As::Bearer(&token)).await.status,
        StatusCode::OK
    );
    // A bearer session token is not a cookie, and the other way round.
    assert_eq!(
        h.get("/api/me", As::Cookie(&token)).await.status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        h.get("/api/me", As::Bearer(&cookie)).await.status,
        StatusCode::UNAUTHORIZED
    );

    let sessions = h.get("/api/me/sessions", As::Cookie(&cookie)).await;
    let list = sessions.body.as_array().unwrap();
    assert_eq!(list.len(), 3);
    assert_eq!(list.iter().filter(|s| s["current"] == true).count(), 1);
    assert!(
        list.iter()
            .any(|s| s["kind"] == "bearer" && s["user_agent"] == "Pixel")
    );

    // Revoke the app session from the browser.
    let app_session = list.iter().find(|s| s["kind"] == "bearer").unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(
        h.delete(
            &format!("/api/me/sessions/{app_session}"),
            As::Cookie(&cookie)
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        h.get("/api/me", As::Bearer(&token)).await.status,
        StatusCode::UNAUTHORIZED
    );

    // Logout ends the session and clears the cookie.
    let out = h
        .post("/api/auth/logout", As::Cookie(&cookie), json!({}))
        .await;
    assert_eq!(out.status, StatusCode::NO_CONTENT);
    assert!(
        out.headers[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .contains("Max-Age=0")
    );
    assert_eq!(
        h.get("/api/me", As::Cookie(&cookie)).await.status,
        StatusCode::UNAUTHORIZED
    );

    // A flow id works once.
    let start = h.post("/api/auth/login/start", As::Nobody, json!({})).await;
    let credential = laptop.authenticate(&start.body["options"]);
    let body = json!({"flow_id": start.body["flow_id"], "credential": credential});
    assert_eq!(
        h.post("/api/auth/login/finish", As::Nobody, body.clone())
            .await
            .status,
        StatusCode::OK
    );
    assert_eq!(
        h.post("/api/auth/login/finish", As::Nobody, body)
            .await
            .status,
        StatusCode::BAD_REQUEST
    );

    // A user handle that belongs to nobody is refused and audited.
    let real_handle = laptop
        .user_handle
        .replace(URL_SAFE_NO_PAD.encode(uuid::Uuid::now_v7().as_bytes()));
    let refused = h.login(&mut laptop, json!({})).await;
    assert_eq!(refused.status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        refused.body["error"]["message"],
        "That passkey was not recognized. Try another one."
    );
    laptop.user_handle = real_handle;
    let (failures,): (i64,) =
        sqlx::query_as("SELECT count(*) FROM audit_log WHERE event = 'auth_failed'")
            .fetch_one(&h.state.db)
            .await
            .unwrap();
    assert_eq!(failures, 1);
}

#[tokio::test]
async fn cookie_post_without_origin_is_rejected() {
    let h = harness().await;
    let mut laptop = Device::new();
    let cookie = h.setup_owner(&mut laptop).await;
    let no_origin = h
        .send(
            Method::POST,
            "/api/invites",
            As::Cookie(&cookie),
            Some(json!({"role": "member"})),
            false,
        )
        .await;
    assert_eq!(no_origin.status, StatusCode::FORBIDDEN);
    assert_eq!(
        no_origin.body["error"]["message"],
        "This request came from another site."
    );

    let mut req = Request::builder()
        .method(Method::POST)
        .uri("/api/invites")
        .header(header::COOKIE, format!("stash={cookie}"))
        .header(header::ORIGIN, "https://evil.example.com")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"role":"member"}"#))
        .unwrap();
    let response = h.app.clone().oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    // Same-site Referer is accepted when Origin is missing.
    req = Request::builder()
        .method(Method::POST)
        .uri("/api/invites")
        .header(header::COOKIE, format!("stash={cookie}"))
        .header(header::REFERER, format!("{ORIGIN}/settings/people"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"role":"member"}"#))
        .unwrap();
    let response = h.app.clone().oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // Public endpoints are covered too (passkey sign-in without any Origin is the one
    // exception, for the Android app: the_android_app_starts_a_passkey_sign_in_without_an_origin).
    let invite = h
        .send(
            Method::POST,
            "/api/auth/invite/start",
            As::Nobody,
            Some(json!({"code": "x", "display_name": "A"})),
            false,
        )
        .await;
    assert_eq!(invite.status, StatusCode::FORBIDDEN);
    // GET needs no Origin.
    assert_eq!(
        h.get("/api/me", As::Cookie(&cookie)).await.status,
        StatusCode::OK
    );
}

#[tokio::test]
async fn bearer_token_scopes_are_enforced() {
    let h = harness().await;
    let mut laptop = Device::new();
    let cookie = h.setup_owner(&mut laptop).await;
    let created = h
        .post(
            "/api/tokens",
            As::Cookie(&cookie),
            json!({"name": "Work laptop", "scopes": ["downloads:create"]}),
        )
        .await;
    assert_eq!(created.status, StatusCode::OK, "{:?}", created.body);
    let token = created.body["token"].as_str().unwrap().to_string();
    assert_eq!(created.headers[header::CACHE_CONTROL], "no-store");
    assert_eq!(created.body["prefix"], token[4..12]);
    assert_eq!(created.body["scopes"], json!(["downloads:create"]));

    // Any-scope routes work; full-scope and owner routes do not.
    assert_eq!(
        h.get("/api/me", As::Bearer(&token)).await.status,
        StatusCode::OK
    );
    let tokens = h.get("/api/tokens", As::Bearer(&token)).await;
    assert_eq!(tokens.status, StatusCode::FORBIDDEN);
    assert_eq!(
        h.get("/api/users", As::Bearer(&token)).await.status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        h.get("/api/audit", As::Bearer(&token)).await.status,
        StatusCode::FORBIDDEN
    );
    // Bearer requests skip the Origin check but still need the scope.
    let invite = h
        .send(
            Method::POST,
            "/api/invites",
            As::Bearer(&token),
            Some(json!({})),
            false,
        )
        .await;
    assert_eq!(invite.status, StatusCode::FORBIDDEN);
    assert_eq!(
        invite.body["error"]["message"],
        "Only the owner can do that."
    );

    // last_used_at is recorded.
    let list = h.get("/api/tokens", As::Cookie(&cookie)).await;
    assert!(list.body[0]["last_used_at"].is_i64());
    assert!(list.body[0].get("token").is_none());

    // Bad scopes are refused.
    let bad = h
        .post(
            "/api/tokens",
            As::Cookie(&cookie),
            json!({"name": "x", "scopes": ["root"]}),
        )
        .await;
    assert_eq!(bad.status, StatusCode::BAD_REQUEST);

    // Revoked tokens stop working.
    let id = created.body["id"].as_str().unwrap();
    assert_eq!(
        h.delete(&format!("/api/tokens/{id}"), As::Cookie(&cookie))
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        h.get("/api/me", As::Bearer(&token)).await.status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        h.get("/api/me", As::Bearer("pst_nonsense")).await.status,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn pairing_delivers_the_token_once() {
    let h = harness().await;
    let mut laptop = Device::new();
    let cookie = h.setup_owner(&mut laptop).await;
    // The extension calls from its own origin; device endpoints accept that.
    let request = Request::builder()
        .method(Method::POST)
        .uri("/api/pairing/request")
        .header(header::ORIGIN, "chrome-extension://abcdefghijklmnop")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({"client_name": "Chrome on Mac", "client_kind": "chrome-extension"}).to_string(),
        ))
        .unwrap();
    let response = h.app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let req = Reply {
        status: StatusCode::OK,
        headers: HeaderMap::new(),
        body: serde_json::from_slice(&bytes).unwrap(),
    };
    assert_eq!(req.body["interval"], 6);
    let user_code = req.body["user_code"].as_str().unwrap().to_string();
    assert_eq!(user_code.len(), 9);
    assert_eq!(
        req.body["verify_url"],
        format!("{ORIGIN}/pair?code={user_code}")
    );
    let poll_body =
        json!({"pairing_id": req.body["pairing_id"], "poll_secret": req.body["poll_secret"]});

    // Native apps send no Origin at all.
    let pending = h
        .send(
            Method::POST,
            "/api/pairing/poll",
            As::Nobody,
            Some(poll_body.clone()),
            false,
        )
        .await;
    assert_eq!(pending.status, StatusCode::ACCEPTED);
    assert_eq!(pending.body["status"], "pending");
    let wrong = h
        .post(
            "/api/pairing/poll",
            As::Nobody,
            json!({"pairing_id": req.body["pairing_id"], "poll_secret": "guess"}),
        )
        .await;
    assert_eq!(wrong.status, StatusCode::NOT_FOUND);

    let list = h.get("/api/pairing/pending", As::Cookie(&cookie)).await;
    assert_eq!(list.body[0]["user_code"], user_code);
    assert_eq!(list.body[0]["client_kind"], "chrome-extension");

    let approve = h
        .post(
            "/api/pairing/approve",
            As::Cookie(&cookie),
            json!({"user_code": user_code.to_lowercase()}),
        )
        .await;
    assert_eq!(approve.status, StatusCode::OK, "{:?}", approve.body);
    assert_eq!(approve.body["ok"], true);

    let delivered = h
        .post("/api/pairing/poll", As::Nobody, poll_body.clone())
        .await;
    assert_eq!(delivered.status, StatusCode::OK, "{:?}", delivered.body);
    assert_eq!(delivered.body["status"], "approved");
    assert_eq!(
        delivered.body["scopes"],
        json!(["downloads:create", "downloads:read"])
    );
    assert_eq!(delivered.body["server_name"], "localhost");
    let token = delivered.body["token"].as_str().unwrap().to_string();

    let again = h.post("/api/pairing/poll", As::Nobody, poll_body).await;
    assert_eq!(again.status, StatusCode::GONE);
    assert!(again.body.get("token").is_none());
    let (stored,): (Option<String>,) = sqlx::query_as("SELECT pending_token FROM pairing_requests")
        .fetch_one(&h.state.db)
        .await
        .unwrap();
    assert!(stored.is_none());

    // The device token works with its scopes and appears in the owner's devices.
    assert_eq!(
        h.get("/api/me", As::Bearer(&token)).await.status,
        StatusCode::OK
    );
    assert_eq!(
        h.get("/api/pairing/pending", As::Bearer(&token))
            .await
            .status,
        StatusCode::FORBIDDEN
    );
    let devices = h.get("/api/tokens", As::Cookie(&cookie)).await;
    assert_eq!(devices.body[0]["client_kind"], "chrome-extension");
    assert_eq!(devices.body[0]["name"], "Chrome on Mac");
    let (paired,): (i64,) =
        sqlx::query_as("SELECT count(*) FROM audit_log WHERE event = 'device_paired'")
            .fetch_one(&h.state.db)
            .await
            .unwrap();
    assert_eq!(paired, 1);

    // Denied requests answer 410 denied.
    let req = h
        .post(
            "/api/pairing/request",
            As::Nobody,
            json!({"client_name": "Phone", "client_kind": "android"}),
        )
        .await;
    let code = req.body["user_code"].as_str().unwrap();
    assert_eq!(
        h.post(
            "/api/pairing/deny",
            As::Cookie(&cookie),
            json!({"user_code": code})
        )
        .await
        .status,
        StatusCode::OK
    );
    let denied = h
        .post(
            "/api/pairing/poll",
            As::Nobody,
            json!({"pairing_id": req.body["pairing_id"], "poll_secret": req.body["poll_secret"]}),
        )
        .await;
    assert_eq!(denied.status, StatusCode::GONE);
    assert_eq!(denied.body["status"], "denied");
    let bad_kind = h
        .post(
            "/api/pairing/request",
            As::Nobody,
            json!({"client_name": "x", "client_kind": "toaster"}),
        )
        .await;
    assert_eq!(bad_kind.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn invite_creates_a_member() {
    let h = harness().await;
    let mut laptop = Device::new();
    let owner = h.setup_owner(&mut laptop).await;
    let invite = h
        .post(
            "/api/invites",
            As::Cookie(&owner),
            json!({"role": "member", "expires_in_hours": 48}),
        )
        .await;
    assert_eq!(invite.status, StatusCode::OK, "{:?}", invite.body);
    let code = invite.body["code"].as_str().unwrap().to_string();
    assert_eq!(code.len(), 22);
    assert_eq!(invite.body["url"], format!("{ORIGIN}/invite/{code}"));

    let info = h.get(&format!("/api/invites/{code}"), As::Nobody).await;
    assert_eq!(info.body["valid"], true);
    assert_eq!(info.body["role"], "member");
    let unknown = h.get("/api/invites/not-a-code", As::Nobody).await;
    assert_eq!(
        unknown.body,
        json!({"valid": false, "role": null, "expires_at": null})
    );

    let mut phone = Device::new();
    let start = h
        .post(
            "/api/auth/invite/start",
            As::Nobody,
            json!({"code": code, "display_name": "Ben"}),
        )
        .await;
    assert_eq!(start.status, StatusCode::OK, "{:?}", start.body);
    let credential = phone.register(&start.body["options"]);
    let finish = h
        .post(
            "/api/auth/invite/finish",
            As::Nobody,
            json!({"flow_id": start.body["flow_id"], "credential": credential}),
        )
        .await;
    assert_eq!(finish.status, StatusCode::OK, "{:?}", finish.body);
    assert_eq!(finish.body["user"]["role"], "member");
    let member = finish.cookie().unwrap();
    let me = h.get("/api/me", As::Cookie(&member)).await;
    assert_eq!(me.body["display_name"], "Ben");

    // Single use.
    let info = h.get(&format!("/api/invites/{code}"), As::Nobody).await;
    assert_eq!(info.body["valid"], false);
    let reuse = h
        .post(
            "/api/auth/invite/start",
            As::Nobody,
            json!({"code": code, "display_name": "Cy"}),
        )
        .await;
    assert_eq!(reuse.status, StatusCode::NOT_FOUND);

    // Members cannot manage people; the member signs in with their own passkey.
    assert_eq!(
        h.get("/api/users", As::Cookie(&member)).await.status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        h.login(&mut phone, json!({})).await.body["user"]["display_name"],
        "Ben"
    );
    let people = h.get("/api/users", As::Cookie(&owner)).await;
    assert_eq!(people.body.as_array().unwrap().len(), 2);
    let invites = h.get("/api/invites", As::Cookie(&owner)).await;
    assert_eq!(invites.body[0]["status"], "used");
    assert_eq!(invites.body[0]["used_by_name"], "Ben");

    // The last owner cannot be demoted or removed; members can be promoted and removed.
    let owner_id = h.get("/api/me", As::Cookie(&owner)).await.body["id"]
        .as_str()
        .unwrap()
        .to_string();
    let member_id = me.body["id"].as_str().unwrap().to_string();
    let demote = h
        .send(
            Method::PATCH,
            &format!("/api/users/{owner_id}"),
            As::Cookie(&owner),
            Some(json!({"role": "member"})),
            true,
        )
        .await;
    assert_eq!(demote.status, StatusCode::CONFLICT);
    assert_eq!(
        h.delete(&format!("/api/users/{owner_id}"), As::Cookie(&owner))
            .await
            .status,
        StatusCode::CONFLICT
    );
    let promote = h
        .send(
            Method::PATCH,
            &format!("/api/users/{member_id}"),
            As::Cookie(&owner),
            Some(json!({"role": "owner"})),
            true,
        )
        .await;
    assert_eq!(promote.body["role"], "owner");
    let demote = h
        .send(
            Method::PATCH,
            &format!("/api/users/{member_id}"),
            As::Cookie(&owner),
            Some(json!({"role": "member"})),
            true,
        )
        .await;
    assert_eq!(demote.status, StatusCode::OK);
    assert_eq!(
        h.delete(&format!("/api/users/{member_id}"), As::Cookie(&owner))
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        h.get("/api/me", As::Cookie(&member)).await.status,
        StatusCode::UNAUTHORIZED
    );

    // The audit log shows who did what, newest first, with pagination.
    let page = h.get("/api/audit?limit=2", As::Cookie(&owner)).await;
    assert_eq!(page.status, StatusCode::OK);
    assert_eq!(page.body["items"].as_array().unwrap().len(), 2);
    assert_eq!(page.body["items"][0]["event"], "user_removed");
    assert_eq!(page.body["items"][0]["user"]["display_name"], "Ana");
    let cursor = page.body["next_cursor"].as_str().unwrap();
    let next = h
        .get(
            &format!("/api/audit?limit=50&cursor={cursor}"),
            As::Cookie(&owner),
        )
        .await;
    let first_next = next.body["items"][0]["id"].as_i64().unwrap();
    assert!(first_next < page.body["items"][1]["id"].as_i64().unwrap());
    assert!(
        next.body["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["event"] == "invite_accepted")
    );
}

#[tokio::test]
async fn last_passkey_cannot_be_deleted() {
    let h = harness().await;
    let mut laptop = Device::new();
    let cookie = h.setup_owner(&mut laptop).await;
    let keys = h.get("/api/me/passkeys", As::Cookie(&cookie)).await;
    let first = keys.body[0]["id"].as_str().unwrap().to_string();
    let refused = h
        .delete(&format!("/api/me/passkeys/{first}"), As::Cookie(&cookie))
        .await;
    assert_eq!(refused.status, StatusCode::CONFLICT);

    // Add a second passkey, rename it, then the first can go.
    let mut phone = Device::new();
    let start = h
        .post("/api/me/passkeys/start", As::Cookie(&cookie), json!({}))
        .await;
    assert_eq!(start.status, StatusCode::OK, "{:?}", start.body);
    let exclude = start.body["options"]["publicKey"]["excludeCredentials"]
        .as_array()
        .unwrap();
    assert_eq!(exclude.len(), 1);
    let credential = phone.register(&start.body["options"]);
    let added = h
        .post(
            "/api/me/passkeys/finish",
            As::Cookie(&cookie),
            json!({"flow_id": start.body["flow_id"], "credential": credential}),
        )
        .await;
    assert_eq!(added.status, StatusCode::OK, "{:?}", added.body);
    let second = added.body["id"].as_str().unwrap().to_string();
    let renamed = h
        .send(
            Method::PATCH,
            &format!("/api/me/passkeys/{second}"),
            As::Cookie(&cookie),
            Some(json!({"name": "Phone"})),
            true,
        )
        .await;
    assert_eq!(renamed.body["name"], "Phone");
    assert_eq!(
        h.delete(&format!("/api/me/passkeys/{first}"), As::Cookie(&cookie))
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        h.delete(&format!("/api/me/passkeys/{second}"), As::Cookie(&cookie))
            .await
            .status,
        StatusCode::CONFLICT
    );
    // The phone passkey signs in; the removed laptop passkey does not.
    assert_eq!(h.login(&mut phone, json!({})).await.status, StatusCode::OK);
    let refused = h.login(&mut laptop, json!({})).await;
    assert_eq!(refused.status, StatusCode::UNAUTHORIZED);
    assert_eq!(refused.body["error"]["code"], "unauthorized");
}

#[tokio::test]
async fn recovery_link_works_once() {
    let h = harness().await;
    let mut laptop = Device::new();
    h.setup_owner(&mut laptop).await;
    let link = super::create_owner_reset_link(&h.state.db, &h.state.config)
        .await
        .unwrap();
    let code = link.rsplit('/').next().unwrap().to_string();

    let mut new_phone = Device::new();
    let start = h
        .post("/api/auth/recover/start", As::Nobody, json!({"code": code}))
        .await;
    assert_eq!(start.status, StatusCode::OK, "{:?}", start.body);
    // Same account: the new passkey carries the owner's user handle.
    assert_eq!(
        start.body["options"]["publicKey"]["user"]["id"].as_str(),
        laptop.user_handle.as_deref()
    );
    let credential = new_phone.register(&start.body["options"]);
    let finish = h
        .post(
            "/api/auth/recover/finish",
            As::Nobody,
            json!({"flow_id": start.body["flow_id"], "credential": credential}),
        )
        .await;
    assert_eq!(finish.status, StatusCode::OK, "{:?}", finish.body);
    assert_eq!(finish.body["user"]["role"], "owner");
    let cookie = finish.cookie().unwrap();
    assert_eq!(
        h.get("/api/me/passkeys", As::Cookie(&cookie))
            .await
            .body
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        h.login(&mut new_phone, json!({})).await.status,
        StatusCode::OK
    );

    let again = h
        .post("/api/auth/recover/start", As::Nobody, json!({"code": code}))
        .await;
    assert_eq!(again.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn the_android_app_starts_a_passkey_sign_in_without_an_origin() {
    let h = harness().await;
    let mut laptop = Device::new();
    h.setup_owner(&mut laptop).await;
    // The Android app is not a browser: no Origin, no Referer, a JSON body.
    let native = h
        .send(
            Method::POST,
            "/api/auth/login/start",
            As::Nobody,
            Some(json!({})),
            false,
        )
        .await;
    assert_eq!(native.status, StatusCode::OK, "{:?}", native.body);
    assert!(native.body["options"]["publicKey"]["challenge"].is_string());
    // A finish with a made-up credential is refused by the passkey check, not as cross-site.
    let finish = h
        .send(
            Method::POST,
            "/api/auth/login/finish",
            As::Nobody,
            Some(json!({"flow_id": native.body["flow_id"], "credential": {}, "want_token": true})),
            false,
        )
        .await;
    assert_ne!(finish.status, StatusCode::FORBIDDEN, "{:?}", finish.body);
    // A browser on another site sends its Origin and is refused as before.
    let forged = Request::builder()
        .method(Method::POST)
        .uri("/api/auth/login/start")
        .header(header::ORIGIN, "https://evil.example")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from("{}"))
        .unwrap();
    let response = h.app.clone().oneshot(forged).await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    // Without an Origin the body must still be JSON.
    let text = Request::builder()
        .method(Method::POST)
        .uri("/api/auth/login/start")
        .header(header::CONTENT_TYPE, "text/plain")
        .body(Body::from("{}"))
        .unwrap();
    let response = h.app.clone().oneshot(text).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    // Setup, invites and recovery still need the origin.
    let setup = h
        .send(
            Method::POST,
            "/api/auth/setup/start",
            As::Nobody,
            Some(json!({"setup_code": "x", "display_name": "A"})),
            false,
        )
        .await;
    assert_eq!(setup.status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn passkeys_unavailable_without_https() {
    let dir = tempfile::tempdir().unwrap();
    let stash = dir.path().join("stash");
    std::fs::create_dir_all(&stash).unwrap();
    let mut config = stash_core::config::Config::for_tests(&stash, &dir.path().join("config"));
    config.public_url = Some(url::Url::parse("http://192.168.1.20:8788").unwrap());
    config.extra_origins = vec![url::Url::parse(ORIGIN).unwrap()];
    let state = crate::app::build_state(config).await.unwrap();
    let h = Harness {
        _dir: dir,
        app: crate::app::router(state.clone()),
        state,
    };
    let auth = h.get("/api/auth/state", As::Nobody).await;
    assert_eq!(auth.body["passkeys_ready"], false);
    assert!(
        auth.body["reason"]
            .as_str()
            .unwrap()
            .contains("STASH_PUBLIC_URL")
    );
    let start = h.post("/api/auth/login/start", As::Nobody, json!({})).await;
    assert_eq!(start.status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(start.body["error"]["code"], "passkeys_unavailable");
    assert!(
        start.body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("HTTPS")
    );
}

#[tokio::test]
async fn auth_endpoints_are_rate_limited() {
    let h = harness().await;
    let mut last = StatusCode::OK;
    for _ in 0..25 {
        last = h
            .post(
                "/api/auth/setup/start",
                As::Nobody,
                json!({"setup_code": "AAAA-BBBB-CCCC", "display_name": "Eve"}),
            )
            .await
            .status;
    }
    assert_eq!(last, StatusCode::TOO_MANY_REQUESTS);
}

/// The JSON error body every client parses: `{"error": {"code", "message"}}`.
fn assert_api_error(reply: &Reply, status: StatusCode, code: &str) {
    assert_eq!(reply.status, status, "{:?}", reply.body);
    assert_eq!(reply.body["error"]["code"], code, "{:?}", reply.body);
    assert!(
        reply.body["error"]["message"]
            .as_str()
            .is_some_and(|m| m.ends_with('.')),
        "{:?}",
        reply.body
    );
}

#[tokio::test]
async fn malformed_requests_get_the_json_error_shape() {
    let h = harness().await;
    let mut laptop = Device::new();
    let owner = h.setup_owner(&mut laptop).await;
    let json_type = (header::CONTENT_TYPE, "application/json");
    let origin = (header::ORIGIN, ORIGIN);
    let cookie_header = format!("stash={owner}");
    let cookie = (header::COOKIE, cookie_header.as_str());

    // Public device endpoints: wrong field types, broken JSON, missing content type.
    let reply = h
        .send_raw(
            Method::POST,
            "/api/pairing/request",
            std::slice::from_ref(&json_type),
            r#"{"client_name": 5, "client_kind": "other"}"#,
        )
        .await;
    assert_api_error(&reply, StatusCode::BAD_REQUEST, "bad_request");
    let reply = h
        .send_raw(
            Method::POST,
            "/api/pairing/poll",
            std::slice::from_ref(&json_type),
            "{not json",
        )
        .await;
    assert_api_error(&reply, StatusCode::BAD_REQUEST, "bad_request");
    let reply = h
        .send_raw(
            Method::POST,
            "/api/pairing/request",
            &[],
            r#"{"client_name": "x", "client_kind": "other"}"#,
        )
        .await;
    assert_api_error(&reply, StatusCode::BAD_REQUEST, "bad_request");

    // Signed-in endpoints.
    for (method, path, body) in [
        (Method::POST, "/api/pairing/approve", "{}"),
        (Method::POST, "/api/pairing/deny", r#"{"user_code": 7}"#),
        (Method::POST, "/api/tokens", r#"{"name": "x"}"#),
        (Method::POST, "/api/invites", r#"{"role": "king"}"#),
        (Method::PATCH, "/api/users/someone", r#"{"role": "admin"}"#),
    ] {
        let reply = h
            .send_raw(
                method,
                path,
                &[json_type.clone(), origin.clone(), cookie.clone()],
                body,
            )
            .await;
        assert_api_error(&reply, StatusCode::BAD_REQUEST, "bad_request");
    }
    let reply = h
        .send_raw(
            Method::GET,
            "/api/audit?limit=lots",
            std::slice::from_ref(&cookie),
            Body::empty(),
        )
        .await;
    assert_api_error(&reply, StatusCode::BAD_REQUEST, "bad_request");

    // Oversized bodies are 413, not a confusing "not understood".
    let huge = format!(r#"{{"flow_id": "{}"}}"#, "x".repeat(3 * 1024 * 1024));
    let reply = h
        .send_raw(
            Method::POST,
            "/api/auth/login/finish",
            &[json_type.clone(), origin.clone()],
            huge,
        )
        .await;
    assert_api_error(&reply, StatusCode::PAYLOAD_TOO_LARGE, "payload_too_large");
}

#[tokio::test]
async fn unsafe_requests_with_a_body_must_be_json_or_an_upload() {
    let h = harness().await;
    let mut laptop = Device::new();
    let owner = h.setup_owner(&mut laptop).await;
    let origin = (header::ORIGIN, ORIGIN);
    let cookie_header = format!("stash={owner}");
    let cookie = (header::COOKIE, cookie_header.as_str());

    // login/start reads its optional body itself, so only the guard can enforce the type.
    for content_type in ["text/plain", "application/x-www-form-urlencoded"] {
        let reply = h
            .send_raw(
                Method::POST,
                "/api/auth/login/start",
                &[origin.clone(), (header::CONTENT_TYPE, content_type)],
                "{}",
            )
            .await;
        assert_api_error(&reply, StatusCode::BAD_REQUEST, "bad_request");
    }
    let reply = h
        .send_raw(
            Method::POST,
            "/api/auth/login/start",
            std::slice::from_ref(&origin),
            "{}",
        )
        .await;
    assert_api_error(&reply, StatusCode::BAD_REQUEST, "bad_request");
    let reply = h
        .send_raw(
            Method::POST,
            "/api/auth/login/start",
            &[origin.clone(), (header::CONTENT_TYPE, "application/json")],
            "{}",
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);

    // Body-less unsafe requests need no content type.
    let reply = h
        .send_raw(
            Method::POST,
            "/api/auth/logout",
            &[origin.clone(), cookie.clone()],
            Body::empty(),
        )
        .await;
    assert_eq!(reply.status, StatusCode::NO_CONTENT, "{:?}", reply.body);
}

#[tokio::test]
async fn forwarded_for_cannot_dodge_the_rate_limit() {
    let dir = tempfile::tempdir().unwrap();
    let stash = dir.path().join("stash");
    std::fs::create_dir_all(&stash).unwrap();
    let mut config = stash_core::config::Config::for_tests(&stash, &dir.path().join("config"));
    config.trust_proxy = true;
    let state = crate::app::build_state(config).await.unwrap();
    let h = Harness {
        _dir: dir,
        app: crate::app::router(state.clone()),
        state,
    };
    let mut statuses = Vec::new();
    for i in 0..25 {
        // The client invents a new address each time; the proxy appends the real one.
        let forwarded = format!("198.51.100.{i}, 203.0.113.9");
        let reply = h
            .send_raw(
                Method::POST,
                "/api/auth/setup/start",
                &[
                    (header::ORIGIN, ORIGIN),
                    (header::CONTENT_TYPE, "application/json"),
                    (
                        header::HeaderName::from_static("x-forwarded-for"),
                        &forwarded,
                    ),
                ],
                r#"{"setup_code": "AAAA-BBBB-CCCC", "display_name": "Eve"}"#,
            )
            .await;
        statuses.push(reply.status);
    }
    assert_eq!(statuses.last(), Some(&StatusCode::TOO_MANY_REQUESTS));
    let (ip,): (Option<String>,) =
        sqlx::query_as("SELECT ip FROM audit_log WHERE event = 'auth_failed' LIMIT 1")
            .fetch_one(&h.state.db)
            .await
            .unwrap();
    assert_eq!(ip.as_deref(), Some("203.0.113.9"));
}

#[tokio::test]
async fn uncollected_pairing_tokens_stop_working() {
    let h = harness().await;
    let mut laptop = Device::new();
    let owner = h.setup_owner(&mut laptop).await;

    // Collected normally: the device keeps a token with no expiry.
    let collected = h.pairing_request("android").await;
    let approve = h
        .post(
            "/api/pairing/approve",
            As::Cookie(&owner),
            json!({"user_code": collected["user_code"]}),
        )
        .await;
    assert_eq!(approve.status, StatusCode::OK);
    let poll =
        json!({"pairing_id": collected["pairing_id"], "poll_secret": collected["poll_secret"]});
    let delivered = h.post("/api/pairing/poll", As::Nobody, poll).await;
    assert_eq!(delivered.status, StatusCode::OK, "{:?}", delivered.body);
    let token = delivered.body["token"].as_str().unwrap();
    let (expiry,): (Option<i64>,) =
        sqlx::query_as("SELECT t.expires_at FROM api_tokens t JOIN pairing_requests p ON p.token_id = t.id WHERE p.id = ?")
            .bind(collected["pairing_id"].as_str())
            .fetch_one(&h.state.db)
            .await
            .unwrap();
    assert_eq!(expiry, None);
    assert_eq!(
        h.get("/api/me", As::Bearer(token)).await.status,
        StatusCode::OK
    );

    // Approved but never collected: the waiting token must not live on in the database.
    let abandoned = h.pairing_request("android").await;
    let approve = h
        .post(
            "/api/pairing/approve",
            As::Cookie(&owner),
            json!({"user_code": abandoned["user_code"]}),
        )
        .await;
    assert_eq!(approve.status, StatusCode::OK);
    let id = abandoned["pairing_id"].as_str().unwrap();
    let (waiting, token_id): (Option<String>, String) =
        sqlx::query_as("SELECT pending_token, token_id FROM pairing_requests WHERE id = ?")
            .bind(id)
            .fetch_one(&h.state.db)
            .await
            .unwrap();
    let waiting = waiting.unwrap();
    let (expiry,): (Option<i64>,) =
        sqlx::query_as("SELECT expires_at FROM api_tokens WHERE id = ?")
            .bind(&token_id)
            .fetch_one(&h.state.db)
            .await
            .unwrap();
    let (request_expiry,): (i64,) =
        sqlx::query_as("SELECT expires_at FROM pairing_requests WHERE id = ?")
            .bind(id)
            .fetch_one(&h.state.db)
            .await
            .unwrap();
    assert_eq!(
        expiry,
        Some(request_expiry + super::pairing::DELIVERY_GRACE_S)
    );

    // An hour passes. Nobody polls and nobody starts another pairing.
    for table in ["pairing_requests", "api_tokens"] {
        sqlx::query(&format!(
            "UPDATE {table} SET expires_at = expires_at - 3600 WHERE expires_at IS NOT NULL"
        ))
        .execute(&h.state.db)
        .await
        .unwrap();
    }
    assert_eq!(
        h.get("/api/me", As::Bearer(&waiting)).await.status,
        StatusCode::UNAUTHORIZED
    );
    let poll = json!({"pairing_id": id, "poll_secret": abandoned["poll_secret"]});
    let late = h.post("/api/pairing/poll", As::Nobody, poll).await;
    assert_eq!(late.status, StatusCode::GONE);
    assert_eq!(late.body["status"], "expired");
    // The collected device is unaffected.
    assert_eq!(
        h.get("/api/me", As::Bearer(token)).await.status,
        StatusCode::OK
    );
}

#[tokio::test]
async fn waiting_pairing_requests_are_capped() {
    let h = harness().await;
    let now = stash_core::now();
    for _ in 0..super::pairing::MAX_PENDING_PAIRINGS {
        sqlx::query(
            "INSERT INTO pairing_requests (id, user_code, poll_secret_hash, client_name, client_kind, status, created_at, expires_at) \
             VALUES (?, ?, 'x', 'Spam', 'android', 'pending', ?, ?)",
        )
        .bind(stash_core::new_id())
        .bind(super::codes::new_user_code())
        .bind(now)
        .bind(now + 600)
        .execute(&h.state.db)
        .await
        .unwrap();
    }
    let full = h
        .send(
            Method::POST,
            "/api/pairing/request",
            As::Nobody,
            Some(json!({"client_name": "Phone", "client_kind": "android"})),
            false,
        )
        .await;
    assert_api_error(&full, StatusCode::TOO_MANY_REQUESTS, "rate_limited");
    // Expired requests do not count.
    sqlx::query("UPDATE pairing_requests SET expires_at = ? WHERE id = (SELECT id FROM pairing_requests LIMIT 1)")
        .bind(now - 1)
        .execute(&h.state.db)
        .await
        .unwrap();
    h.pairing_request("android").await;
}

#[tokio::test]
async fn owner_cannot_mint_full_access_for_someone_else() {
    let h = harness().await;
    let mut laptop = Device::new();
    let owner = h.setup_owner(&mut laptop).await;
    let mut phone = Device::new();
    let (member, member_id) = h.add_member(&owner, &mut phone, "Ben").await;

    // Full access would let the owner act as Ben, including adding a passkey to his account.
    for scopes in [json!(["full"]), json!(["library:read", "full"])] {
        let reply = h
            .post(
                "/api/tokens",
                As::Cookie(&owner),
                json!({"name": "Ben's laptop", "scopes": scopes, "user_id": member_id}),
            )
            .await;
        assert_api_error(&reply, StatusCode::FORBIDDEN, "forbidden");
    }
    let (count,): (i64,) = sqlx::query_as("SELECT count(*) FROM api_tokens WHERE user_id = ?")
        .bind(&member_id)
        .fetch_one(&h.state.db)
        .await
        .unwrap();
    assert_eq!(count, 0);

    // A limited device for someone else is still fine.
    let reply = h
        .post(
            "/api/tokens",
            As::Cookie(&owner),
            json!({"name": "Ben's Chrome", "scopes": ["downloads:create"], "user_id": member_id}),
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    let token = reply.body["token"].as_str().unwrap();
    let me = h.get("/api/me", As::Bearer(token)).await;
    assert_eq!(me.body["id"], member_id);

    // Members only ever create devices for themselves.
    let owner_id = h.get("/api/me", As::Cookie(&owner)).await.body["id"].clone();
    let reply = h
        .post(
            "/api/tokens",
            As::Cookie(&member),
            json!({"name": "x", "scopes": ["downloads:create"], "user_id": owner_id}),
        )
        .await;
    assert_api_error(&reply, StatusCode::FORBIDDEN, "forbidden");
    // Their own full-access device works.
    let reply = h
        .post(
            "/api/tokens",
            As::Cookie(&member),
            json!({"name": "Ben's tablet", "scopes": ["full"]}),
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
}

#[tokio::test]
async fn browser_idle_rejects_polls_and_requires_a_fresh_passkey() {
    use super::sessions::BROWSER_IDLE_S;
    let h = harness().await;
    let mut device = Device::new();
    let cookie = h.setup_owner(&mut device).await;
    let initial = h.get("/api/auth/state", As::Cookie(&cookie)).await;
    assert_eq!(
        initial.body["idle_expires_at"].as_i64().unwrap()
            - initial.body["server_time"].as_i64().unwrap(),
        BROWSER_IDLE_S
    );
    // Polling touches last_seen_at, never human activity.
    let activity = stash_core::now() - BROWSER_IDLE_S + 5;
    sqlx::query("UPDATE sessions SET last_activity_at = ?, last_seen_at = last_seen_at - 600")
        .bind(activity)
        .execute(&h.state.db)
        .await
        .unwrap();
    assert_eq!(
        h.get("/api/me", As::Cookie(&cookie)).await.status,
        StatusCode::OK
    );
    let actual: i64 = sqlx::query_scalar("SELECT last_activity_at FROM sessions")
        .fetch_one(&h.state.db)
        .await
        .unwrap();
    assert_eq!(actual, activity);
    // Only the explicit activity endpoint extends the deadline.
    let touched = h
        .post("/api/auth/activity", As::Cookie(&cookie), json!({}))
        .await;
    assert_eq!(touched.status, StatusCode::OK);
    assert!(touched.body["idle_expires_at"].as_i64().unwrap() > activity + BROWSER_IDLE_S);
    sqlx::query("UPDATE sessions SET last_activity_at = ?")
        .bind(stash_core::now() - BROWSER_IDLE_S)
        .execute(&h.state.db)
        .await
        .unwrap();
    assert_eq!(
        h.post("/api/auth/activity", As::Cookie(&cookie), json!({}))
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        h.get("/api/me", As::Cookie(&cookie)).await.status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        h.get("/api/auth/state", As::Cookie(&cookie)).await.body["authenticated"],
        false
    );
    let signed_in = h.login(&mut device, json!({})).await;
    assert_eq!(signed_in.status, StatusCode::OK);
    assert_eq!(
        h.get("/api/me", As::Cookie(&signed_in.cookie().unwrap()))
            .await
            .status,
        StatusCode::OK
    );
}

#[tokio::test]
async fn browser_idle_does_not_expire_android_or_device_tokens() {
    let h = harness().await;
    let mut device = Device::new();
    h.setup_owner(&mut device).await;
    let login = h.login(&mut device, json!({"want_token": true})).await;
    let bearer = login.body["token"].as_str().unwrap();
    sqlx::query("UPDATE sessions SET last_activity_at = 0")
        .execute(&h.state.db)
        .await
        .unwrap();
    assert_eq!(
        h.get("/api/me", As::Bearer(bearer)).await.status,
        StatusCode::OK
    );
    let state = h.get("/api/auth/state", As::Bearer(bearer)).await;
    assert_eq!(state.body["authenticated"], true);
    assert_eq!(state.body["idle_expires_at"], Value::Null);
    assert_eq!(
        h.post("/api/auth/activity", As::Bearer(bearer), json!({}))
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
}
