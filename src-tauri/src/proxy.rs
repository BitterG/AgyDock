use chrono::Local;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::thread;
use std::time::Duration;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

const FALLBACK_HTTP_PORTS: [u16; 8] = [7897, 7899, 7890, 7891, 10809, 10808, 2080, 1080];

fn re_mixed_port() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?m)^\s*(?:verge_mixed_port|mixed-port)\s*:\s*(\d+)").unwrap())
}

fn re_fallback_port() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?m)^\s*(?:verge_port|port)\s*:\s*(\d+)").unwrap())
}

fn re_clash_mode() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?m)^\s*mode\s*:\s*([A-Za-z0-9_-]+)").unwrap())
}

fn re_tun_desc() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)Meta|Mihomo|Clash|Tun|Wintun|Sing-box").unwrap())
}

fn re_tun_name() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)^(Meta|Mihomo|Clash|utun)").unwrap())
}

fn re_fake_ip() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^198\.1[89]\.").unwrap())
}

fn default_antigravity_path() -> String {
    if let Some(local_app) = dirs::data_local_dir() {
        let p = local_app
            .join("Programs")
            .join("antigravity")
            .join("Antigravity.exe");
        if p.exists() {
            return p.to_string_lossy().to_string();
        }
        return p.to_string_lossy().to_string();
    }
    String::new()
}

fn default_clash_verge_path() -> String {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(pf) = std::env::var("ProgramFiles") {
        candidates.push(PathBuf::from(&pf).join("Clash Verge").join("clash-verge.exe"));
        candidates.push(PathBuf::from(&pf).join("Clash Verge Rev").join("clash-verge.exe"));
    }
    if let Some(local_app) = dirs::data_local_dir() {
        candidates.push(
            local_app
                .join("Programs")
                .join("Clash Verge")
                .join("clash-verge.exe"),
        );
        candidates.push(
            local_app
                .join("Programs")
                .join("clash-verge")
                .join("clash-verge.exe"),
        );
    }
    candidates.push(PathBuf::from(r"C:\Program Files\Clash Verge\clash-verge.exe"));

    for c in &candidates {
        if c.exists() {
            return c.to_string_lossy().to_string();
        }
    }
    candidates
        .first()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default()
}

fn default_true() -> bool {
    true
}

/// 内置国内常用域名后缀（不含 .cn，.cn 已单独通配），用于在启用「国内/内网直连绕过」时
/// 避免访问国内站点走海外代理节点触发风控。
const CHINA_DIRECT_DOMAINS: &[&str] = &[
    "baidu.com",
    "bdstatic.com",
    "bcebos.com",
    "qq.com",
    "gtimg.com",
    "qcloud.com",
    "myqcloud.com",
    "tencent.com",
    "tencentcloudapi.com",
    "alibaba.com",
    "alicdn.com",
    "aliyun.com",
    "aliyuncs.com",
    "taobao.com",
    "tmall.com",
    "alipay.com",
    "dingtalk.com",
    "1688.com",
    "amap.com",
    "npmmirror.com",
    "yuque.com",
    "bytedance.com",
    "byteimg.com",
    "douyin.com",
    "toutiao.com",
    "larkoffice.com",
    "volces.com",
    "volcengine.com",
    "jd.com",
    "360buyimg.com",
    "pinduoduo.com",
    "meituan.com",
    "dianping.com",
    "ctrip.com",
    "bilibili.com",
    "hdslb.com",
    "zhihu.com",
    "zhimg.com",
    "weibo.com",
    "sina.com",
    "xiaohongshu.com",
    "xhscdn.com",
    "douban.com",
    "163.com",
    "126.net",
    "csdn.net",
    "cnblogs.com",
    "oschina.net",
    "gitee.com",
    "segmentfault.com",
    "mi.com",
    "xiaomi.com",
    "huawei.com",
    "huaweicloud.com",
    "deepseek.com",
    "baichuan-ai.com",
    "qiniu.com",
    "qiniucdn.com",
    "upyun.com",
    "ccb.com",
    "icbc.com",
    "cmbchina.com",
    "abchina.com",
];

const LAN_BYPASS_CIDRS: &[&str] = &[
    "10.0.0.0/8",
    "172.16.0.0/12",
    "192.168.0.0/16",
    "169.254.0.0/16",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxySettings {
    pub proxy_host: String,
    pub auto_detect_port: bool,
    pub custom_port: u16,
    pub antigravity_path: String,
    pub clash_verge_path: String,
    pub workspace_path: String,
    pub auto_start_clash: bool,
    pub disable_quic: bool,
    pub show_loopback_ipc: bool,
    #[serde(default = "default_true")]
    pub bypass_china_and_lan: bool,
    #[serde(default)]
    pub custom_bypass_domains: String,
}

impl Default for ProxySettings {
    fn default() -> Self {
        Self {
            proxy_host: "127.0.0.1".to_string(),
            auto_detect_port: true,
            custom_port: 7897,
            antigravity_path: default_antigravity_path(),
            clash_verge_path: default_clash_verge_path(),
            workspace_path: String::new(),
            auto_start_clash: true,
            disable_quic: true,
            show_loopback_ipc: false,
            bypass_china_and_lan: true,
            custom_bypass_domains: String::new(),
        }
    }
}

impl ProxySettings {
    pub fn sanitized(mut self) -> Self {
        if self.proxy_host.trim().is_empty() {
            self.proxy_host = "127.0.0.1".to_string();
        } else {
            self.proxy_host = self.proxy_host.trim().to_string();
        }
        if self.custom_port == 0 {
            self.custom_port = 7897;
        }
        if self.antigravity_path.trim().is_empty() {
            self.antigravity_path = default_antigravity_path();
        } else {
            self.antigravity_path = self.antigravity_path.trim().to_string();
        }
        if self.clash_verge_path.trim().is_empty() {
            self.clash_verge_path = default_clash_verge_path();
        } else {
            self.clash_verge_path = self.clash_verge_path.trim().to_string();
        }
        self.workspace_path = self.workspace_path.trim().to_string();
        self.custom_bypass_domains = self.custom_bypass_domains.trim().to_string();
        self
    }

    fn settings_path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("antigravity-usage-dashboard").join("proxy-settings.json"))
    }

    pub fn load() -> Self {
        if let Some(p) = Self::settings_path() {
            if let Ok(raw) = fs::read_to_string(&p) {
                if let Ok(parsed) = serde_json::from_str::<ProxySettings>(&raw) {
                    return parsed.sanitized();
                }
            }
        }
        Self::default().sanitized()
    }

    pub fn save(&self) {
        let Some(p) = Self::settings_path() else {
            return;
        };
        if let Some(parent) = p.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = fs::write(p, json);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ConnectionCategory {
    Proxied,
    TunFakeIp,
    PublicDirect,
    LocalIpc,
}

impl ConnectionCategory {
    pub fn label(self) -> &'static str {
        match self {
            Self::Proxied => "走代理",
            Self::TunFakeIp => "TUN Fake-IP",
            Self::PublicDirect => "公网直连",
            Self::LocalIpc => "本地/内网",
        }
    }

    fn sort_order(self) -> u8 {
        match self {
            Self::PublicDirect => 0,
            Self::TunFakeIp => 1,
            Self::Proxied => 2,
            Self::LocalIpc => 3,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TcpConnectionItem {
    pub key: String,
    pub category: ConnectionCategory,
    pub category_label: String,
    pub pid: u32,
    pub process_name: String,
    pub local_endpoint: String,
    pub remote_endpoint: String,
    pub remote_host: String,
    pub remote_port: u16,
    pub state: String,
    pub seen_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessEntry {
    pub pid: u32,
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxySnapshot {
    pub proxy_host: String,
    pub resolved_port: u16,
    pub port_source: String,
    pub is_proxy_listening: bool,
    pub clash_mode: String,
    pub is_clash_running: bool,
    pub system_proxy_enabled: bool,
    pub system_proxy_server: String,
    pub tun_enabled: bool,
    pub tun_adapter_text: String,
    pub antigravity_proc_count: usize,
    pub language_server_proc_count: usize,
    pub processes: Vec<ProcessEntry>,
    pub proxied_count: usize,
    pub tun_count: usize,
    pub direct_count: usize,
    pub connections: Vec<TcpConnectionItem>,
    pub captured_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionResponse {
    pub ok: bool,
    pub need_confirm: bool,
    pub process_count: usize,
    pub resolved_port: u16,
    pub logs: Vec<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditProgressPayload {
    pub elapsed: u32,
    pub total: u32,
    pub snapshot: ProxySnapshot,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditResult {
    pub cancelled: bool,
    pub latest_snapshot: ProxySnapshot,
    pub cumulative_proxied_count: usize,
    pub cumulative_tun_count: usize,
    pub cumulative_direct_count: usize,
    pub cumulative_connections: Vec<TcpConnectionItem>,
    pub report_text: String,
    pub report_path: String,
    pub passed: Option<bool>,
}

pub fn get_clash_verge_config_dirs(clash_verge_path: &str) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if !clash_verge_path.trim().is_empty() {
        if let Some(clash_dir) = Path::new(clash_verge_path.trim()).parent() {
            candidates.push(
                clash_dir
                    .join(".config")
                    .join("io.github.clash-verge-rev.clash-verge-rev"),
            );
            candidates.push(clash_dir.join(".config"));
            candidates.push(clash_dir.join("data"));
        }
    }

    if let Some(app_data) = dirs::config_dir() {
        candidates.push(app_data.join("io.github.clash-verge-rev.clash-verge-rev"));
        candidates.push(app_data.join("clash-verge-rev"));
        candidates.push(app_data.join("clash-verge"));
    }
    if let Some(local_data) = dirs::data_local_dir() {
        candidates.push(local_data.join("io.github.clash-verge-rev.clash-verge-rev"));
    }

    let mut seen = HashSet::new();
    let mut existing = Vec::new();
    for dir in candidates {
        if dir.is_dir() {
            let key = dir.to_string_lossy().to_lowercase();
            if seen.insert(key) {
                existing.push(dir);
            }
        }
    }
    existing
}

#[cfg(test)]
pub fn parse_clash_yaml_port_and_mode(content: &str) -> (u16, Option<String>) {
    let port = re_mixed_port()
        .captures(content)
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().parse::<u16>().ok())
        .filter(|&p| p > 0)
        .or_else(|| {
            re_fallback_port()
                .captures(content)
                .and_then(|c| c.get(1))
                .and_then(|m| m.as_str().parse::<u16>().ok())
                .filter(|&p| p > 0)
        })
        .unwrap_or(0);

    let mode = re_clash_mode()
        .captures(content)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().trim().to_string())
        .filter(|s| !s.is_empty());

    (port, mode)
}

pub fn read_clash_verge_config(clash_verge_path: &str) -> (u16, String) {
    let dirs = get_clash_verge_config_dirs(clash_verge_path);
    let mut port: u16 = 0;
    let mut mode = "未知".to_string();

    for dir in &dirs {
        for fname in ["verge.yaml", "config.yaml", "clash-verge.yaml"] {
            let file = dir.join(fname);
            if let Ok(content) = fs::read_to_string(&file) {
                if let Some(p) = re_mixed_port()
                    .captures(&content)
                    .and_then(|c| c.get(1))
                    .and_then(|m| m.as_str().parse::<u16>().ok())
                    .filter(|&v| v > 0)
                {
                    port = p;
                    break;
                }
            }
        }
        if port > 0 {
            break;
        }
    }

    if port == 0 {
        for dir in &dirs {
            for fname in ["verge.yaml", "config.yaml"] {
                let file = dir.join(fname);
                if let Ok(content) = fs::read_to_string(&file) {
                    if let Some(p) = re_fallback_port()
                        .captures(&content)
                        .and_then(|c| c.get(1))
                        .and_then(|m| m.as_str().parse::<u16>().ok())
                        .filter(|&v| v > 0)
                    {
                        port = p;
                        break;
                    }
                }
            }
            if port > 0 {
                break;
            }
        }
    }

    for dir in &dirs {
        for fname in ["config.yaml", "clash-verge.yaml"] {
            let file = dir.join(fname);
            if let Ok(content) = fs::read_to_string(&file) {
                if let Some(m) = re_clash_mode()
                    .captures(&content)
                    .and_then(|c| c.get(1))
                    .map(|g| g.as_str().trim().to_string())
                    .filter(|s| !s.is_empty())
                {
                    mode = m;
                    break;
                }
            }
        }
        if mode != "未知" {
            break;
        }
    }

    (port, mode)
}

pub fn test_tcp_port(host: &str, port: u16, timeout_ms: u64) -> bool {
    if port == 0 || host.trim().is_empty() {
        return false;
    }
    let timeout = Duration::from_millis(timeout_ms);
    if let Ok(ip) = host.trim().parse::<IpAddr>() {
        return TcpStream::connect_timeout(&SocketAddr::new(ip, port), timeout).is_ok();
    }
    let addr_str = format!("{}:{}", host.trim(), port);
    if let Ok(mut addrs) = addr_str.to_socket_addrs() {
        if let Some(addr) = addrs.next() {
            return TcpStream::connect_timeout(&addr, timeout).is_ok();
        }
    }
    false
}

pub fn resolve_port(settings: &ProxySettings) -> (u16, String, bool, String) {
    let (cfg_port, mode) = read_clash_verge_config(&settings.clash_verge_path);

    if !settings.auto_detect_port {
        let manual = if settings.custom_port > 0 {
            settings.custom_port
        } else {
            7897
        };
        let ok = test_tcp_port(&settings.proxy_host, manual, 350);
        return (manual, "手动指定".to_string(), ok, mode);
    }

    if cfg_port > 0 && test_tcp_port(&settings.proxy_host, cfg_port, 350) {
        return (cfg_port, "Clash Verge 配置".to_string(), true, mode);
    }

    for candidate in FALLBACK_HTTP_PORTS {
        if test_tcp_port(&settings.proxy_host, candidate, 160) {
            return (candidate, "本地端口探测".to_string(), true, mode);
        }
    }

    if cfg_port > 0 {
        return (cfg_port, "Clash Verge 配置".to_string(), false, mode);
    }

    (7897, "默认端口".to_string(), false, mode)
}

fn is_local_or_private_ip(host: &str) -> bool {
    if let Ok(ip) = host.trim().parse::<IpAddr>() {
        return match ip {
            IpAddr::V4(v4) => {
                v4.is_loopback()
                    || v4.is_private()
                    || v4.is_link_local()
                    || v4.is_unspecified()
            }
            IpAddr::V6(v6) => {
                let seg0 = v6.segments()[0];
                v6.is_loopback()
                    || v6.is_unspecified()
                    || (seg0 & 0xfe00) == 0xfc00
                    || (seg0 & 0xffc0) == 0xfe80
            }
        };
    }
    host.starts_with("127.") || host == "0.0.0.0" || host == "::1" || host == "::"
}

pub fn classify_remote(
    remote_host: &str,
    remote_port: u16,
    proxy_host: &str,
    proxy_port: u16,
) -> ConnectionCategory {
    let host_is_local_proxy = remote_host.eq_ignore_ascii_case(proxy_host)
        || remote_host == "127.0.0.1"
        || remote_host == "::1";

    if proxy_port > 0 && host_is_local_proxy && remote_port == proxy_port {
        return ConnectionCategory::Proxied;
    }

    if re_fake_ip().is_match(remote_host) {
        return ConnectionCategory::TunFakeIp;
    }

    if is_local_or_private_ip(remote_host) {
        return ConnectionCategory::LocalIpc;
    }

    ConnectionCategory::PublicDirect
}

#[derive(Debug, Clone)]
struct RawTcpSocket {
    pid: u32,
    local_endpoint: String,
    remote_host: String,
    remote_port: u16,
    remote_endpoint: String,
    state: String,
}

#[cfg(windows)]
mod win_native {
    use super::*;
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::Foundation::{CloseHandle, ERROR_BUFFER_OVERFLOW, ERROR_INSUFFICIENT_BUFFER, INVALID_HANDLE_VALUE, NO_ERROR};
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        GetAdaptersAddresses, GetExtendedTcpTable, GAA_FLAG_INCLUDE_PREFIX, IP_ADAPTER_ADDRESSES_LH,
        MIB_TCP6ROW_OWNER_PID, MIB_TCPROW_OWNER_PID, TCP_TABLE_OWNER_PID_ALL,
    };
    use windows_sys::Win32::NetworkManagement::Ndis::IfOperStatusUp;
    use windows_sys::Win32::Networking::WinSock::{AF_INET, AF_INET6, AF_UNSPEC};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, TerminateProcess,
        PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE,
    };
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    const MIB_TCP_STATE_SYN_SENT: u32 = 3;
    const MIB_TCP_STATE_ESTAB: u32 = 5;

    pub fn get_system_proxy_state() -> (bool, String) {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let Ok(key) = hkcu.open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Internet Settings")
        else {
            return (false, String::new());
        };
        let enabled = key.get_value::<u32, _>("ProxyEnable").unwrap_or(0) == 1;
        let server = key.get_value::<String, _>("ProxyServer").unwrap_or_default();
        (enabled, server)
    }

    unsafe fn pwstr_to_string(ptr: *const u16) -> String {
        if ptr.is_null() {
            return String::new();
        }
        let mut len = 0usize;
        while *ptr.add(len) != 0 {
            len += 1;
        }
        OsString::from_wide(std::slice::from_raw_parts(ptr, len))
            .to_string_lossy()
            .to_string()
    }

    pub fn get_tun_adapter_state() -> (bool, String) {
        let mut buf_len: u32 = 16 * 1024;
        let mut buf = vec![0u8; buf_len as usize];
        let mut ret = unsafe {
            GetAdaptersAddresses(
                AF_UNSPEC as u32,
                GAA_FLAG_INCLUDE_PREFIX,
                std::ptr::null_mut(),
                buf.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH,
                &mut buf_len,
            )
        };
        if ret == ERROR_BUFFER_OVERFLOW && buf_len > 0 {
            buf.resize(buf_len as usize, 0);
            ret = unsafe {
                GetAdaptersAddresses(
                    AF_UNSPEC as u32,
                    GAA_FLAG_INCLUDE_PREFIX,
                    std::ptr::null_mut(),
                    buf.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH,
                    &mut buf_len,
                )
            };
        }
        if ret != NO_ERROR {
            return (false, String::new());
        }

        let mut matches = Vec::new();
        let mut cur = buf.as_ptr() as *const IP_ADAPTER_ADDRESSES_LH;
        while !cur.is_null() {
            let adapter = unsafe { &*cur };
            if adapter.OperStatus == IfOperStatusUp {
                let name = unsafe { pwstr_to_string(adapter.FriendlyName) };
                let desc = unsafe { pwstr_to_string(adapter.Description) };
                if re_tun_desc().is_match(&desc) || re_tun_name().is_match(&name) {
                    matches.push(format!("{name} ({desc})"));
                }
            }
            cur = adapter.Next;
        }

        if matches.is_empty() {
            (false, String::new())
        } else {
            (true, matches.join(", "))
        }
    }

    fn query_process_image_path(pid: u32) -> Option<String> {
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if handle.is_null() {
            return None;
        }
        let mut buf = vec![0u16; 1024];
        let mut size = buf.len() as u32;
        let ok = unsafe { QueryFullProcessImageNameW(handle, 0, buf.as_mut_ptr(), &mut size) };
        unsafe { CloseHandle(handle) };
        if ok == 0 || size == 0 {
            return None;
        }
        Some(
            OsString::from_wide(&buf[..size as usize])
                .to_string_lossy()
                .to_string(),
        )
    }

    pub fn scan_antigravity_and_clash(antigravity_path: &str) -> (Vec<ProcessEntry>, bool) {
        let install_dir = Path::new(antigravity_path.trim())
            .parent()
            .map(|p| p.to_string_lossy().to_lowercase())
            .filter(|s| !s.is_empty());

        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
        if snapshot == INVALID_HANDLE_VALUE || snapshot.is_null() {
            return (Vec::new(), false);
        }

        let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;

        let mut procs = Vec::new();
        let mut clash_running = false;

        let mut has_next = unsafe { Process32FirstW(snapshot, &mut entry) } != 0;
        while has_next {
            let nul_pos = entry
                .szExeFile
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(entry.szExeFile.len());
            let exe_name = OsString::from_wide(&entry.szExeFile[..nul_pos])
                .to_string_lossy()
                .to_string();
            let stem = exe_name
                .strip_suffix(".exe")
                .or_else(|| exe_name.strip_suffix(".EXE"))
                .unwrap_or(&exe_name);
            let stem_lower = stem.to_lowercase();

            if stem_lower.contains("clash-verge") || stem_lower.contains("verge-mihomo") {
                clash_running = true;
            }

            let is_target_name = (stem_lower.starts_with("antigravity")
                || stem_lower.starts_with("language_server"))
                && stem_lower != "uninstall antigravity"
                && stem_lower != "agydock"
                && stem_lower != "antigravityusage"
                && stem_lower != "antigravity-usage-desktop"
                && stem_lower != "antigravitytunpanel";

            if is_target_name {
                if let Some(ref dir_lower) = install_dir {
                    if let Some(full_path) = query_process_image_path(entry.th32ProcessID) {
                        if full_path.to_lowercase().starts_with(dir_lower) {
                            procs.push(ProcessEntry {
                                pid: entry.th32ProcessID,
                                name: stem.to_string(),
                            });
                        }
                    }
                }
            }

            has_next = unsafe { Process32NextW(snapshot, &mut entry) } != 0;
        }

        unsafe { CloseHandle(snapshot) };
        procs.sort_by_key(|p| p.pid);
        (procs, clash_running)
    }

    pub fn terminate_pid(pid: u32) -> Result<(), String> {
        let handle = unsafe { OpenProcess(PROCESS_TERMINATE, 0, pid) };
        if handle.is_null() {
            return Err("无法打开进程句柄".to_string());
        }
        let ok = unsafe { TerminateProcess(handle, 1) };
        unsafe { CloseHandle(handle) };
        if ok == 0 {
            Err("TerminateProcess 调用失败".to_string())
        } else {
            Ok(())
        }
    }

    fn decode_port(raw_port: u32) -> u16 {
        (((raw_port & 0xFF) << 8) | ((raw_port >> 8) & 0xFF)) as u16
    }

    fn state_label(state: u32) -> Option<&'static str> {
        match state {
            MIB_TCP_STATE_ESTAB => Some("ESTABLISHED"),
            MIB_TCP_STATE_SYN_SENT => Some("SYN_SENT"),
            _ => None,
        }
    }

    fn normalize_ipv6(bytes: [u8; 16]) -> IpAddr {
        let v6 = Ipv6Addr::from(bytes);
        if let Some(v4) = v6.to_ipv4_mapped() {
            IpAddr::V4(v4)
        } else {
            IpAddr::V6(v6)
        }
    }

    fn format_endpoint(ip: IpAddr, port: u16) -> String {
        match ip {
            IpAddr::V4(v4) => format!("{v4}:{port}"),
            IpAddr::V6(v6) => format!("[{v6}]:{port}"),
        }
    }

    fn query_tcp_table(af: u32, pid_filter: &HashSet<u32>, is_ipv6: bool, out: &mut Vec<RawTcpSocket>) {
        let mut size: u32 = 0;
        let mut ret = unsafe {
            GetExtendedTcpTable(
                std::ptr::null_mut(),
                &mut size,
                0,
                af,
                TCP_TABLE_OWNER_PID_ALL,
                0,
            )
        };
        if ret != ERROR_INSUFFICIENT_BUFFER && ret != NO_ERROR {
            return;
        }
        if size == 0 {
            return;
        }

        let mut buf: Vec<u8> = Vec::new();
        for _ in 0..3 {
            buf.resize(size as usize, 0);
            ret = unsafe {
                GetExtendedTcpTable(
                    buf.as_mut_ptr() as *mut _,
                    &mut size,
                    0,
                    af,
                    TCP_TABLE_OWNER_PID_ALL,
                    0,
                )
            };
            if ret == NO_ERROR {
                break;
            }
            if ret != ERROR_INSUFFICIENT_BUFFER {
                return;
            }
        }
        if ret != NO_ERROR || buf.len() < 4 {
            return;
        }

        let num_entries = u32::from_ne_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
        if !is_ipv6 {
            let row_size = std::mem::size_of::<MIB_TCPROW_OWNER_PID>();
            let mut offset = 4usize;
            for _ in 0..num_entries {
                if offset + row_size > buf.len() {
                    break;
                }
                let row = unsafe {
                    std::ptr::read_unaligned(buf.as_ptr().add(offset) as *const MIB_TCPROW_OWNER_PID)
                };
                offset += row_size;
                if let Some(st) = state_label(row.dwState) {
                    if pid_filter.contains(&row.dwOwningPid) {
                        let local_ip = IpAddr::V4(Ipv4Addr::from(row.dwLocalAddr.to_ne_bytes()));
                        let remote_ip = IpAddr::V4(Ipv4Addr::from(row.dwRemoteAddr.to_ne_bytes()));
                        let local_port = decode_port(row.dwLocalPort);
                        let remote_port = decode_port(row.dwRemotePort);
                        out.push(RawTcpSocket {
                            pid: row.dwOwningPid,
                            local_endpoint: format_endpoint(local_ip, local_port),
                            remote_host: remote_ip.to_string(),
                            remote_port,
                            remote_endpoint: format_endpoint(remote_ip, remote_port),
                            state: st.to_string(),
                        });
                    }
                }
            }
        } else {
            let row_size = std::mem::size_of::<MIB_TCP6ROW_OWNER_PID>();
            let mut offset = 4usize;
            for _ in 0..num_entries {
                if offset + row_size > buf.len() {
                    break;
                }
                let row = unsafe {
                    std::ptr::read_unaligned(buf.as_ptr().add(offset) as *const MIB_TCP6ROW_OWNER_PID)
                };
                offset += row_size;
                if let Some(st) = state_label(row.dwState) {
                    if pid_filter.contains(&row.dwOwningPid) {
                        let local_ip = normalize_ipv6(row.ucLocalAddr);
                        let remote_ip = normalize_ipv6(row.ucRemoteAddr);
                        let local_port = decode_port(row.dwLocalPort);
                        let remote_port = decode_port(row.dwRemotePort);
                        out.push(RawTcpSocket {
                            pid: row.dwOwningPid,
                            local_endpoint: format_endpoint(local_ip, local_port),
                            remote_host: remote_ip.to_string(),
                            remote_port,
                            remote_endpoint: format_endpoint(remote_ip, remote_port),
                            state: st.to_string(),
                        });
                    }
                }
            }
        }
    }

    pub fn get_active_tcp_sockets(pid_filter: &HashSet<u32>) -> Vec<RawTcpSocket> {
        let mut out = Vec::new();
        if pid_filter.is_empty() {
            return out;
        }
        query_tcp_table(AF_INET as u32, pid_filter, false, &mut out);
        query_tcp_table(AF_INET6 as u32, pid_filter, true, &mut out);
        out
    }
}

#[cfg(not(windows))]
mod win_native {
    use super::*;
    pub fn get_system_proxy_state() -> (bool, String) {
        (false, String::new())
    }
    pub fn get_tun_adapter_state() -> (bool, String) {
        (false, String::new())
    }
    pub fn scan_antigravity_and_clash(_antigravity_path: &str) -> (Vec<ProcessEntry>, bool) {
        (Vec::new(), false)
    }
    pub fn terminate_pid(_pid: u32) -> Result<(), String> {
        Err("仅支持 Windows".to_string())
    }
    pub fn get_active_tcp_sockets(_pid_filter: &HashSet<u32>) -> Vec<RawTcpSocket> {
        Vec::new()
    }
}

pub fn build_snapshot(settings: &ProxySettings) -> ProxySnapshot {
    let (resolved_port, port_source, is_listening, clash_mode) = resolve_port(settings);
    let (sys_proxy_enabled, sys_proxy_server) = win_native::get_system_proxy_state();
    let (tun_enabled, tun_adapter_text) = win_native::get_tun_adapter_state();
    let (procs, clash_running) = win_native::scan_antigravity_and_clash(&settings.antigravity_path);

    let mut pid_map: HashMap<u32, String> = HashMap::new();
    let mut pid_set: HashSet<u32> = HashSet::new();
    let mut agy_count = 0usize;
    let mut ls_count = 0usize;

    for p in &procs {
        pid_map.insert(p.pid, p.name.clone());
        pid_set.insert(p.pid);
        if p.name.to_lowercase().starts_with("language_server") {
            ls_count += 1;
        } else {
            agy_count += 1;
        }
    }

    let seen_at = Local::now().format("%H:%M:%S").to_string();
    let raw_sockets = win_native::get_active_tcp_sockets(&pid_set);
    let mut unique: HashMap<String, TcpConnectionItem> = HashMap::new();

    for sock in raw_sockets {
        let key = format!("{}|{}->{}", sock.pid, sock.local_endpoint, sock.remote_endpoint);
        if unique.contains_key(&key) {
            continue;
        }
        let cat = classify_remote(
            &sock.remote_host,
            sock.remote_port,
            &settings.proxy_host,
            resolved_port,
        );
        let proc_name = pid_map
            .get(&sock.pid)
            .cloned()
            .unwrap_or_else(|| "unknown".to_string());
        unique.insert(
            key.clone(),
            TcpConnectionItem {
                key,
                category: cat,
                category_label: cat.label().to_string(),
                pid: sock.pid,
                process_name: proc_name,
                local_endpoint: sock.local_endpoint,
                remote_endpoint: sock.remote_endpoint,
                remote_host: sock.remote_host,
                remote_port: sock.remote_port,
                state: sock.state,
                seen_at: seen_at.clone(),
            },
        );
    }

    let mut connections: Vec<TcpConnectionItem> = unique.into_values().collect();
    connections.sort_by(|a, b| {
        a.category
            .sort_order()
            .cmp(&b.category.sort_order())
            .then_with(|| a.process_name.to_lowercase().cmp(&b.process_name.to_lowercase()))
            .then_with(|| a.pid.cmp(&b.pid))
            .then_with(|| a.remote_endpoint.cmp(&b.remote_endpoint))
    });

    let proxied_count = connections
        .iter()
        .filter(|c| c.category == ConnectionCategory::Proxied)
        .count();
    let tun_count = connections
        .iter()
        .filter(|c| c.category == ConnectionCategory::TunFakeIp)
        .count();
    let direct_count = connections
        .iter()
        .filter(|c| c.category == ConnectionCategory::PublicDirect)
        .count();

    ProxySnapshot {
        proxy_host: settings.proxy_host.clone(),
        resolved_port,
        port_source,
        is_proxy_listening: is_listening,
        clash_mode,
        is_clash_running: clash_running,
        system_proxy_enabled: sys_proxy_enabled,
        system_proxy_server: sys_proxy_server,
        tun_enabled,
        tun_adapter_text,
        antigravity_proc_count: agy_count,
        language_server_proc_count: ls_count,
        processes: procs,
        proxied_count,
        tun_count,
        direct_count,
        connections,
        captured_at: seen_at,
    }
}

pub fn start_clash_verge(settings: &ProxySettings) -> (u16, Vec<String>) {
    let mut logs = Vec::new();
    let path = Path::new(settings.clash_verge_path.trim());
    if settings.clash_verge_path.trim().is_empty() || !path.is_file() {
        logs.push(format!(
            "[X] 找不到 Clash Verge 可执行文件: {}",
            settings.clash_verge_path
        ));
        return (0, logs);
    }

    let (cfg_port, _) = read_clash_verge_config(&settings.clash_verge_path);
    let expected_port = if !settings.auto_detect_port && settings.custom_port > 0 {
        settings.custom_port
    } else if cfg_port > 0 {
        cfg_port
    } else {
        7897
    };

    logs.push(format!(
        "[..] 正在启动/唤醒 Clash Verge: {}",
        settings.clash_verge_path
    ));

    let mut cmd = Command::new(path);
    if let Some(dir) = path.parent() {
        cmd.current_dir(dir);
    }
    if let Err(err) = cmd.spawn() {
        logs.push(format!("[X] 启动 Clash Verge 失败: {err}"));
        return (0, logs);
    }

    for _ in 0..40 {
        thread::sleep(Duration::from_millis(500));
        if test_tcp_port(&settings.proxy_host, expected_port, 350) {
            logs.push(format!(
                "[OK] Clash Verge 本地代理端口 {}:{} 已就绪。",
                settings.proxy_host, expected_port
            ));
            return (expected_port, logs);
        }
        let (p, _, listening, _) = resolve_port(settings);
        if listening {
            logs.push(format!(
                "[OK] 检测到本地代理端口 {}:{} 已就绪。",
                settings.proxy_host, p
            ));
            return (p, logs);
        }
    }

    logs.push("[!] 等待 Clash Verge 本地代理端口就绪超时（20 秒）。".to_string());
    (0, logs)
}

pub fn flush_dns_cache() -> Vec<String> {
    let mut logs = vec!["[..] 正在执行 ipconfig /flushdns ...".to_string()];
    let mut cmd = Command::new("ipconfig.exe");
    cmd.arg("/flushdns");
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);

    match cmd.output() {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let stderr = String::from_utf8_lossy(&out.stderr);
            let combined = if stderr.trim().is_empty() {
                stdout.to_string()
            } else {
                format!("{stdout}\n{stderr}")
            };
            for line in combined.lines() {
                let t = line.trim();
                if !t.is_empty() {
                    logs.push(format!("    {t}"));
                }
            }
            logs.push(format!(
                "[OK] DNS 刷新完成 (退出码 {})。",
                out.status.code().unwrap_or(-1)
            ));
        }
        Err(err) => {
            logs.push(format!("[X] 无法执行 ipconfig.exe: {err}"));
        }
    }
    logs
}

pub fn stop_antigravity_processes(antigravity_path: &str) -> (bool, Vec<String>) {
    let mut logs = Vec::new();
    let (procs, _) = win_native::scan_antigravity_and_clash(antigravity_path);
    if procs.is_empty() {
        logs.push("[i] 当前没有正在运行的 Antigravity 进程。".to_string());
        return (true, logs);
    }

    logs.push(format!(
        "[..] 正在关闭 {} 个 Antigravity 相关进程...",
        procs.len()
    ));
    for p in &procs {
        if let Err(err) = win_native::terminate_pid(p.pid) {
            logs.push(format!("    忽略关闭 {}(PID {}): {err}", p.name, p.pid));
        }
    }

    for _ in 0..16 {
        thread::sleep(Duration::from_millis(300));
        let (rem, _) = win_native::scan_antigravity_and_clash(antigravity_path);
        if rem.is_empty() {
            logs.push("[OK] 所有旧 Antigravity 进程已退出。".to_string());
            return (true, logs);
        }
    }

    let (remaining, _) = win_native::scan_antigravity_and_clash(antigravity_path);
    logs.push(format!("[!] 仍有 {} 个进程未完全退出。", remaining.len()));
    (remaining.is_empty(), logs)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AntigravityRuntimeStatus {
    pub exe_valid: bool,
    pub antigravity_path: String,
    pub running_count: usize,
    pub proxy_listening: bool,
    pub proxy_endpoint: String,
    pub auto_start_clash: bool,
}

pub fn get_antigravity_runtime_status(settings: &ProxySettings) -> AntigravityRuntimeStatus {
    let exe_valid = Path::new(settings.antigravity_path.trim()).is_file();
    let (procs, _) = win_native::scan_antigravity_and_clash(&settings.antigravity_path);
    let (port, _, proxy_listening, _) = resolve_port(settings);
    AntigravityRuntimeStatus {
        exe_valid,
        antigravity_path: settings.antigravity_path.clone(),
        running_count: procs.len(),
        proxy_listening,
        proxy_endpoint: format!("{}:{}", settings.proxy_host, port),
        auto_start_clash: settings.auto_start_clash,
    }
}

fn normalize_custom_bypass_entry(raw: &str) -> Option<String> {
    let mut s = raw.trim().to_ascii_lowercase();
    if s.is_empty() {
        return None;
    }
    if let Some(rest) = s
        .strip_prefix("https://")
        .or_else(|| s.strip_prefix("http://"))
    {
        s = rest.to_string();
    }
    if let Some(idx) = s.find(['/', '?', '#']) {
        // 保留 CIDR 网段中的 /prefix（例如 10.20.0.0/16）
        let is_cidr = s
            .split_once('/')
            .map(|(ip_part, prefix)| {
                ip_part.parse::<IpAddr>().is_ok() && prefix.parse::<u8>().is_ok()
            })
            .unwrap_or(false);
        if !is_cidr {
            s.truncate(idx);
        }
    }
    if !s.contains('/') && !s.starts_with('[') && s.matches(':').count() == 1 {
        if let Some((host, port)) = s.split_once(':') {
            if port.chars().all(|c| c.is_ascii_digit()) {
                s = host.to_string();
            }
        }
    }
    let s = s.trim_start_matches("*.").trim_start_matches('.').trim();
    if s.is_empty()
        || !s
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':' | '/' | '[' | ']'))
    {
        return None;
    }
    Some(s.to_string())
}

/// 生成 Chromium `--proxy-bypass-list` 与通用环境变量 `NO_PROXY` 规则列表。
///
/// - Chromium 支持 `domain;*.domain`、`*.cn`、CIDR 与 `<local>`；
/// - Go / Node (`NODE_USE_ENV_PROXY=1`) / curl / Python 的 `NO_PROXY` 使用 `.domain` 后缀与 CIDR（不带 `*`）。
pub fn build_bypass_lists(settings: &ProxySettings) -> (String, String, usize) {
    let mut chromium: Vec<String> = vec![
        "localhost".to_string(),
        "127.0.0.1".to_string(),
        "[::1]".to_string(),
        "<local>".to_string(),
    ];
    let mut no_proxy: Vec<String> = vec![
        "localhost".to_string(),
        "127.0.0.1".to_string(),
        "::1".to_string(),
        "[::1]".to_string(),
    ];

    let mut seen_domains: HashSet<String> = HashSet::new();

    if settings.bypass_china_and_lan {
        for cidr in LAN_BYPASS_CIDRS {
            chromium.push((*cidr).to_string());
            no_proxy.push((*cidr).to_string());
        }
        for suffix in ["local", "lan", "cn"] {
            chromium.push(format!("*.{suffix}"));
            no_proxy.push(format!(".{suffix}"));
            seen_domains.insert(suffix.to_string());
        }
        for domain in CHINA_DIRECT_DOMAINS {
            if seen_domains.insert((*domain).to_string()) {
                chromium.push((*domain).to_string());
                chromium.push(format!("*.{domain}"));
                no_proxy.push(format!(".{domain}"));
            }
        }
    }

    let mut custom_count = 0usize;
    for token in settings
        .custom_bypass_domains
        .split(|c: char| matches!(c, ',' | ';' | '\n' | '\r' | '\t' | ' '))
    {
        let Some(clean) = normalize_custom_bypass_entry(token) else {
            continue;
        };
        if !seen_domains.insert(clean.clone()) {
            continue;
        }
        custom_count += 1;
        let is_ip_or_cidr = clean.contains('/') || clean.parse::<IpAddr>().is_ok() || clean.starts_with('[');
        if is_ip_or_cidr {
            chromium.push(clean.clone());
            no_proxy.push(clean);
        } else if !clean.contains('.') {
            chromium.push(clean.clone());
            chromium.push(format!("*.{clean}"));
            no_proxy.push(format!(".{clean}"));
        } else {
            chromium.push(clean.clone());
            chromium.push(format!("*.{clean}"));
            no_proxy.push(format!(".{clean}"));
        }
    }

    (chromium.join(";"), no_proxy.join(","), custom_count)
}

fn apply_proxy_env_and_args(cmd: &mut Command, settings: &ProxySettings, proxy_url: &str) {
    let (chromium_bypass, env_no_proxy, _) = build_bypass_lists(settings);
    cmd.arg(format!("--proxy-server={proxy_url}"));
    cmd.arg(format!("--proxy-bypass-list={chromium_bypass}"));
    for key in [
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "http_proxy",
        "https_proxy",
        "all_proxy",
    ] {
        cmd.env(key, proxy_url);
    }
    for key in ["NO_PROXY", "no_proxy"] {
        cmd.env(key, &env_no_proxy);
    }
    cmd.env("NODE_USE_ENV_PROXY", "1");
}

/// 账号切换后重启 Antigravity：调用前进程应已停止。
///
/// 与 `launch_isolated_antigravity` 的区别：若代理端口未监听且开启了自动拉起 Clash，
/// 会先尝试拉起 Clash Verge；若仍未就绪也不阻塞，退化为普通启动并在日志里说明。
pub fn launch_antigravity_after_stop(settings: &ProxySettings) -> (bool, Vec<String>, Option<String>) {
    let mut logs = Vec::new();
    let agy_path = Path::new(settings.antigravity_path.trim());
    if !agy_path.is_file() {
        let msg = format!("找不到 Antigravity 可执行文件: {}", settings.antigravity_path);
        logs.push(format!("[X] {msg}"));
        return (false, logs, Some(msg));
    }

    let (mut port, _source, mut is_listening, _) = resolve_port(settings);
    if !is_listening
        && settings.auto_start_clash
        && Path::new(settings.clash_verge_path.trim()).is_file()
    {
        let (started_port, clash_logs) = start_clash_verge(settings);
        logs.extend(clash_logs);
        if started_port > 0 {
            port = started_port;
            is_listening = true;
        }
    }

    let proxy_url = if is_listening {
        Some(format!("http://{}:{}", settings.proxy_host, port))
    } else {
        None
    };

    let mut cmd = Command::new(agy_path);
    if let Some(dir) = agy_path.parent() {
        cmd.current_dir(dir);
    }

    match &proxy_url {
        Some(url) => {
            apply_proxy_env_and_args(&mut cmd, settings, url);
            let bypass_note = if settings.bypass_china_and_lan {
                "（已启用国内/内网直连绕过）"
            } else {
                ""
            };
            logs.push(format!(
                "[OK] Antigravity 已以隔离代理方式启动 -> {url}{bypass_note}"
            ));
        }
        None => {
            logs.push(
                "[i] 本地代理端口未监听，本次以普通方式启动 Antigravity（不经过本地代理）。".to_string(),
            );
        }
    }

    if settings.disable_quic {
        cmd.arg("--disable-quic");
    }
    if !settings.workspace_path.trim().is_empty() {
        cmd.arg(settings.workspace_path.trim());
    }

    match cmd.spawn() {
        Ok(child) => {
            logs.push(format!("[OK] Antigravity 已启动 (PID {})", child.id()));
            (true, logs, None)
        }
        Err(err) => {
            let msg = format!("启动 Antigravity 失败: {err}");
            logs.push(format!("[X] {msg}"));
            (false, logs, Some(msg))
        }
    }
}

pub fn build_dry_run_preview(settings: &ProxySettings, proxy_port: u16) -> String {
    let proxy_url = format!("http://{}:{}", settings.proxy_host, proxy_port);
    let (chromium_bypass, env_no_proxy, custom_count) = build_bypass_lists(settings);
    let mut args = vec![
        format!("--proxy-server={proxy_url}"),
        format!("--proxy-bypass-list={chromium_bypass}"),
    ];
    if settings.disable_quic {
        args.push("--disable-quic".to_string());
    }
    if !settings.workspace_path.trim().is_empty() {
        args.push(format!("\"{}\"", settings.workspace_path.trim()));
    }

    let bypass_summary = if settings.bypass_china_and_lan {
        format!(
            "开启（含 LAN 网段 + .cn + {} 个内置国内常用域名{}）",
            CHINA_DIRECT_DOMAINS.len(),
            if custom_count > 0 {
                format!(" + {custom_count} 条自定义规则")
            } else {
                String::new()
            }
        )
    } else if custom_count > 0 {
        format!("仅自定义（{custom_count} 条规则）")
    } else {
        "关闭（仅绕过本机回环地址）".to_string()
    };

    [
        "[DryRun 启动参数预览]".to_string(),
        format!("  目标程序   : {}", settings.antigravity_path),
        format!("  国内/内网直连: {bypass_summary}"),
        format!("  启动参数   : {}", args.join(" ")),
        format!("  子进程环境 : HTTP_PROXY = HTTPS_PROXY = ALL_PROXY = {proxy_url}"),
        format!("  子进程环境 : NO_PROXY = {env_no_proxy}"),
        "  子进程环境 : NODE_USE_ENV_PROXY = 1".to_string(),
        "  系统影响   : 不改动 Windows 注册表代理，不改动 Clash Verge 配置，不开 TUN。".to_string(),
    ]
    .join("\n")
}

pub fn launch_isolated_antigravity(settings: &ProxySettings, force_restart: bool) -> ActionResponse {
    let mut logs = Vec::new();
    let agy_path = Path::new(settings.antigravity_path.trim());
    if !agy_path.is_file() {
        let msg = format!("找不到 Antigravity 可执行文件: {}", settings.antigravity_path);
        logs.push(format!("[X] {msg}"));
        return ActionResponse {
            ok: false,
            need_confirm: false,
            process_count: 0,
            resolved_port: 0,
            logs,
            error: Some(msg),
        };
    }

    let (mut target_port, _, is_listening, _) = resolve_port(settings);
    if !is_listening {
        if settings.auto_start_clash && Path::new(settings.clash_verge_path.trim()).is_file() {
            let (started_port, clash_logs) = start_clash_verge(settings);
            logs.extend(clash_logs);
            if started_port == 0 {
                return ActionResponse {
                    ok: false,
                    need_confirm: false,
                    process_count: 0,
                    resolved_port: target_port,
                    logs,
                    error: Some("自动启动 Clash Verge 后端口仍未就绪，请确认 Clash Verge 正常运行。".to_string()),
                };
            }
            target_port = started_port;
        } else {
            let msg = format!(
                "本地代理端口 {}:{} 未监听，请先启动 Clash Verge。",
                settings.proxy_host, target_port
            );
            logs.push(format!("[X] {msg}"));
            return ActionResponse {
                ok: false,
                need_confirm: false,
                process_count: 0,
                resolved_port: target_port,
                logs,
                error: Some(msg),
            };
        }
    }

    let (existing, _) = win_native::scan_antigravity_and_clash(&settings.antigravity_path);
    if !existing.is_empty() {
        if !force_restart {
            return ActionResponse {
                ok: false,
                need_confirm: true,
                process_count: existing.len(),
                resolved_port: target_port,
                logs,
                error: None,
            };
        }
        let (stopped, stop_logs) = stop_antigravity_processes(&settings.antigravity_path);
        logs.extend(stop_logs);
        if !stopped {
            return ActionResponse {
                ok: false,
                need_confirm: false,
                process_count: existing.len(),
                resolved_port: target_port,
                logs,
                error: Some("部分旧进程未能退出，请手动完全退出 Antigravity 后再试。".to_string()),
            };
        }
    }

    let proxy_url = format!("http://{}:{}", settings.proxy_host, target_port);
    let mut cmd = Command::new(agy_path);
    if let Some(dir) = agy_path.parent() {
        cmd.current_dir(dir);
    }
    apply_proxy_env_and_args(&mut cmd, settings, &proxy_url);
    if settings.disable_quic {
        cmd.arg("--disable-quic");
    }
    if !settings.workspace_path.trim().is_empty() {
        cmd.arg(settings.workspace_path.trim());
    }

    match cmd.spawn() {
        Ok(child) => {
            let bypass_note = if settings.bypass_china_and_lan {
                "（已启用国内/内网直连绕过）"
            } else {
                ""
            };
            logs.push(format!(
                "[OK] Antigravity 已以隔离代理方式启动 (PID {}) -> {}{}",
                child.id(),
                proxy_url,
                bypass_note
            ));
            ActionResponse {
                ok: true,
                need_confirm: false,
                process_count: 1,
                resolved_port: target_port,
                logs,
                error: None,
            }
        }
        Err(err) => {
            let msg = format!("启动 Antigravity 失败: {err}");
            logs.push(format!("[X] {msg}"));
            ActionResponse {
                ok: false,
                need_confirm: false,
                process_count: 0,
                resolved_port: target_port,
                logs,
                error: Some(msg),
            }
        }
    }
}

pub fn verify_report_path() -> PathBuf {
    std::env::temp_dir().join("antigravity-launcher-verify.txt")
}

pub fn run_cumulative_audit<F>(
    settings: &ProxySettings,
    seconds: u32,
    cancel_flag: &Arc<AtomicBool>,
    mut on_tick: F,
) -> AuditResult
where
    F: FnMut(u32, u32, &ProxySnapshot),
{
    let secs = seconds.max(1);
    let total_ticks = ((secs as f64) / 0.8).ceil().max(1.0) as u32;
    let mut seen_sockets: HashMap<String, TcpConnectionItem> = HashMap::new();
    let mut latest = build_snapshot(settings);
    let mut cancelled = false;

    for tick in 1..=total_ticks {
        if cancel_flag.load(Ordering::SeqCst) {
            cancelled = true;
            break;
        }
        latest = build_snapshot(settings);
        for item in &latest.connections {
            seen_sockets.insert(item.key.clone(), item.clone());
        }
        let elapsed = ((tick as f64 * 0.8).round() as u32).min(secs);
        on_tick(elapsed, secs, &latest);

        if tick < total_ticks {
            for _ in 0..8 {
                if cancel_flag.load(Ordering::SeqCst) {
                    cancelled = true;
                    break;
                }
                thread::sleep(Duration::from_millis(100));
            }
            if cancelled {
                break;
            }
        }
    }

    let mut cumulative: Vec<TcpConnectionItem> = seen_sockets.into_values().collect();
    cumulative.sort_by(|a, b| {
        a.category
            .sort_order()
            .cmp(&b.category.sort_order())
            .then_with(|| a.process_name.to_lowercase().cmp(&b.process_name.to_lowercase()))
            .then_with(|| a.remote_endpoint.cmp(&b.remote_endpoint))
    });

    let proxied = cumulative
        .iter()
        .filter(|c| c.category == ConnectionCategory::Proxied)
        .count();
    let tun = cumulative
        .iter()
        .filter(|c| c.category == ConnectionCategory::TunFakeIp)
        .count();
    let direct = cumulative
        .iter()
        .filter(|c| c.category == ConnectionCategory::PublicDirect)
        .count();
    let ls_direct = cumulative
        .iter()
        .filter(|c| {
            c.category == ConnectionCategory::PublicDirect
                && c.process_name.to_lowercase().starts_with("language_server")
        })
        .count();
    let bypass_enabled =
        settings.bypass_china_and_lan || !settings.custom_bypass_domains.trim().is_empty();

    let passed = if tun > 0 || ls_direct > 0 || (!bypass_enabled && direct > 0) {
        Some(false)
    } else if proxied > 0 {
        Some(true)
    } else {
        None
    };

    let now_str = Local::now().format("%Y-%m-%d %H:%M:%S");
    let mut lines = vec![
        format!("[{now_str}] Antigravity 隔离代理检查 ({secs} 秒累计)"),
        format!(
            "  代理端口      : http://{}:{} ({}, Clash 模式: {})",
            settings.proxy_host, latest.resolved_port, latest.port_source, latest.clash_mode
        ),
        format!(
            "  国内/内网直连 : {}",
            if settings.bypass_china_and_lan {
                "已开启（命中直连规则的站点不走代理）"
            } else {
                "关闭"
            }
        ),
        format!(
            "  进程数        : Antigravity x{}, language_server x{}",
            latest.antigravity_proc_count, latest.language_server_proc_count
        ),
    ];

    if latest.system_proxy_enabled {
        lines.push(format!(
            "  [!] Windows 系统代理开启中: {}",
            latest.system_proxy_server
        ));
    }
    if latest.tun_enabled {
        lines.push(format!(
            "  [!] TUN 虚拟网卡开启中: {}",
            latest.tun_adapter_text
        ));
    }
    if latest.clash_mode.eq_ignore_ascii_case("global") {
        lines.push(
            "  [!] Clash 当前处于 Global 模式，未命中直连列表的国内站点可能走海外节点触发风控，建议切为 Rule 模式。"
                .to_string(),
        );
    }
    lines.push(format!("  走代理的连接  : {proxied} 条"));
    lines.push(format!("  TUN 接管连接  : {tun} 条"));
    lines.push(format!("  公网直连连接  : {direct} 条"));

    if tun > 0 {
        lines.push(
            "  [X] 检测到 Clash TUN Fake-IP (198.18.x.x) 连接，尚未纯隔离到本地代理端口。"
                .to_string(),
        );
        for item in cumulative
            .iter()
            .filter(|c| c.category == ConnectionCategory::TunFakeIp)
            .take(8)
        {
            lines.push(format!(
                "      - {} ({}, PID {})",
                item.remote_endpoint, item.process_name, item.pid
            ));
        }
    }
    if direct > 0 {
        if bypass_enabled && ls_direct == 0 {
            lines.push(
                "  [i] 检测到非 language_server 进程存在公网直连（已开启国内/内网直连绕过，访问国内站点时属正常现象）："
                    .to_string(),
            );
        } else {
            lines.push(
                "  [X] 检测到公网直连，请关闭旧实例后用本面板重新启动 Antigravity。".to_string(),
            );
        }
        for item in cumulative
            .iter()
            .filter(|c| c.category == ConnectionCategory::PublicDirect)
            .take(8)
        {
            lines.push(format!(
                "      - {} ({}, PID {})",
                item.remote_endpoint, item.process_name, item.pid
            ));
        }
    }
    if passed == Some(true) {
        if bypass_enabled && direct > 0 {
            lines.push(
                "  [OK] 核心代理流量均落在本地代理端口，且已按配置允许国内/内网直连绕过。"
                    .to_string(),
            );
        } else {
            lines.push(
                "  [OK] 观察期内无 TUN 接管和公网直连，出网连接均落在本地代理端口上。".to_string(),
            );
        }
    } else if passed.is_none() && !cancelled {
        lines.push(
            "  [..] 观察期内应用未产生外部连接（空闲）。请在 Antigravity 中发一条消息后再验收。"
                .to_string(),
        );
    }

    let report_text = lines.join("\n");
    let report_path = verify_report_path();
    if !cancelled {
        let bom_content = format!("\u{FEFF}{report_text}\r\n");
        let _ = fs::write(&report_path, bom_content.as_bytes());
    }

    AuditResult {
        cancelled,
        latest_snapshot: latest,
        cumulative_proxied_count: proxied,
        cumulative_tun_count: tun,
        cumulative_direct_count: direct,
        cumulative_connections: cumulative,
        report_text,
        report_path: report_path.to_string_lossy().to_string(),
        passed,
    }
}

pub fn open_verify_report() -> Result<String, String> {
    let path = verify_report_path();
    if !path.is_file() {
        return Err(format!(
            "尚未生成验收报告: {}，请先运行一次「验收 20s」。",
            path.to_string_lossy()
        ));
    }
    #[cfg(windows)]
    {
        Command::new("notepad.exe")
            .arg(&path)
            .spawn()
            .map_err(|e| format!("无法打开报告: {e}"))?;
    }
    Ok(path.to_string_lossy().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_remote_endpoints_accurately() {
        assert_eq!(
            classify_remote("127.0.0.1", 7897, "127.0.0.1", 7897),
            ConnectionCategory::Proxied
        );
        assert_eq!(
            classify_remote("198.18.0.12", 443, "127.0.0.1", 7897),
            ConnectionCategory::TunFakeIp
        );
        assert_eq!(
            classify_remote("127.0.0.1", 51234, "127.0.0.1", 7897),
            ConnectionCategory::LocalIpc
        );
        assert_eq!(
            classify_remote("192.168.1.25", 443, "127.0.0.1", 7897),
            ConnectionCategory::LocalIpc
        );
        assert_eq!(
            classify_remote("10.0.0.8", 8080, "127.0.0.1", 7897),
            ConnectionCategory::LocalIpc
        );
        assert_eq!(
            classify_remote("142.250.190.78", 443, "127.0.0.1", 7897),
            ConnectionCategory::PublicDirect
        );
    }

    #[test]
    fn builds_china_and_custom_bypass_lists_in_dual_formats() {
        let mut s = ProxySettings::default();
        s.bypass_china_and_lan = true;
        s.custom_bypass_domains = "https://www.MyCorp.com/api, 10.20.0.0/16\n*.example.org".to_string();
        let (chromium, no_proxy, custom_count) = build_bypass_lists(&s);
        assert_eq!(custom_count, 3);
        assert!(chromium.contains("*.cn"));
        assert!(chromium.contains("baidu.com;*.baidu.com"));
        assert!(chromium.contains("www.mycorp.com;*.www.mycorp.com"));
        assert!(chromium.contains("example.org;*.example.org"));
        assert!(chromium.contains("10.20.0.0/16"));

        assert!(no_proxy.contains(".cn"));
        assert!(no_proxy.contains(".baidu.com"));
        assert!(no_proxy.contains(".www.mycorp.com"));
        assert!(no_proxy.contains(".example.org"));
        assert!(no_proxy.contains("10.20.0.0/16"));
        assert!(!no_proxy.contains('*'), "NO_PROXY must not contain * wildcards");

        s.bypass_china_and_lan = false;
        s.custom_bypass_domains = String::new();
        let (chromium_off, no_proxy_off, count_off) = build_bypass_lists(&s);
        assert_eq!(count_off, 0);
        assert!(!chromium_off.contains("*.cn"));
        assert!(!no_proxy_off.contains(".cn"));
    }

    #[test]
    fn parses_clash_yaml_port_and_mode() {
        let yaml = "verge_mixed_port: 7897\nmode: global\n";
        let (port, mode) = parse_clash_yaml_port_and_mode(yaml);
        assert_eq!(port, 7897);
        assert_eq!(mode.as_deref(), Some("global"));
    }

    #[test]
    fn builds_dry_run_preview_with_env_and_args() {
        let s = ProxySettings::default();
        let preview = build_dry_run_preview(&s, 7897);
        assert!(preview.contains("--proxy-server=http://127.0.0.1:7897"));
        assert!(preview.contains("*.cn"));
        assert!(preview.contains("NODE_USE_ENV_PROXY = 1"));
    }
}
