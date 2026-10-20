use std::io;

/// Erros do núcleo. O `code` é enviado para o Kotlin e traduzido para uma
/// mensagem localizada na interface.
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("operação cancelada pelo usuário")]
    Cancelled,

    #[error("arquivo PKG inválido ou corrompido: {0}")]
    InvalidPkg(String),

    #[error("o PKG usa PFS criptografado e as chaves não puderam ser derivadas: {0}")]
    EncryptedPkg(String),

    #[error("o PKG não contém uma imagem PFS reconhecível")]
    NoPfsImage,

    #[error("formato não suportado: {0}")]
    Unsupported(String),

    #[error("entrada inválida: {0}")]
    InvalidInput(String),

    #[error("espaço insuficiente no destino")]
    NoSpace,

    #[error("erro de E/S: {0}")]
    Io(#[from] io::Error),

    #[error("erro interno: {0}")]
    Internal(String),
}

impl CoreError {
    pub fn code(&self) -> &'static str {
        match self {
            CoreError::Cancelled => "CANCELLED",
            CoreError::InvalidPkg(_) => "INVALID_PKG",
            CoreError::EncryptedPkg(_) => "ENCRYPTED_PKG",
            CoreError::NoPfsImage => "NO_PFS",
            CoreError::Unsupported(_) => "UNSUPPORTED",
            CoreError::InvalidInput(_) => "INVALID_INPUT",
            CoreError::NoSpace => "NO_SPACE",
            CoreError::Io(e) => {
                if e.raw_os_error() == Some(28) || e.to_string().to_lowercase().contains("no space") {
                    "NO_SPACE"
                } else {
                    "IO"
                }
            }
            CoreError::Internal(_) => "INTERNAL",
        }
    }

    pub fn message(&self) -> String {
        self.to_string()
    }
}

pub type Result<T> = std::result::Result<T, CoreError>;

pub fn io_err(context: &str, e: io::Error) -> CoreError {
    if e.raw_os_error() == Some(28) || e.to_string().to_lowercase().contains("no space") {
        CoreError::NoSpace
    } else {
        CoreError::Io(io::Error::new(e.kind(), format!("{context}: {e}")))
    }
}
