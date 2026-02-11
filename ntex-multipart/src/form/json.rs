//! Deserializes a field as JSON.

use std::sync::Arc;

use super::FieldErrorHandler;
use crate::{
    Field, MultipartError,
    form::{FieldReader, Limits, bytes::Bytes},
};
use derive_more::{Deref, DerefMut, Display, Error};
use futures::future::LocalBoxFuture;
use ntex::http::{Response, ResponseError};
use ntex::web::HttpRequest;
use ntex_http::Error;
use serde::de::DeserializeOwned;

/// Deserialize from JSON.
#[derive(Debug, Deref, DerefMut)]
pub struct Json<T: DeserializeOwned>(pub T);

impl<T: DeserializeOwned> Json<T> {
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<'t, T> FieldReader<'t> for Json<T>
where
    T: DeserializeOwned + 'static,
{
    type Future = LocalBoxFuture<'t, Result<Self, MultipartError>>;

    fn read_field(req: &'t HttpRequest, field: Field, limits: &'t mut Limits) -> Self::Future {
        Box::pin(async move {
            let config = JsonConfig::from_req(req);

            if config.validate_content_type {
                let valid = if let Some(mime) = field.content_type() {
                    mime.subtype() == mime::JSON || mime.suffix() == Some(mime::JSON)
                } else {
                    false
                };

                if !valid {
                    return Err(MultipartError::Field {
                        name: field.form_field_name,
                        source: config.map_error(req, JsonFieldError::ContentType),
                    });
                }
            }

            let form_field_name = field.form_field_name.clone();

            let bytes = Bytes::read_field(req, field, limits).await?;

            Ok(Json(serde_json::from_slice(bytes.data.as_ref()).map_err(|err| {
                MultipartError::Field {
                    name: form_field_name,
                    source: config.map_error(req, JsonFieldError::Deserialize(err)),
                }
            })?))
        })
    }
}

#[derive(Debug, Display, Error)]
#[non_exhaustive]
pub enum JsonFieldError {
    /// Deserialize error.
    #[display("Json deserialize error: {}", _0)]
    Deserialize(serde_json::Error),

    /// Content type error.
    #[display("Content type error")]
    ContentType,
}

impl ResponseError for JsonFieldError {
    fn error_response(&self) -> Response {
        todo!()
    }
}

/// Configuration for the [`Json`] field reader.
#[derive(Clone)]
pub struct JsonConfig {
    err_handler: FieldErrorHandler<JsonFieldError>,
    validate_content_type: bool,
}

const DEFAULT_CONFIG: JsonConfig =
    JsonConfig { err_handler: None, validate_content_type: true };

impl JsonConfig {
    pub fn error_handler<F>(mut self, f: F) -> Self
    where
        F: Fn(JsonFieldError, &HttpRequest) -> Error + Send + Sync + 'static,
    {
        self.err_handler = Some(Arc::new(f));
        self
    }

    /// Extract payload config from app data. Check both `T` and `Data<T>`, in that order, and fall
    /// back to the default payload config.
    fn from_req(req: &HttpRequest) -> &Self {
        req.app_State::<Self>()
            .or_else(|| req.app_state::<ntex::web::types::State<Self>>().map(|d| d.as_ref()))
            .unwrap_or(&DEFAULT_CONFIG)
    }

    fn map_error(&self, req: &HttpRequest, err: JsonFieldError) -> Error {
        if let Some(err_handler) = self.err_handler.as_ref() {
            (*err_handler)(err, req)
        } else {
            err.into()
        }
    }

    /// Sets whether or not the field must have a valid `Content-Type` header to be parsed.
    pub fn validate_content_type(mut self, validate_content_type: bool) -> Self {
        self.validate_content_type = validate_content_type;
        self
    }
}

impl Default for JsonConfig {
    fn default() -> Self {
        DEFAULT_CONFIG
    }
}
