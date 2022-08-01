use axum::{
    body,
    http::StatusCode,
    response::{IntoResponse, Response},
};

pub struct ApiError {
    status_code: StatusCode,
    message: Option<&'static str>,
}

impl ApiError {
    pub fn new(status_code: u16, message: &'static str) -> Self {
        Self {
            status_code: StatusCode::from_u16(status_code)
                .expect("Status Code used that doesn't exist"),
            message: Some(message),
        }
    }

    pub fn empty(status_code: u16, error: Option<String>) -> Self {
        println!("{:?}", error);
        Self {
            status_code: StatusCode::from_u16(status_code)
                .expect("Status Code used that doesn't exist"),
            message: None,
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        if self.message.is_some() {
            Response::builder()
                .status(self.status_code)
                .body(body::boxed(body::Full::from(self.message.unwrap())))
                .unwrap()
        } else {
            Response::builder()
                .status(self.status_code)
                .body(body::boxed(body::Empty::new()))
                .unwrap()
        }
    }
}
