use crate::compat::value::{JsString, Value};
use crate::{files::images::Image, http::api::Body};
use base64::{Engine, engine::general_purpose::STANDARD};

pub struct Input<'a> {
    pub connection: &'a Value,
    pub request: &'a Value,
    pub model: &'a JsString,
    pub operation: &'a str,
    pub references: &'a [Image],
    pub mask: Option<&'a Image>,
}

pub struct Prepared {
    pub operation: String,
    pub prefix: &'static str,
    pub body: Body,
}
impl Prepared {
    pub fn json(operation: &str, prefix: &'static str, value: Value) -> Self {
        Self {
            operation: operation.into(),
            prefix,
            body: Body::Json(value),
        }
    }
}

#[derive(Default)]
pub struct ResultItem {
    pub base64: Option<JsString>,
    pub url: Option<JsString>,
    pub bytes: Option<Vec<u8>>,
    pub error: Option<String>,
}

#[derive(Clone)]
pub struct Job {
    pub id: JsString,
    pub kind: String,
}

#[derive(Default)]
pub struct Normalized {
    pub images: Vec<ResultItem>,
    pub terminal: Option<String>,
    pub upstream_model: Option<JsString>,
    pub upstream_id: Option<JsString>,
    pub usage: Option<Value>,
    pub job: Option<Job>,
    pub continuation: Option<Value>,
    pub warnings: Vec<JsString>,
}

pub fn mime_data(image: &Image) -> String {
    format!(
        "data:{};base64,{}",
        image.mime,
        STANDARD.encode(&image.bytes)
    )
}
