use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use base64::engine::general_purpose::STANDARD as base64_engine;
use base64::Engine;
use bytes::Bytes;
use hyper::header::{HeaderMap, HeaderName, HeaderValue, AUTHORIZATION, HOST};
use hyper::http::uri::InvalidUri;
use hyper::{Method, Request, Response, StatusCode, Uri};
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use hyperlocal::{UnixConnector, Uri as UnixUri};
use thiserror::Error;

const LOCAL_API_HOST: &str = "local-tailscaled.sock";
const SEC_TAILSCALE_HEADER: &str = "sec-tailscale";

type RequestBody = http_body_util::Full<Bytes>;

#[derive(Debug, Error)]
pub enum LocalApiError {
    #[error("invalid request URI: {0}")]
    InvalidUri(#[from] InvalidUri),
    #[error("http error: {0}")]
    Http(#[from] hyper_util::client::legacy::Error),
    #[error("hyper error: {0}")]
    Hyper(#[from] hyper::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("localapi password file {path} must have 0600 permissions (got {mode:03o})")]
    PasswordPermissions { path: PathBuf, mode: u32 },
    #[error("localapi password file {path} could not be read: {source}")]
    PasswordRead {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("localapi password file {path} is empty")]
    EmptyPasswordFile { path: PathBuf },
    #[error("invalid header value for {name}")]
    InvalidHeaderValue { name: &'static str },
    #[error("unexpected status {status} for {method} {path}: {body}")]
    HttpStatus {
        status: StatusCode,
        method: Method,
        path: String,
        body: String,
    },
    #[error("watch-ipn-bus did not provide a session id")]
    MissingSessionId,
}

pub struct TransportRequest {
    pub method: Method,
    pub path: String,
    pub headers: HeaderMap,
    pub body: Option<Vec<u8>>,
}

impl TransportRequest {
    pub fn new(method: Method, path: impl Into<String>) -> Self {
        let mut path = path.into();
        if !path.starts_with('/') {
            path = format!("/{path}");
        }
        Self {
            method,
            path,
            headers: HeaderMap::new(),
            body: None,
        }
    }

    pub fn with_body(mut self, body: Vec<u8>) -> Self {
        self.body = Some(body);
        self
    }

    fn build_request(
        &self,
        uri: Uri,
        extra_headers: HeaderMap,
    ) -> Result<Request<RequestBody>, LocalApiError> {
        let mut builder = Request::builder().method(self.method.clone()).uri(uri);
        let headers = builder
            .headers_mut()
            .ok_or(LocalApiError::InvalidHeaderValue { name: "request" })?;
        headers.extend(self.headers.clone());
        headers.extend(extra_headers);
        let body = match &self.body {
            Some(bytes) => RequestBody::new(Bytes::copy_from_slice(bytes)),
            None => RequestBody::new(Bytes::new()),
        };
        let request = builder
            .body(body)
            .map_err(|_| LocalApiError::InvalidHeaderValue { name: "request" })?;
        Ok(request)
    }
}

#[derive(Clone)]
pub enum LocalApiTransport {
    UnixSocket(UnixSocketTransport),
    TcpAuth(TcpAuthTransport),
}

impl LocalApiTransport {
    pub fn unix_socket(socket_path: impl Into<PathBuf>) -> Self {
        Self::UnixSocket(UnixSocketTransport::new(socket_path.into()))
    }

    pub fn tcp_auth_password_file(
        host: impl Into<String>,
        port: u16,
        password_file: impl Into<PathBuf>,
    ) -> Result<Self, LocalApiError> {
        Ok(Self::TcpAuth(TcpAuthTransport::new_with_password_file(
            host.into(),
            port,
            password_file.into(),
        )?))
    }

    pub async fn send(
        &self,
        request: TransportRequest,
    ) -> Result<Response<hyper::body::Incoming>, LocalApiError> {
        match self {
            LocalApiTransport::UnixSocket(transport) => transport.send(request).await,
            LocalApiTransport::TcpAuth(transport) => transport.send(request).await,
        }
    }
}

#[derive(Clone)]
pub struct UnixSocketTransport {
    socket_path: PathBuf,
    client: Client<UnixConnector, RequestBody>,
}

impl UnixSocketTransport {
    pub fn new(socket_path: PathBuf) -> Self {
        let client = Client::builder(TokioExecutor::new()).build(UnixConnector);
        Self {
            socket_path,
            client,
        }
    }

    async fn send(
        &self,
        request: TransportRequest,
    ) -> Result<Response<hyper::body::Incoming>, LocalApiError> {
        let uri: Uri = UnixUri::new(&self.socket_path, request.path.as_str()).into();
        let mut extra_headers = HeaderMap::new();
        extra_headers.insert(HOST, HeaderValue::from_static(LOCAL_API_HOST));
        let req = request.build_request(uri, extra_headers)?;
        tracing::debug!(method = %req.method(), path = %request.path, "LocalAPI request (unix)");
        let response = self.client.request(req).await?;
        Ok(response)
    }
}

#[derive(Clone)]
pub struct TcpAuthTransport {
    host: String,
    port: u16,
    password: Arc<Mutex<String>>,
    password_file: PathBuf,
    client: Client<HttpConnector, RequestBody>,
}

impl TcpAuthTransport {
    pub fn new_with_password_file(
        host: String,
        port: u16,
        password_file: PathBuf,
    ) -> Result<Self, LocalApiError> {
        let password = read_password_file(&password_file)?;
        let connector = HttpConnector::new();
        let client = Client::builder(TokioExecutor::new()).build(connector);
        Ok(Self {
            host,
            port,
            password: Arc::new(Mutex::new(password)),
            password_file,
            client,
        })
    }

    async fn send(
        &self,
        request: TransportRequest,
    ) -> Result<Response<hyper::body::Incoming>, LocalApiError> {
        let response = self.send_once(&request).await?;
        if response.status() == StatusCode::UNAUTHORIZED {
            tracing::debug!(path = %self.password_file.display(), "LocalAPI auth rejected, re-reading password file");
            let refreshed = read_password_file(&self.password_file)?;
            {
                let mut guard = self.lock_password()?;
                *guard = refreshed;
            }
            let retry = self.send_once(&request).await?;
            return Ok(retry);
        }
        Ok(response)
    }

    async fn send_once(
        &self,
        request: &TransportRequest,
    ) -> Result<Response<hyper::body::Incoming>, LocalApiError> {
        let uri: Uri = format!("http://{}:{}{}", self.host, self.port, request.path).parse()?;
        let mut extra_headers = HeaderMap::new();
        let auth_value = {
            let password = self.lock_password()?;
            build_basic_auth(&password)?
        };
        extra_headers.insert(AUTHORIZATION, auth_value);
        extra_headers.insert(
            HeaderName::from_static(SEC_TAILSCALE_HEADER),
            HeaderValue::from_static("localapi"),
        );
        let req = request.build_request(uri, extra_headers)?;
        tracing::debug!(method = %req.method(), path = %request.path, "LocalAPI request (tcp)");
        let response = self.client.request(req).await?;
        Ok(response)
    }

    fn lock_password(&self) -> Result<std::sync::MutexGuard<'_, String>, LocalApiError> {
        self.password
            .lock()
            .map_err(|_| std::io::Error::other("password lock poisoned").into())
    }
}

fn build_basic_auth(password: &str) -> Result<HeaderValue, LocalApiError> {
    let creds = format!(":{password}");
    let encoded = base64_engine.encode(creds.as_bytes());
    let header_value = format!("Basic {encoded}");
    HeaderValue::from_str(&header_value).map_err(|_| LocalApiError::InvalidHeaderValue {
        name: "authorization",
    })
}

fn read_password_file(path: &Path) -> Result<String, LocalApiError> {
    validate_password_permissions(path)?;
    let contents = std::fs::read_to_string(path).map_err(|source| LocalApiError::PasswordRead {
        path: path.to_path_buf(),
        source,
    })?;
    let password = contents.trim_end_matches(&['\r', '\n'][..]).to_string();
    if password.is_empty() {
        return Err(LocalApiError::EmptyPasswordFile {
            path: path.to_path_buf(),
        });
    }
    Ok(password)
}

#[cfg(unix)]
fn validate_password_permissions(path: &Path) -> Result<(), LocalApiError> {
    use std::os::unix::fs::PermissionsExt;

    let metadata = std::fs::metadata(path)?;
    let mode = metadata.permissions().mode() & 0o777;
    if mode != 0o600 {
        return Err(LocalApiError::PasswordPermissions {
            path: path.to_path_buf(),
            mode,
        });
    }
    Ok(())
}

#[cfg(not(unix))]
fn validate_password_permissions(_path: &Path) -> Result<(), LocalApiError> {
    Ok(())
}
