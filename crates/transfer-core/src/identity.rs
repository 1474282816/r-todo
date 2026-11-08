//! 本机身份：自签证书 + 私钥 + fingerprint（大写 hex）。
//!
//! 首次启动生成一次并持久化；后续启动直接复用，对端看到的 fingerprint 保持稳定。

use std::path::Path;

use crate::{ErrorCode, ErrorScope, TransferError};

const CERT_FILE: &str = "transfer-cert.pem";
const KEY_FILE: &str = "transfer-key.pem";
const FINGERPRINT_FILE: &str = "transfer-fingerprint.txt";

#[derive(Debug, Clone)]
pub struct Identity {
    pub cert_pem: String,
    pub private_key_pem: String,
    /// 证书 DER 的 SHA-256，大写 hex（LocalSend 约定）。
    pub fingerprint: String,
}

impl Identity {
    /// 从目录加载，缺失或不完整时重新生成。
    pub fn load_or_create(dir: &Path) -> Result<Self, TransferError> {
        let cert_path = dir.join(CERT_FILE);
        let key_path = dir.join(KEY_FILE);
        let fp_path = dir.join(FINGERPRINT_FILE);

        if let (Ok(cert_pem), Ok(private_key_pem), Ok(fingerprint)) = (
            std::fs::read_to_string(&cert_path),
            std::fs::read_to_string(&key_path),
            std::fs::read_to_string(&fp_path),
        ) {
            let fingerprint = fingerprint.trim().to_string();
            if !cert_pem.trim().is_empty() && !private_key_pem.trim().is_empty() && !fingerprint.is_empty() {
                return Ok(Self {
                    cert_pem,
                    private_key_pem,
                    fingerprint,
                });
            }
        }

        let generated = localsend::crypto::cert::generate_self_signed().map_err(|e| TransferError::Protocol {
            scope: ErrorScope::Server,
            code: ErrorCode::Tls,
            message: format!("生成自签证书失败: {e}"),
        })?;

        std::fs::create_dir_all(dir)?;
        std::fs::write(&cert_path, &generated.certificate_pem)?;
        std::fs::write(&key_path, &generated.private_key_pem)?;
        std::fs::write(&fp_path, &generated.fingerprint)?;

        Ok(Self {
            cert_pem: generated.certificate_pem,
            private_key_pem: generated.private_key_pem,
            fingerprint: generated.fingerprint,
        })
    }

    /// 会话内临时身份（不落盘），用于无法写盘的极端场景。
    pub fn ephemeral() -> Result<Self, TransferError> {
        let generated = localsend::crypto::cert::generate_self_signed().map_err(|e| TransferError::Protocol {
            scope: ErrorScope::Server,
            code: ErrorCode::Tls,
            message: format!("生成自签证书失败: {e}"),
        })?;
        Ok(Self {
            cert_pem: generated.certificate_pem,
            private_key_pem: generated.private_key_pem,
            fingerprint: generated.fingerprint,
        })
    }
}
