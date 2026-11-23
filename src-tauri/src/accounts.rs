//! 多账号档案：把 Antigravity 的登录凭据按账号备份到本地，用于一键切换。
//!
//! 目录结构（默认 `%APPDATA%\antigravity-usage-dashboard\accounts`）：
//! - `accounts.json`        账号索引（id / 邮箱 / 备注 / 置顶 / 切换次数 / 保存时间 / 最近使用）
//! - `accounts.json.bak`    上次成功写入的索引滚动备份（防止异常中断导致索引丢失）
//! - `<id>.cred.bin`        该账号凭据的 DPAPI 密文（原始 blob，可直接写回凭据管理器）
//!
//! 「当前账号」不是本地状态，而是实时读取 Windows 凭据管理器得到的真实登录身份，
//! 因此用户手动在 Antigravity 里换号后，这里也会同步。

use crate::credential::{self, CredentialInfo};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static TMP_SEQ: AtomicU64 = AtomicU64::new(1);

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn default_true() -> bool {
    true
}

fn default_version() -> u32 {
    2
}

/// 原子写入：先写入同目录临时文件，再 rename 替换目标文件，避免半截文件损坏数据。
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let seq = TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("tmp");
    let tmp_path = path.with_file_name(format!("{file_name}.tmp.{}.{seq}", std::process::id()));

    fs::write(&tmp_path, bytes)
        .map_err(|e| format!("写入临时文件 {} 失败: {e}", tmp_path.display()))?;

    if let Err(err) = fs::rename(&tmp_path, path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(format!("替换目标文件 {} 失败: {err}", path.display()));
    }
    Ok(())
}

/// 账号存档条目（不含任何令牌）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AccountEntry {
    pub id: String,
    pub email: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub auth_method: Option<String>,
    #[serde(default)]
    pub has_refresh_token: bool,
    #[serde(default)]
    pub expires_at_ms: Option<i64>,
    #[serde(default)]
    pub saved_at_ms: i64,
    #[serde(default)]
    pub last_used_at_ms: Option<i64>,
    #[serde(default)]
    pub switch_count: u32,
    #[serde(default)]
    pub blob_hash: Option<String>,
    /// 实时检测的本地备份文件是否存在且非空（不依赖持久化值）。
    #[serde(default = "default_true")]
    pub backup_ok: bool,
}

/// 实时读取到的当前登录状态。
#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CurrentCredential {
    pub present: bool,
    pub email: Option<String>,
    pub auth_method: Option<String>,
    pub has_refresh_token: bool,
    pub expires_at_ms: Option<i64>,
    pub expired: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountsList {
    pub current: CurrentCredential,
    pub accounts: Vec<AccountEntry>,
    pub store_dir: String,
    pub ignored_current: bool,
    pub error: Option<String>,
}

/// 单账号备份深度校验结果（包含 DPAPI 解密与 JSON 完整性检查）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountHealth {
    pub id: String,
    pub email: String,
    /// `"ok"` | `"missing"` | `"corrupted"`
    pub status: String,
    pub detail: Option<String>,
    pub has_refresh_token: bool,
    pub expires_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AccountIndex {
    #[serde(default = "default_version")]
    version: u32,
    #[serde(default)]
    accounts: Vec<AccountEntry>,
    /// 用户主动删除过的账号邮箱：避免「当前登录账号」被自动重新归档。
    #[serde(default)]
    ignored: Vec<String>,
}

impl Default for AccountIndex {
    fn default() -> Self {
        Self {
            version: default_version(),
            accounts: Vec::new(),
            ignored: Vec::new(),
        }
    }
}

/// 由邮箱派生的稳定 id（用于备份文件名，便于人工识别）。
pub fn account_id(email: &str) -> String {
    let lowered = email.trim().to_ascii_lowercase();
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in lowered.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    let safe: String = lowered
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c
            } else {
                '_'
            }
        })
        .take(24)
        .collect();
    format!("{safe}-{hash:016x}")
}

pub struct AccountStore {
    dir: PathBuf,
}

impl AccountStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// 与 proxy 设置同目录：`%APPDATA%\antigravity-usage-dashboard\accounts`
    pub fn default_store() -> Self {
        let base = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("antigravity-usage-dashboard");
        Self::new(base.join("accounts"))
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn index_path(&self) -> PathBuf {
        self.dir.join("accounts.json")
    }

    fn index_backup_path(&self) -> PathBuf {
        self.dir.join("accounts.json.bak")
    }

    fn blob_path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.cred.bin"))
    }

    fn blob_exists_non_empty(&self, id: &str) -> bool {
        fs::metadata(self.blob_path(id))
            .map(|m| m.is_file() && m.len() > 0)
            .unwrap_or(false)
    }

    pub fn ensure_dir(&self) -> Result<(), String> {
        fs::create_dir_all(&self.dir)
            .map_err(|e| format!("无法创建账号目录 {}: {e}", self.dir.display()))
    }

    fn load_index(&self) -> AccountIndex {
        if let Ok(raw) = fs::read_to_string(self.index_path()) {
            if let Ok(parsed) = serde_json::from_str::<AccountIndex>(&raw) {
                return parsed;
            }
        }
        // 主索引损坏时回退读取最近一次滚动备份，避免静默清空账号列表。
        if let Ok(raw) = fs::read_to_string(self.index_backup_path()) {
            if let Ok(parsed) = serde_json::from_str::<AccountIndex>(&raw) {
                return parsed;
            }
        }
        AccountIndex::default()
    }

    fn save_index(&self, index: &AccountIndex) -> Result<(), String> {
        self.ensure_dir()?;
        let json = serde_json::to_string_pretty(index)
            .map_err(|e| format!("序列化账号索引失败: {e}"))?;

        // 覆盖前保留一份上一版有效索引的滚动备份。
        let main_path = self.index_path();
        if main_path.is_file() {
            let _ = fs::copy(&main_path, self.index_backup_path());
        }

        write_atomic(&main_path, json.as_bytes()).map_err(|e| format!("写入账号索引失败: {e}"))
    }

    pub fn store_blob(&self, id: &str, blob: &[u8]) -> Result<(), String> {
        self.ensure_dir()?;
        let encrypted = credential::protect(blob)?;
        write_atomic(&self.blob_path(id), &encrypted)
            .map_err(|e| format!("写入账号凭据备份失败: {e}"))
    }

    pub fn load_blob(&self, id: &str) -> Result<Vec<u8>, String> {
        let path = self.blob_path(id);
        let raw = fs::read(&path)
            .map_err(|e| format!("找不到账号凭据备份 {}: {e}", path.display()))?;
        if raw.is_empty() {
            return Err(format!("账号凭据备份文件为空: {}", path.display()));
        }
        credential::unprotect(&raw)
            .map_err(|e| format!("账号凭据备份无法解密（可能不属于当前 Windows 用户）: {e}"))
    }

    pub fn get(&self, id: &str) -> Option<AccountEntry> {
        self.load_index().accounts.into_iter().find(|a| a.id == id).map(|mut a| {
            a.backup_ok = self.blob_exists_non_empty(&a.id);
            a
        })
    }

    pub fn contains_email(&self, email: &str) -> bool {
        let lower = email.trim().to_ascii_lowercase();
        self.load_index().accounts.iter().any(|a| a.email == lower)
    }

    #[cfg(test)]
    pub fn find_by_email(&self, email: &str) -> Option<AccountEntry> {
        self.load_index()
            .accounts
            .into_iter()
            .find(|a| a.email.eq_ignore_ascii_case(email.trim()))
    }

    /// 只读读取已排序账号列表（供系统托盘等轻量场景使用，不触发磁盘写入）。
    pub fn list_readonly(&self, current_email: Option<&str>) -> Vec<AccountEntry> {
        let cur = current_email.map(|e| e.trim().to_ascii_lowercase());
        let mut accounts = self.load_index().accounts;
        for acc in &mut accounts {
            acc.backup_ok = self.blob_exists_non_empty(&acc.id);
        }
        sort_accounts(&mut accounts, cur.as_deref());
        accounts
    }

    pub fn mark_used(&self, id: &str) -> Result<(), String> {
        let mut index = self.load_index();
        if let Some(entry) = index.accounts.iter_mut().find(|a| a.id == id) {
            entry.last_used_at_ms = Some(now_ms());
            entry.switch_count = entry.switch_count.saturating_add(1);
            self.save_index(&index)?;
        }
        Ok(())
    }

    pub fn rename(&self, id: &str, name: &str) -> Result<AccountEntry, String> {
        let trimmed: String = name.trim().chars().take(48).collect();
        let mut index = self.load_index();
        let entry = index
            .accounts
            .iter_mut()
            .find(|a| a.id == id)
            .ok_or_else(|| "账号不存在".to_string())?;
        entry.name = trimmed;
        let mut updated = entry.clone();
        updated.backup_ok = self.blob_exists_non_empty(&updated.id);
        self.save_index(&index)?;
        Ok(updated)
    }

    pub fn set_pinned(&self, id: &str, pinned: bool) -> Result<AccountEntry, String> {
        let mut index = self.load_index();
        let entry = index
            .accounts
            .iter_mut()
            .find(|a| a.id == id)
            .ok_or_else(|| "账号不存在".to_string())?;
        entry.pinned = pinned;
        let mut updated = entry.clone();
        updated.backup_ok = self.blob_exists_non_empty(&updated.id);
        self.save_index(&index)?;
        Ok(updated)
    }

    pub fn remove(&self, id: &str) -> Result<bool, String> {
        let mut index = self.load_index();
        let Some(entry) = index.accounts.iter().find(|a| a.id == id).cloned() else {
            return Ok(false);
        };
        index.accounts.retain(|a| a.id != id);
        if !index.ignored.iter().any(|e| e.eq_ignore_ascii_case(&entry.email)) {
            index.ignored.push(entry.email.clone());
        }
        self.save_index(&index)?;
        let path = self.blob_path(id);
        if path.exists() {
            let _ = fs::remove_file(path);
        }
        Ok(true)
    }

    /// 深度校验所有已保存账号的 DPAPI 加密备份是否可读且包含有效凭据。
    pub fn verify_all(&self) -> Vec<AccountHealth> {
        let index = self.load_index();
        let mut results = Vec::with_capacity(index.accounts.len());
        for acc in &index.accounts {
            let path = self.blob_path(&acc.id);
            if !path.is_file() {
                results.push(AccountHealth {
                    id: acc.id.clone(),
                    email: acc.email.clone(),
                    status: "missing".to_string(),
                    detail: Some("备份文件不存在".to_string()),
                    has_refresh_token: acc.has_refresh_token,
                    expires_at_ms: acc.expires_at_ms,
                });
                continue;
            }
            match self.load_blob(&acc.id) {
                Ok(plain) => {
                    let info = credential::parse_blob(&plain);
                    if !info.has_access_token && !info.has_refresh_token {
                        results.push(AccountHealth {
                            id: acc.id.clone(),
                            email: acc.email.clone(),
                            status: "corrupted".to_string(),
                            detail: Some("备份中缺少有效令牌字段".to_string()),
                            has_refresh_token: false,
                            expires_at_ms: info.expires_at_ms,
                        });
                    } else {
                        results.push(AccountHealth {
                            id: acc.id.clone(),
                            email: acc.email.clone(),
                            status: "ok".to_string(),
                            detail: None,
                            has_refresh_token: info.has_refresh_token,
                            expires_at_ms: info.expires_at_ms,
                        });
                    }
                }
                Err(err) => {
                    results.push(AccountHealth {
                        id: acc.id.clone(),
                        email: acc.email.clone(),
                        status: "corrupted".to_string(),
                        detail: Some(err),
                        has_refresh_token: acc.has_refresh_token,
                        expires_at_ms: acc.expires_at_ms,
                    });
                }
            }
        }
        results
    }

    /// 保存/更新当前登录账号的凭据备份。
    pub fn save_current(&self) -> Result<AccountEntry, String> {
        let blob = credential::read_blob()?
            .ok_or_else(|| "当前没有检测到 Antigravity 登录凭据，请先在 Antigravity 中登录".to_string())?;
        let info = credential::parse_blob(&blob);
        let email = info
            .email
            .clone()
            .ok_or_else(|| "当前凭据里没有账号邮箱，无法归档".to_string())?;
        self.upsert(&email, &info, &blob, true)
    }

    fn upsert(
        &self,
        email: &str,
        info: &CredentialInfo,
        blob: &[u8],
        explicit: bool,
    ) -> Result<AccountEntry, String> {
        let mut index = self.load_index();
        let now = now_ms();
        let lower = email.trim().to_ascii_lowercase();
        let hash = credential::blob_fingerprint(blob);
        index.ignored.retain(|e| !e.eq_ignore_ascii_case(&lower));

        let entry = if let Some(existing) = index.accounts.iter_mut().find(|a| a.email == lower) {
            existing.has_refresh_token = info.has_refresh_token;
            existing.auth_method = info.auth_method.clone();
            existing.expires_at_ms = info.expires_at_ms;
            existing.blob_hash = Some(hash);
            existing.backup_ok = true;
            if explicit {
                existing.saved_at_ms = now;
            }
            existing.clone()
        } else {
            let new_entry = AccountEntry {
                id: account_id(&lower),
                email: lower.clone(),
                name: String::new(),
                pinned: false,
                auth_method: info.auth_method.clone(),
                has_refresh_token: info.has_refresh_token,
                expires_at_ms: info.expires_at_ms,
                saved_at_ms: now,
                last_used_at_ms: None,
                switch_count: 0,
                blob_hash: Some(hash),
                backup_ok: true,
            };
            index.accounts.push(new_entry.clone());
            new_entry
        };

        self.store_blob(&entry.id, blob)?;
        self.save_index(&index)?;
        Ok(entry)
    }

    /// 读取账号列表，并把「当前登录但尚未归档」的账号自动补进列表（无变化时不写磁盘）。
    pub fn list_and_sync(&self) -> Result<AccountsList, String> {
        let mut index = self.load_index();
        let now = now_ms();
        let mut error: Option<String> = None;

        let current = match credential::read_blob() {
            Ok(Some(blob)) => {
                let info = credential::parse_blob(&blob);
                if let Some(email) = info.email.clone() {
                    if let Err(err) = self.auto_sync(&mut index, &email, &info, &blob) {
                        error = Some(err);
                    }
                }
                CurrentCredential {
                    present: true,
                    email: info.email.clone(),
                    auth_method: info.auth_method.clone(),
                    has_refresh_token: info.has_refresh_token,
                    expires_at_ms: info.expires_at_ms,
                    expired: info.is_expired(now),
                }
            }
            Ok(None) => CurrentCredential::default(),
            Err(err) => {
                error = Some(err);
                CurrentCredential::default()
            }
        };

        let current_email = current.email.clone().map(|e| e.to_ascii_lowercase());
        let ignored_current = current_email
            .as_deref()
            .map(|e| index.ignored.iter().any(|ig| ig.eq_ignore_ascii_case(e)))
            .unwrap_or(false);

        let mut accounts = index.accounts.clone();
        for acc in &mut accounts {
            acc.backup_ok = self.blob_exists_non_empty(&acc.id);
        }
        sort_accounts(&mut accounts, current_email.as_deref());

        Ok(AccountsList {
            current,
            accounts,
            store_dir: self.dir.display().to_string(),
            ignored_current,
            error,
        })
    }

    fn auto_sync(
        &self,
        index: &mut AccountIndex,
        email: &str,
        info: &CredentialInfo,
        blob: &[u8],
    ) -> Result<(), String> {
        let lower = email.trim().to_ascii_lowercase();
        if index.ignored.iter().any(|e| e.eq_ignore_ascii_case(&lower)) {
            return Ok(());
        }

        let hash = credential::blob_fingerprint(blob);
        let mut dirty = false;

        if let Some(existing) = index.accounts.iter_mut().find(|a| a.email == lower) {
            let blob_missing = !self.blob_exists_non_empty(&existing.id);
            let blob_changed = existing.blob_hash.as_deref() != Some(hash.as_str());
            let meta_changed = existing.expires_at_ms != info.expires_at_ms
                || existing.has_refresh_token != info.has_refresh_token
                || existing.auth_method != info.auth_method;

            if blob_missing || blob_changed {
                let id = existing.id.clone();
                self.store_blob(&id, blob)?;
                existing.blob_hash = Some(hash);
                dirty = true;
            }
            if meta_changed {
                existing.has_refresh_token = info.has_refresh_token;
                existing.auth_method = info.auth_method.clone();
                existing.expires_at_ms = info.expires_at_ms;
                dirty = true;
            }
            existing.backup_ok = true;
        } else {
            let entry = AccountEntry {
                id: account_id(&lower),
                email: lower,
                name: String::new(),
                pinned: false,
                auth_method: info.auth_method.clone(),
                has_refresh_token: info.has_refresh_token,
                expires_at_ms: info.expires_at_ms,
                saved_at_ms: now_ms(),
                last_used_at_ms: None,
                switch_count: 0,
                blob_hash: Some(hash),
                backup_ok: true,
            };
            self.store_blob(&entry.id, blob)?;
            index.accounts.push(entry);
            dirty = true;
        }

        if dirty {
            self.save_index(index)?;
        }
        Ok(())
    }
}

fn sort_accounts(accounts: &mut [AccountEntry], current_email: Option<&str>) {
    accounts.sort_by(|a, b| {
        let a_current = current_email == Some(a.email.as_str());
        let b_current = current_email == Some(b.email.as_str());
        b_current
            .cmp(&a_current)
            .then_with(|| b.pinned.cmp(&a.pinned))
            .then_with(|| b.last_used_at_ms.unwrap_or(0).cmp(&a.last_used_at_ms.unwrap_or(0)))
            .then_with(|| b.saved_at_ms.cmp(&a.saved_at_ms))
            .then_with(|| a.email.cmp(&b.email))
    });
}

/// 当前实时登录信息（无需存储目录）。
pub fn current_credential() -> Result<CurrentCredential, String> {
    let now = now_ms();
    Ok(match credential::read_blob()? {
        Some(blob) => {
            let info = credential::parse_blob(&blob);
            let expired = info.is_expired(now);
            CurrentCredential {
                present: true,
                email: info.email,
                auth_method: info.auth_method,
                has_refresh_token: info.has_refresh_token,
                expires_at_ms: info.expires_at_ms,
                expired,
            }
        }
        None => CurrentCredential::default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_blob(email: &str) -> Vec<u8> {
        format!(
            r#"{{"token":{{"access_token":"ya29.token","token_type":"Bearer","refresh_token":"1//refresh","expiry":"2026-11-20T11:33:36+08:00","expires_in":3599}},"auth_method":"consumer","id_token_email":"{email}","writer_id":"w1"}}"#
        )
        .into_bytes()
    }

    fn temp_store(tag: &str) -> AccountStore {
        let stamp = now_ms();
        let seq = TMP_SEQ.fetch_add(1, Ordering::Relaxed);
        AccountStore::new(
            std::env::temp_dir().join(format!(
                "agydock-accounts-{tag}-{}-{stamp}-{seq}",
                std::process::id()
            )),
        )
    }

    #[test]
    fn account_ids_are_stable_and_case_insensitive() {
        let a = account_id("Demo@Example.com");
        let b = account_id("demo@example.com");
        assert_eq!(a, b);
        assert_ne!(a, account_id("other@example.com"));
        assert!(a.len() <= 24 + 1 + 16);
    }

    #[cfg(windows)]
    #[test]
    fn store_roundtrip_list_rename_pin_and_remove() {
        let store = temp_store("roundtrip");
        let email = "demo@example.com";
        let blob = sample_blob(email);
        let info = credential::parse_blob(&blob);

        let entry = store.upsert(email, &info, &blob, true).unwrap();
        let id = entry.id.clone();

        assert_eq!(store.load_blob(&id).unwrap(), blob);
        assert_eq!(store.get(&id).unwrap().email, email);
        assert!(store.get(&id).unwrap().backup_ok);
        assert_eq!(store.find_by_email("DEMO@example.com").unwrap().id, id);

        let renamed = store.rename(&id, "  主力账号  ").unwrap();
        assert_eq!(renamed.name, "主力账号");
        assert_eq!(store.get(&id).unwrap().name, "主力账号");

        let pinned = store.set_pinned(&id, true).unwrap();
        assert!(pinned.pinned);
        assert!(store.get(&id).unwrap().pinned);

        store.mark_used(&id).unwrap();
        store.mark_used(&id).unwrap();
        let used = store.get(&id).unwrap();
        assert!(used.last_used_at_ms.is_some());
        assert_eq!(used.switch_count, 2);

        let health = store.verify_all();
        assert_eq!(health.len(), 1);
        assert_eq!(health[0].status, "ok");

        // 空名称应清空备注
        let cleared = store.rename(&id, "   ").unwrap();
        assert_eq!(cleared.name, "");

        assert!(store.remove(&id).unwrap());
        assert!(!store.remove(&id).unwrap());
        assert!(store.get(&id).is_none());

        let _ = fs::remove_dir_all(store.dir());
    }

    #[cfg(windows)]
    #[test]
    fn blob_tampering_and_missing_are_detected() {
        let store = temp_store("tamper");
        let email = "demo@example.com";
        let blob = sample_blob(email);
        let info = credential::parse_blob(&blob);
        let entry = store.upsert(email, &info, &blob, true).unwrap();
        let id = entry.id;

        let mut raw = fs::read(store.blob_path(&id)).unwrap();
        let last = raw.len() - 1;
        raw[last] ^= 0xff;
        fs::write(store.blob_path(&id), &raw).unwrap();
        assert!(store.load_blob(&id).is_err());

        let health = store.verify_all();
        assert_eq!(health[0].status, "corrupted");

        fs::remove_file(store.blob_path(&id)).unwrap();
        assert!(!store.get(&id).unwrap().backup_ok);
        let health2 = store.verify_all();
        assert_eq!(health2[0].status, "missing");

        // auto_sync should self-heal a missing blob for the currently logged-in account
        let mut index = store.load_index();
        store.auto_sync(&mut index, email, &info, &blob).unwrap();
        assert!(store.get(&id).unwrap().backup_ok);

        let _ = fs::remove_dir_all(store.dir());
    }

    #[cfg(windows)]
    #[test]
    fn removed_account_is_not_auto_rearchived_but_explicit_save_restores_it() {
        let store = temp_store("ignored");
        let email = "demo@example.com";
        let blob = sample_blob(email);
        let info = credential::parse_blob(&blob);

        let entry = store.upsert(email, &info, &blob, true).unwrap();
        let id = entry.id.clone();
        assert!(store.remove(&id).unwrap());
        assert!(store.get(&id).is_none());

        let mut index = store.load_index();
        store.auto_sync(&mut index, email, &info, &blob).unwrap();
        assert!(store.get(&id).is_none(), "被删除的账号不应被自动重新归档");

        let restored = store.upsert(email, &info, &blob, true).unwrap();
        assert_eq!(restored.id, id);
        assert!(store.get(&id).is_some());

        let _ = fs::remove_dir_all(store.dir());
    }

    #[cfg(windows)]
    #[test]
    fn corrupted_index_falls_back_to_backup_file() {
        let store = temp_store("bak-fallback");
        let blob1 = sample_blob("one@example.com");
        let info1 = credential::parse_blob(&blob1);
        store.upsert("one@example.com", &info1, &blob1, true).unwrap();

        let blob2 = sample_blob("two@example.com");
        let info2 = credential::parse_blob(&blob2);
        store.upsert("two@example.com", &info2, &blob2, true).unwrap();

        // Corrupt main accounts.json -> should recover from accounts.json.bak
        fs::write(store.index_path(), "{corrupted").unwrap();
        let recovered = store.load_index();
        assert!(!recovered.accounts.is_empty());

        let _ = fs::remove_dir_all(store.dir());
    }

    #[test]
    fn index_parsing_tolerates_missing_file_and_legacy_v1_fields() {
        let store = temp_store("index");
        assert!(store.load_index().accounts.is_empty());
        let legacy_json = r#"{"version":1,"accounts":[{"id":"x","email":"a@b.com","name":"","authMethod":null,"hasRefreshToken":false,"expiresAtMs":null,"savedAtMs":0,"lastUsedAtMs":null}]}"#;
        let parsed: AccountIndex = serde_json::from_str(legacy_json).unwrap();
        assert_eq!(parsed.accounts.len(), 1);
        assert!(!parsed.accounts[0].pinned);
        assert_eq!(parsed.accounts[0].switch_count, 0);
        assert!(parsed.accounts[0].backup_ok);
    }

    #[test]
    fn sorting_prioritizes_current_then_pinned_then_recent() {
        let mut items = vec![
            AccountEntry {
                id: "1".into(),
                email: "old@example.com".into(),
                name: String::new(),
                pinned: false,
                auth_method: None,
                has_refresh_token: true,
                expires_at_ms: None,
                saved_at_ms: 100,
                last_used_at_ms: Some(500),
                switch_count: 1,
                blob_hash: None,
                backup_ok: true,
            },
            AccountEntry {
                id: "2".into(),
                email: "pinned@example.com".into(),
                name: String::new(),
                pinned: true,
                auth_method: None,
                has_refresh_token: true,
                expires_at_ms: None,
                saved_at_ms: 50,
                last_used_at_ms: None,
                switch_count: 0,
                blob_hash: None,
                backup_ok: true,
            },
            AccountEntry {
                id: "3".into(),
                email: "current@example.com".into(),
                name: String::new(),
                pinned: false,
                auth_method: None,
                has_refresh_token: true,
                expires_at_ms: None,
                saved_at_ms: 10,
                last_used_at_ms: None,
                switch_count: 0,
                blob_hash: None,
                backup_ok: true,
            },
        ];
        sort_accounts(&mut items, Some("current@example.com"));
        assert_eq!(items[0].email, "current@example.com");
        assert_eq!(items[1].email, "pinned@example.com");
        assert_eq!(items[2].email, "old@example.com");
    }
}
