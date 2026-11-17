#[cfg(windows)]
mod windows_impl {
    use std::ptr::null_mut;

    use windows::{
        core::PCWSTR,
        Win32::{
            Foundation::{ERROR_NOT_FOUND, GetLastError},
            Security::Credentials::{CredDeleteW, CredFree, CredReadW, CREDENTIALW, CRED_TYPE_GENERIC},
        },
    };

    pub struct SyncCredentials {
        pub server_url: String,
        pub device_id: String,
        pub device_name: String,
        pub token: String,
    }

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn read(name: &str) -> Option<String> {
        let target = wide(&format!("rtools:{name}"));
        unsafe {
            let mut credential: *mut CREDENTIALW = null_mut();
            let result = CredReadW(
                PCWSTR(target.as_ptr()),
                CRED_TYPE_GENERIC,
                0,
                &mut credential,
            );
            if result.is_err() {
                if GetLastError() == ERROR_NOT_FOUND {
                    return None;
                }
                log::warn!("CredReadW({name}) failed: {:?}", result.err());
                return None;
            }
            if credential.is_null() {
                return None;
            }
            let value = (&*credential).CredentialBlob;
            let len = (&*credential).CredentialBlobSize as usize;
            let bytes = std::slice::from_raw_parts(value, len);
            CredFree(credential.cast());
            std::str::from_utf8(bytes).ok().map(str::to_owned)
        }
    }

    pub fn load_sync_credentials() -> Option<SyncCredentials> {
        Some(SyncCredentials {
            server_url: read("sync-server-url")?,
            device_id: read("sync-device-id")?,
            device_name: read("sync-device-name")?,
            token: read("sync-token")?,
        })
    }

    #[allow(dead_code)]
    pub fn delete(name: &str) {
        let target = wide(&format!("rtools:{name}"));
        unsafe {
            let _ = CredDeleteW(PCWSTR(target.as_ptr()), CRED_TYPE_GENERIC, 0);
        }
    }
}

#[cfg(windows)]
pub use windows_impl::{delete, load_sync_credentials};

#[cfg(not(windows))]
pub fn load_sync_credentials() -> Option<super::process::SyncCredentials> {
    None
}
