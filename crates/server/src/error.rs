use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use domain::PandaError;
use prost::Message;
use proto::ApiError;

/// Local newtype so we can implement axum's `IntoResponse` (orphan rules).
pub struct AppError(pub PandaError);

impl From<PandaError> for AppError {
    fn from(value: PandaError) -> Self {
        Self(value)
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let err = self.0;
        let status = StatusCode::from_u16(err.code.http_status())
            .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let body = ApiError {
            code: err.code.as_str().to_string(),
            message: err.message,
            retryable: err.code.retryable(),
            current_etag: err.current_etag,
            details_json: err.details_json,
        };
        let json = serde_json::to_vec(&body).unwrap_or_default();
        (
            status,
            [(
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/json"),
            )],
            json,
        )
            .into_response()
    }
}

pub fn proto_or_json<T: Message + serde::Serialize>(accept_proto: bool, msg: &T) -> Response {
    if accept_proto {
        let mut buf = Vec::with_capacity(msg.encoded_len());
        if msg.encode(&mut buf).is_ok() {
            return (
                StatusCode::OK,
                [(
                    header::CONTENT_TYPE,
                    HeaderValue::from_static("application/x-protobuf"),
                )],
                buf,
            )
                .into_response();
        }
    }
    let json = serde_json::to_vec(msg).unwrap_or_default();
    (
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        )],
        json,
    )
        .into_response()
}

pub fn wants_protobuf(headers: &axum::http::HeaderMap) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.contains("protobuf"))
        || headers
            .get(header::ACCEPT)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.contains("protobuf"))
}

pub async fn read_body_proto_or_json<T>(
    headers: &axum::http::HeaderMap,
    bytes: bytes::Bytes,
) -> Result<T, PandaError>
where
    T: Message + Default + serde::de::DeserializeOwned,
{
    if wants_protobuf(headers) || looks_like_proto(&bytes) {
        T::decode(bytes.as_ref()).map_err(|e| PandaError::invalid(format!("bad protobuf: {e}")))
    } else {
        serde_json::from_slice(&bytes).map_err(|e| PandaError::invalid(format!("bad json: {e}")))
    }
}

fn looks_like_proto(bytes: &[u8]) -> bool {
    !bytes.is_empty() && bytes[0] != b'{' && bytes[0] != b'['
}
