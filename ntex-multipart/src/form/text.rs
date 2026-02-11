//! Deserializes a field from plain text.

use super::FieldErrorHandler;
use crate::{
    Field, MultipartError,
    form::{FieldReader, Limits, bytes::Bytes},
};
use derive_more::{Deref, DerefMut, Display, Error};
use futures::future::LocalBoxFuture;
use ntex::http::{Response, ResponseError};
use ntex::web::HttpRequest;
use serde::de::DeserializeOwned;
use std::{str, sync::Arc};

/// Deserialize from plain text.
///
/// Internally this uses [`serde_plain`] for deserialization, which supports primitive types
/// including strings, numbers, and simple enums.
#[derive(Debug, Deref, DerefMut)]
pub struct Text<T: DeserializeOwned>(pub T);

impl<T: DeserializeOwned> Text<T> {
    /// Unwraps into inner value.
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<'t, T> FieldReader<'t> for Text<T>
where
    T: DeserializeOwned + 'static,
{
    type Future = LocalBoxFuture<'t, Result<Self, MultipartError>>;

    fn read_field(req: &'t HttpRequest, field: Field, limits: &'t mut Limits) -> Self::Future {
        Box::pin(async move {
            let config = TextConfig::from_req(req);

            if config.validate_content_type {
                let valid = if let Some(mime) = field.content_type() {
                    mime.subtype() == mime::PLAIN || mime.suffix() == Some(mime::PLAIN)
                } else {
                    // https://datatracker.ietf.org/doc/html/rfc7578#section-4.4
                    // content type defaults to text/plain, so None should be considered valid
                    true
                };

                if !valid {
                    return Err(MultipartError::Field {
                        name: field.form_field_name,
                        source: config.map_error(req, TextError::ContentType),
                    });
                }
            }

            let form_field_name = field.form_field_name.clone();

            let bytes = Bytes::read_field(req, field, limits).await?;

            let text = str::from_utf8(&bytes.data).map_err(|err| MultipartError::Field {
                name: form_field_name.clone(),
                source: config.map_error(req, TextError::Utf8Error(err)),
            })?;

            Ok(Text(serde_plain::from_str(text).map_err(|err| MultipartError::Field {
                name: form_field_name,
                source: config.map_error(req, TextError::Deserialize(err)),
            })?))
        })
    }
}

#[derive(Debug, Display, Error)]
#[non_exhaustive]
pub enum TextError {
    /// UTF-8 decoding error.
    #[display("UTF-8 decoding error: {}", _0)]
    Utf8Error(str::Utf8Error),

    /// Deserialize error.
    #[display("Plain text deserialize error: {}", _0)]
    Deserialize(serde_plain::Error),

    /// Content type error.
    #[display("Content type error")]
    ContentType,
}

impl ResponseError for TextError {
    fn error_response(&self) -> Response {
        todo!()
    }
}

/// Configuration for the [`Text`] field reader.
#[derive(Clone)]
pub struct TextConfig {
    err_handler: FieldErrorHandler<TextError>,
    validate_content_type: bool,
}

impl TextConfig {
    /// Sets custom error handler.
    pub fn error_handler<F>(mut self, f: F) -> Self
    where
        F: Fn(TextError, &HttpRequest) -> ntex::web::Error + Send + Sync + 'static,
    {
        self.err_handler = Some(Arc::new(f));
        self
    }

    /// Extracts payload config from app data. Check both `T` and `Data<T>`, in that order, and fall
    /// back to the default payload config.
    fn from_req(req: &HttpRequest) -> &Self {
        req.app_state::<Self>()
            .or_else(|| req.app_state::<ntex::web::types::State<Self>>().map(|d| d.as_ref()))
            .unwrap_or(&DEFAULT_CONFIG)
    }

    fn map_error(&self, req: &HttpRequest, err: TextError) -> ntex::web::Error {
        if let Some(ref err_handler) = self.err_handler {
            (err_handler)(err, req)
        } else {
            err.into()
        }
    }

    /// Sets whether or not the field must have a valid `Content-Type` header to be parsed.
    ///
    /// Note that an empty `Content-Type` is also accepted, as the multipart specification defines
    /// `text/plain` as the default for text fields.
    pub fn validate_content_type(mut self, validate_content_type: bool) -> Self {
        self.validate_content_type = validate_content_type;
        self
    }
}

const DEFAULT_CONFIG: TextConfig =
    TextConfig { err_handler: None, validate_content_type: true };

impl Default for TextConfig {
    fn default() -> Self {
        DEFAULT_CONFIG
    }
}
