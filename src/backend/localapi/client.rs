use std::io;

use futures::{StreamExt, TryStreamExt};
use http_body_util::BodyExt;
use hyper::header::{HeaderValue, CONTENT_TYPE, ETAG, IF_MATCH};
use hyper::{Method, Response, StatusCode};
use serde_json::Value;
use tokio::task::JoinHandle;
use tokio_util::codec::{FramedRead, LinesCodec};
use tokio_util::io::StreamReader;

use crate::net::{LocalApiError, LocalApiTransport, TransportRequest};

const WATCH_MASK_INITIAL_STATE: u64 = 1 << 1;
const MAX_WATCH_LINE: usize = 1024 * 1024;
const JSON_CONTENT_TYPE: &str = "application/json";
const STATUS_ENDPOINT: &str = "/localapi/v0/status";
const WATCH_IPN_BUS_ENDPOINT: &str = "/localapi/v0/watch-ipn-bus";
const SERVE_CONFIG_ENDPOINT: &str = "/localapi/v0/serve-config";

pub struct LocalApiClient {
    transport: LocalApiTransport,
}

pub struct ServeConfigResponse {
    pub etag: Option<String>,
    pub config: Value,
}

pub struct WatchIpnBus {
    session_id: String,
    drain_task: Option<JoinHandle<()>>,
}

impl WatchIpnBus {
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn close(&mut self) {
        if let Some(task) = self.drain_task.take() {
            task.abort();
        }
    }
}

impl Drop for WatchIpnBus {
    fn drop(&mut self) {
        self.close();
    }
}

impl LocalApiClient {
    pub fn new(transport: LocalApiTransport) -> Self {
        Self { transport }
    }

    pub async fn get_status(&self) -> Result<Value, LocalApiError> {
        let request = TransportRequest::new(Method::GET, STATUS_ENDPOINT);
        let response = self.send_ok(request).await?;
        parse_json_response(response).await
    }

    pub async fn get_serve_config(&self) -> Result<ServeConfigResponse, LocalApiError> {
        let request = TransportRequest::new(Method::GET, SERVE_CONFIG_ENDPOINT);
        let response = self.send_ok(request).await?;
        let etag = header_to_string(response.headers(), ETAG)?;
        let config = parse_json_response(response).await?;
        Ok(ServeConfigResponse { etag, config })
    }

    pub async fn set_serve_config(
        &self,
        config: &Value,
        etag: Option<&str>,
    ) -> Result<(), LocalApiError> {
        let body = serde_json::to_vec(config)?;
        let mut request =
            TransportRequest::new(Method::POST, SERVE_CONFIG_ENDPOINT).with_body(body);
        request
            .headers
            .insert(CONTENT_TYPE, HeaderValue::from_static(JSON_CONTENT_TYPE));
        if let Some(etag_value) = etag {
            let header_value = HeaderValue::from_str(etag_value)
                .map_err(|_| LocalApiError::InvalidHeaderValue { name: "if-match" })?;
            request.headers.insert(IF_MATCH, header_value);
        }
        self.send_ok(request).await?;
        Ok(())
    }

    pub async fn watch_ipn_bus(&self) -> Result<WatchIpnBus, LocalApiError> {
        let path = format!("{WATCH_IPN_BUS_ENDPOINT}?mask={}", WATCH_MASK_INITIAL_STATE);
        let request = TransportRequest::new(Method::GET, path);
        let response = self.send_ok(request).await?;

        let stream = response
            .into_body()
            .into_data_stream()
            .map_err(io::Error::other);
        let reader = StreamReader::new(stream);
        let mut lines = FramedRead::new(reader, LinesCodec::new_with_max_length(MAX_WATCH_LINE));

        let mut session_id = None;
        while let Some(result) = lines.next().await {
            let line = match result {
                Ok(line) => line,
                Err(err) => {
                    return Err(io::Error::other(err).into());
                }
            };
            if line.trim().is_empty() {
                continue;
            }
            if let Some(id) = parse_session_id(&line)? {
                session_id = Some(id);
                break;
            }
        }

        let session_id = session_id.ok_or(LocalApiError::MissingSessionId)?;

        let drain_task = tokio::spawn(async move {
            while let Some(result) = lines.next().await {
                if let Err(err) = result {
                    tracing::debug!(error = %err, "watch-ipn-bus stream ended with error");
                    break;
                }
            }
        });

        Ok(WatchIpnBus {
            session_id,
            drain_task: Some(drain_task),
        })
    }

    async fn send_ok(
        &self,
        request: TransportRequest,
    ) -> Result<Response<hyper::body::Incoming>, LocalApiError> {
        let method = request.method.clone();
        let path = request.path.clone();
        let response = self.transport.send(request).await?;
        ensure_status_ok(response, method, path).await
    }
}

async fn ensure_status_ok(
    response: Response<hyper::body::Incoming>,
    method: Method,
    path: String,
) -> Result<Response<hyper::body::Incoming>, LocalApiError> {
    if response.status() == StatusCode::OK {
        return Ok(response);
    }
    let status = response.status();
    let body = read_body_string(response).await?;
    Err(LocalApiError::HttpStatus {
        status,
        method,
        path,
        body,
    })
}

async fn parse_json_response(
    response: Response<hyper::body::Incoming>,
) -> Result<Value, LocalApiError> {
    let bytes = response.into_body().collect().await?.to_bytes();
    if bytes.is_empty() {
        return Ok(Value::Null);
    }
    let value = serde_json::from_slice(bytes.as_ref())?;
    Ok(value)
}

async fn read_body_string(
    response: Response<hyper::body::Incoming>,
) -> Result<String, LocalApiError> {
    let bytes = response.into_body().collect().await?.to_bytes();
    if bytes.is_empty() {
        return Ok("<empty>".to_string());
    }
    Ok(String::from_utf8_lossy(bytes.as_ref()).to_string())
}

fn parse_session_id(line: &str) -> Result<Option<String>, LocalApiError> {
    let value: Value = serde_json::from_str(line)?;
    let session_id = value
        .get("SessionID")
        .and_then(Value::as_str)
        .or_else(|| value.get("session_id").and_then(Value::as_str))
        .or_else(|| value.get("sessionId").and_then(Value::as_str))
        .filter(|id| !id.is_empty());
    Ok(session_id.map(str::to_string))
}

fn header_to_string(
    headers: &hyper::HeaderMap,
    name: hyper::header::HeaderName,
) -> Result<Option<String>, LocalApiError> {
    match headers.get(name) {
        Some(value) => value
            .to_str()
            .map(|s| Some(s.to_string()))
            .map_err(|_| LocalApiError::InvalidHeaderValue { name: "etag" }),
        None => Ok(None),
    }
}
