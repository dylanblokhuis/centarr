use axum::{
    body,
    http::StatusCode,
    response::{IntoResponse, Response},
};

#[derive(Debug)]
pub struct ApiError {
    status_code: StatusCode,
    message: Option<String>,
}

impl ApiError {
    pub fn new(status_code: u16, message: String) -> Self {
        Self {
            status_code: StatusCode::from_u16(status_code)
                .expect("Status Code used that doesn't exist"),
            message: Some(message),
        }
    }

    pub fn empty(status_code: u16, log: Option<String>) -> Self {
        println!("{:?}", log);
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
