//! Antigravity 登录凭据的读取 / 备份 / 恢复。
//!
//! Antigravity 把 Google OAuth 令牌写入 Windows 凭据管理器：
//! `LegacyGeneric:target=gemini:antigravity`（Generic，LocalMachine 持久化）。
//! 该 blob 是一段 JSON：`token.access_token` / `token.refresh_token` /
//! `id_token_email` / `auth_method` / `writer_id`。
//!
//! 因此「切换账号」不需要重新登录，只要把某个账号的 blob 原样写回凭据管理器，
//! 再重启 Antigravity 即可（refresh_token 会让 Antigravity 自动换新 access_token）。
//!
//! 本地备份文件使用 DPAPI（CryptProtectData，当前用户作用域）加密，避免明文令牌落盘。

use serde::Serialize;

/// Antigravity 在 Windows 凭据管理器中的目标名。
pub const TARGET: &str = "gemini:antigravity";
/// 凭据管理器里的用户名（固定为 antigravity）。
pub const TARGET_USER: &str = "antigravity";

/// 从凭据 blob 中解析出的公开信息（不含任何令牌内容）。
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialInfo {
    pub email: Option<String>,
    pub auth_method: Option<String>,
    pub has_access_token: bool,
    pub has_refresh_token: bool,
    /// `token.expiry` 解析出的毫秒时间戳（RFC3339，可能带时区偏移）。
    pub expires_at_ms: Option<i64>,
    pub blob_len: usize,
}

impl CredentialInfo {
    pub fn is_expired(&self, now_ms: i64) -> bool {
        matches!(self.expires_at_ms, Some(exp) if exp <= now_ms)
    }
}

/// 对凭据原始字节计算 FNV-1a 64-bit 指纹（不含明文，仅用于检测令牌内容是否发生变化）。
pub fn blob_fingerprint(blob: &[u8]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in blob {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

fn decode_base64url(input: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(input.len() * 3 / 4);
    let mut buf: u32 = 0;
    let mut bits: u8 = 0;
    for b in input.bytes() {
        let val = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'-' | b'+' => 62,
            b'_' | b'/' => 63,
            b'=' | b'\r' | b'\n' | b' ' => continue,
            _ => return None,
        };
        buf = (buf << 6) | (val as u32);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((buf >> bits) & 0xff) as u8);
        }
    }
    Some(out)
}

fn extract_email_from_jwt(jwt: &str) -> Option<String> {
    let mut parts = jwt.trim().split('.');
    let _header = parts.next()?;
    let payload_b64 = parts.next()?;
    let bytes = decode_base64url(payload_b64)?;
    let val: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    val.get("email")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn parse_expiry_value(val: &serde_json::Value) -> Option<i64> {
    if let Some(s) = val.as_str() {
        let trimmed = s.trim();
        if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(trimmed) {
            return Some(dt.timestamp_millis());
        }
        if let Ok(num) = trimmed.parse::<i64>() {
            return Some(if num.abs() < 100_000_000_000 {
                num * 1000
            } else {
                num
            });
        }
    } else if let Some(num) = val.as_i64() {
        return Some(if num.abs() < 100_000_000_000 {
            num * 1000
        } else {
            num
        });
    } else if let Some(num) = val.as_f64() {
        let n = num as i64;
        return Some(if n.abs() < 100_000_000_000 {
            n * 1000
        } else {
            n
        });
    }
    None
}

/// 解析凭据 blob。无法解析时返回空信息（保留 blob 原样，不影响写回）。
pub fn parse_blob(blob: &[u8]) -> CredentialInfo {
    let mut info = CredentialInfo {
        blob_len: blob.len(),
        ..Default::default()
    };
    let text = String::from_utf8_lossy(blob);
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
        return info;
    };

    let token = value.get("token").cloned().unwrap_or(serde_json::Value::Null);

    let non_empty_str = |v: Option<&serde_json::Value>| -> Option<String> {
        v.and_then(|x| x.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    };

    let email = non_empty_str(value.get("id_token_email"))
        .or_else(|| non_empty_str(value.get("email")))
        .or_else(|| non_empty_str(token.get("email")))
        .or_else(|| {
            non_empty_str(value.get("id_token"))
                .or_else(|| non_empty_str(token.get("id_token")))
                .and_then(|jwt| extract_email_from_jwt(&jwt))
        });
    info.email = email;

    info.auth_method = non_empty_str(value.get("auth_method"));

    info.has_access_token = token
        .get("access_token")
        .and_then(|v| v.as_str())
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);
    info.has_refresh_token = token
        .get("refresh_token")
        .and_then(|v| v.as_str())
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);

    info.expires_at_ms = token
        .get("expiry")
        .and_then(parse_expiry_value)
        .or_else(|| token.get("expires_at").and_then(parse_expiry_value))
        .or_else(|| token.get("expiry_date").and_then(parse_expiry_value));

    info
}

/// 内部使用的凭据令牌结构：绝不派生 Serialize，Debug 输出对令牌脱敏。
#[derive(Clone, Default)]
pub(crate) struct CredentialTokens {
    pub email: Option<String>,
    pub access_token: Option<String>,
    pub refresh_token: Option<String>,
    pub expires_at_ms: Option<i64>,
}

impl std::fmt::Debug for CredentialTokens {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CredentialTokens")
            .field("email", &self.email)
            .field("access_token", &self.access_token.as_ref().map(|_| "[REDACTED]"))
            .field("refresh_token", &self.refresh_token.as_ref().map(|_| "[REDACTED]"))
            .field("expires_at_ms", &self.expires_at_ms)
            .finish()
    }
}

/// 从凭据 blob 中提取 OAuth 令牌（仅供后端发起额度查询使用）。
pub(crate) fn extract_tokens(blob: &[u8]) -> CredentialTokens {
    let info = parse_blob(blob);
    let mut out = CredentialTokens {
        email: info.email,
        access_token: None,
        refresh_token: None,
        expires_at_ms: info.expires_at_ms,
    };
    let text = String::from_utf8_lossy(blob);
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
        return out;
    };

    let non_empty_str = |v: Option<&serde_json::Value>| -> Option<String> {
        v.and_then(|x| x.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    };

    let token = value.get("token");
    out.access_token = token
        .and_then(|t| non_empty_str(t.get("access_token")))
        .or_else(|| non_empty_str(value.get("access_token")));
    out.refresh_token = token
        .and_then(|t| non_empty_str(t.get("refresh_token")))
        .or_else(|| non_empty_str(value.get("refresh_token")));

    out
}

/// 读取当前登录凭据的原始 blob；未登录时返回 `Ok(None)`。
pub fn read_blob() -> Result<Option<Vec<u8>>, String> {
    sys::read_blob(TARGET)
}

/// 把备份的 blob 原样写回凭据管理器（覆盖当前登录账号）。
pub fn write_blob(blob: &[u8]) -> Result<(), String> {
    if blob.is_empty() {
        return Err("凭据备份为空，无法恢复".to_string());
    }
    sys::write_blob(TARGET, TARGET_USER, blob)
}

/// 清除当前登录凭据；不存在时返回 `Ok(false)`。
pub fn delete() -> Result<bool, String> {
    sys::delete(TARGET)
}

/// DPAPI 加密（当前 Windows 用户作用域）。
pub fn protect(data: &[u8]) -> Result<Vec<u8>, String> {
    sys::protect(data)
}

/// DPAPI 解密。
pub fn unprotect(data: &[u8]) -> Result<Vec<u8>, String> {
    sys::unprotect(data)
}

#[cfg(windows)]
mod sys {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr;
    use windows_sys::Win32::Foundation::{GetLastError, LocalFree, FILETIME};
    use windows_sys::Win32::Security::Credentials::{
        CredDeleteW, CredFree, CredReadW, CredWriteW, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE,
        CRED_TYPE_GENERIC,
    };
    use windows_sys::Win32::Security::Cryptography::{
        CryptProtectData, CryptUnprotectData, CRYPT_INTEGER_BLOB,
    };

    const ERROR_NOT_FOUND: u32 = 1168;

    fn wide(value: &str) -> Vec<u16> {
        OsStr::new(value).encode_wide().chain(Some(0)).collect()
    }

    pub fn read_blob(target: &str) -> Result<Option<Vec<u8>>, String> {
        let target_w = wide(target);
        let mut handle: *mut CREDENTIALW = ptr::null_mut();
        let ok = unsafe { CredReadW(target_w.as_ptr(), CRED_TYPE_GENERIC, 0, &mut handle) };
        if ok == 0 {
            let code = unsafe { GetLastError() };
            return if code == ERROR_NOT_FOUND {
                Ok(None)
            } else {
                Err(format!("读取 Windows 凭据失败 (Win32 {code})"))
            };
        }
        if handle.is_null() {
            return Ok(None);
        }

        let blob = unsafe {
            let cred = &*handle;
            let bytes = if cred.CredentialBlob.is_null() || cred.CredentialBlobSize == 0 {
                Vec::new()
            } else {
                std::slice::from_raw_parts(
                    cred.CredentialBlob as *const u8,
                    cred.CredentialBlobSize as usize,
                )
                .to_vec()
            };
            CredFree(handle as *const core::ffi::c_void);
            bytes
        };

        Ok(Some(blob))
    }

    pub fn write_blob(target: &str, user: &str, blob: &[u8]) -> Result<(), String> {
        let mut target_w = wide(target);
        let mut user_w = wide(user);
        let mut blob_copy = blob.to_vec();

        let cred = CREDENTIALW {
            Flags: 0,
            Type: CRED_TYPE_GENERIC,
            TargetName: target_w.as_mut_ptr(),
            Comment: ptr::null_mut(),
            LastWritten: FILETIME {
                dwLowDateTime: 0,
                dwHighDateTime: 0,
            },
            CredentialBlobSize: blob_copy.len() as u32,
            CredentialBlob: blob_copy.as_mut_ptr(),
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            AttributeCount: 0,
            Attributes: ptr::null_mut(),
            TargetAlias: ptr::null_mut(),
            UserName: user_w.as_mut_ptr(),
        };

        let ok = unsafe { CredWriteW(&cred, 0) };
        if ok == 0 {
            let code = unsafe { GetLastError() };
            return Err(format!("写入 Windows 凭据失败 (Win32 {code})"));
        }
        Ok(())
    }

    pub fn delete(target: &str) -> Result<bool, String> {
        let target_w = wide(target);
        let ok = unsafe { CredDeleteW(target_w.as_ptr(), CRED_TYPE_GENERIC, 0) };
        if ok == 0 {
            let code = unsafe { GetLastError() };
            return if code == ERROR_NOT_FOUND {
                Ok(false)
            } else {
                Err(format!("删除 Windows 凭据失败 (Win32 {code})"))
            };
        }
        Ok(true)
    }

    pub fn protect(data: &[u8]) -> Result<Vec<u8>, String> {
        let input = CRYPT_INTEGER_BLOB {
            cbData: data.len() as u32,
            pbData: data.as_ptr() as *mut u8,
        };
        let mut output = CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: ptr::null_mut(),
        };
        let ok = unsafe {
            CryptProtectData(
                &input,
                ptr::null(),
                ptr::null(),
                ptr::null(),
                ptr::null(),
                0,
                &mut output,
            )
        };
        if ok == 0 {
            let code = unsafe { GetLastError() };
            return Err(format!("DPAPI 加密失败 (Win32 {code})"));
        }
        let encrypted = unsafe {
            let bytes =
                std::slice::from_raw_parts(output.pbData as *const u8, output.cbData as usize)
                    .to_vec();
            LocalFree(output.pbData as *mut core::ffi::c_void);
            bytes
        };
        Ok(encrypted)
    }

    pub fn unprotect(data: &[u8]) -> Result<Vec<u8>, String> {
        let input = CRYPT_INTEGER_BLOB {
            cbData: data.len() as u32,
            pbData: data.as_ptr() as *mut u8,
        };
        let mut output = CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: ptr::null_mut(),
        };
        let ok = unsafe {
            CryptUnprotectData(
                &input,
                ptr::null_mut(),
                ptr::null(),
                ptr::null(),
                ptr::null(),
                0,
                &mut output,
            )
        };
        if ok == 0 {
            let code = unsafe { GetLastError() };
            return Err(format!("DPAPI 解密失败 (Win32 {code})"));
        }
        let plain = unsafe {
            let bytes =
                std::slice::from_raw_parts(output.pbData as *const u8, output.cbData as usize)
                    .to_vec();
            LocalFree(output.pbData as *mut core::ffi::c_void);
            bytes
        };
        Ok(plain)
    }
}

#[cfg(not(windows))]
mod sys {
    pub fn read_blob(_target: &str) -> Result<Option<Vec<u8>>, String> {
        Err("账号切换功能仅支持 Windows".to_string())
    }

    pub fn write_blob(_target: &str, _user: &str, _blob: &[u8]) -> Result<(), String> {
        Err("账号切换功能仅支持 Windows".to_string())
    }

    pub fn delete(_target: &str) -> Result<bool, String> {
        Err("账号切换功能仅支持 Windows".to_string())
    }

    pub fn protect(_data: &[u8]) -> Result<Vec<u8>, String> {
        Err("账号切换功能仅支持 Windows".to_string())
    }

    pub fn unprotect(_data: &[u8]) -> Result<Vec<u8>, String> {
        Err("账号切换功能仅支持 Windows".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_blob(email: &str, refresh: bool) -> Vec<u8> {
        let refresh_token = if refresh { "1//0abcdef" } else { "" };
        format!(
            r#"{{"token":{{"access_token":"ya29.token","token_type":"Bearer","refresh_token":"{refresh_token}","expiry":"2026-11-20T11:33:36.4912548+08:00","expires_in":3599}},"auth_method":"consumer","id_token_email":"{email}","writer_id":"writer-1"}}"#
        )
        .into_bytes()
    }

    #[test]
    fn parses_email_tokens_and_expiry() {
        let info = parse_blob(&sample_blob("demo@example.com", true));
        assert_eq!(info.email.as_deref(), Some("demo@example.com"));
        assert_eq!(info.auth_method.as_deref(), Some("consumer"));
        assert!(info.has_access_token);
        assert!(info.has_refresh_token);
        assert_eq!(info.expires_at_ms, Some(1795145616491));
        assert!(info.is_expired(1795145616492));
        assert!(!info.is_expired(1795145616490));
    }

    #[test]
    fn parses_blob_without_refresh_token() {
        let info = parse_blob(&sample_blob("demo@example.com", false));
        assert!(!info.has_refresh_token);
        assert!(info.has_access_token);
    }

    #[test]
    fn tolerates_garbage_and_empty_blobs() {
        let info = parse_blob(b"not-json");
        assert!(info.email.is_none());
        assert!(!info.has_refresh_token);
        assert_eq!(info.blob_len, 8);

        let empty = parse_blob(b"");
        assert_eq!(empty.blob_len, 0);
    }

    #[test]
    fn parses_fallback_jwt_email_and_numeric_expiry() {
        // payload: {"email":"jwt.user@example.com"} -> eyJlbWFpbCI6Imp3dC51c2VyQGV4YW1wbGUuY29tIn0
        let jwt = "eyJhbGciOiJSUzI1NiJ9.eyJlbWFpbCI6Imp3dC51c2VyQGV4YW1wbGUuY29tIn0.sig";
        let raw = format!(
            r#"{{"token":{{"access_token":"ya29.x","refresh_token":"1//r","expires_at":1795145616,"id_token":"{jwt}"}}}}"#
        );
        let info = parse_blob(raw.as_bytes());
        assert_eq!(info.email.as_deref(), Some("jwt.user@example.com"));
        assert_eq!(info.expires_at_ms, Some(1795145616000));
        assert!(info.has_refresh_token);
        assert_ne!(blob_fingerprint(raw.as_bytes()), blob_fingerprint(b"other"));
    }

    #[cfg(windows)]
    #[test]
    fn credential_roundtrip_and_dpapi_roundtrip() {
        let pid = std::process::id();
        let target = format!("gemini:antigravity.test.{pid}");
        let payload = sample_blob("probe@example.com", true);

        write_blob_for_test(&target, &payload).expect("write test credential");
        let got = read_blob_for_test(&target).expect("read test credential").expect("exists");
        assert_eq!(got, payload);
        delete_for_test(&target).expect("delete test credential");
        assert!(read_blob_for_test(&target).expect("read after delete").is_none());

        let encrypted = protect(b"secret-blob").expect("protect");
        assert_ne!(encrypted, b"secret-blob".to_vec());
        assert_eq!(unprotect(&encrypted).expect("unprotect"), b"secret-blob".to_vec());
    }

    #[cfg(windows)]
    fn write_blob_for_test(target: &str, blob: &[u8]) -> Result<(), String> {
        super::sys::write_blob(target, "probe", blob)
    }

    #[cfg(windows)]
    fn read_blob_for_test(target: &str) -> Result<Option<Vec<u8>>, String> {
        super::sys::read_blob(target)
    }

    #[cfg(windows)]
    fn delete_for_test(target: &str) -> Result<bool, String> {
        super::sys::delete(target)
    }

    #[test]
    fn extract_tokens_parses_nested_and_top_level_and_redacts_debug() {
        let nested = br#"{"token":{"access_token":"  ya29.secret-abc  ","refresh_token":"1//refresh-xyz","expiry":"2026-11-20T11:33:36+08:00"},"id_token_email":"user@example.com"}"#;
        let t1 = extract_tokens(nested);
        assert_eq!(t1.email.as_deref(), Some("user@example.com"));
        assert_eq!(t1.access_token.as_deref(), Some("ya29.secret-abc"));
        assert_eq!(t1.refresh_token.as_deref(), Some("1//refresh-xyz"));
        assert_eq!(t1.expires_at_ms, Some(1795145616000));
        let dbg = format!("{t1:?}");
        assert!(!dbg.contains("ya29.secret-abc"));
        assert!(!dbg.contains("1//refresh-xyz"));
        assert!(dbg.contains("[REDACTED]"));

        let top = br#"{"access_token":"top-access","refresh_token":"","email":"top@example.com"}"#;
        let t2 = extract_tokens(top);
        assert_eq!(t2.email.as_deref(), Some("top@example.com"));
        assert_eq!(t2.access_token.as_deref(), Some("top-access"));
        assert!(t2.refresh_token.is_none());

        let garbage = extract_tokens(b"not-json");
        assert!(garbage.access_token.is_none());
        assert!(garbage.refresh_token.is_none());
    }
}
