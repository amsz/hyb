use serde::Serialize;

/// 统一错误类型：Tauri command 直接返回给前端（原包错误文案见 exe 字符串表）
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("http: {0}")]
    Http(#[from] reqwest::Error),
    #[error("{0}")]
    Message(String),
}

impl AppError {
    pub fn msg(s: impl Into<String>) -> Self {
        AppError::Message(s.into())
    }
}

impl Serialize for AppError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;
