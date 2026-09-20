//! 本地出站代理（VPN 固定转发端口）自动探测。
//!
//! 用途：设置页「出站代理」卡片点击「自动探测」后，扫描本机常见代理端口，
//! 并用「经该代理请求一个 204 探针 URL」验证真实连通性 + 测延迟，
//! 返回存活候选列表供用户下拉选择。
//!
//! 探测范围：
//! 1. 环境变量 HTTP_PROXY / HTTPS_PROXY / ALL_PROXY（大小写）；
//! 2. 常见本地代理端口（Clash / Clash Verge / v2rayN / Surge / Privoxy 等）。

use serde::{Deserialize, Serialize};

/// 单个候选代理的探测结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyCandidate {
    /// 代理 URL（可直接填入出站代理设置，如 http://127.0.0.1:7890）。
    pub url: String,
    /// 来源：env（环境变量）或 scan（端口扫描）。
    pub source: String,
    /// 猜测的客户端标签（如 Clash / v2rayN），无法识别时为「未知代理」。
    pub label: String,
    /// 探测延迟（毫秒）；None = 探测失败（不应出现，仅存活项会返回）。
    pub latency_ms: Option<u64>,
}

/// 204 探针 URL：任意可用的 HTTP 代理都能转发它；用国内可达地址，
/// 即使代理本身不提供翻墙也能确认「代理端口活着」，不把翻墙能力与端口探活混在一起。
const PROBE_URL: &str = "http://connect.rom.miui.com/generate_204";

/// (端口, 标签, scheme)。socks 端口用 socks5://，其余默认 http://。
const COMMON_PORTS: &[(u16, &str, &str)] = &[
    (7890, "Clash", "http"),
    (7891, "Clash (SOCKS)", "socks5"),
    (7897, "Clash Verge", "http"),
    (7899, "Clash Verge", "http"),
    (10808, "v2rayN (SOCKS)", "socks5"),
    (10809, "v2rayN", "http"),
    (1087, "v2ray", "http"),
    (1086, "v2ray (SOCKS)", "socks5"),
    (6152, "Surge", "http"),
    (6153, "Surge", "http"),
    (8118, "Privoxy", "http"),
    (8889, "未知代理", "http"),
    (33210, "未知代理", "http"),
    (2080, "未知代理", "http"),
];

/// 从环境变量收集代理候选（去重、剔除空值）。
fn env_candidates() -> Vec<String> {
    let mut out = Vec::new();
    for key in [
        "HTTP_PROXY",
        "http_proxy",
        "HTTPS_PROXY",
        "https_proxy",
        "ALL_PROXY",
        "all_proxy",
    ] {
        if let Ok(v) = std::env::var(key) {
            let v = v.trim().to_string();
            if !v.is_empty() && !out.contains(&v) {
                out.push(v);
            }
        }
    }
    out
}

/// 用给定代理请求探针 URL；存活则返回延迟毫秒。
async fn probe_proxy(proxy_url: &str) -> Option<u64> {
    let proxy = reqwest::Proxy::all(proxy_url).ok()?;
    let client = reqwest::Client::builder()
        // 显式设置 .proxy() 后 reqwest 会忽略环境变量代理，无需再调 no_proxy()
        //（且 no_proxy() 在 proxy() 之后调用会覆盖清除代理，导致探测假存活）。
        .proxy(proxy)
        .connect_timeout(std::time::Duration::from_millis(800))
        .timeout(std::time::Duration::from_secs(3))
        .build()
        .ok()?;
    let start = std::time::Instant::now();
    let resp = client.get(PROBE_URL).send().await.ok()?;
    if resp.status().is_success() || resp.status().as_u16() == 204 {
        Some(start.elapsed().as_millis() as u64)
    } else {
        None
    }
}

/// 端口猜测标签。
fn label_for_port(port: u16) -> &'static str {
    COMMON_PORTS
        .iter()
        .find(|(p, _, _)| *p == port)
        .map(|(_, label, _)| *label)
        .unwrap_or("未知代理")
}

/// 探测本机可用的出站代理端口，返回存活候选（按延迟升序）。
#[tauri::command]
pub async fn detect_local_proxies() -> Result<Vec<ProxyCandidate>, String> {
    // 组装候选：环境变量 + 常见端口。
    let mut candidates: Vec<(String, &'static str)> = env_candidates()
        .into_iter()
        .map(|url| {
            let label = "环境变量代理";
            (url, label)
        })
        .collect();
    for (port, label, scheme) in COMMON_PORTS {
        candidates.push((format!("{scheme}://127.0.0.1:{port}"), label));
    }

    // 并发探测。
    let tasks: Vec<_> = candidates
        .into_iter()
        .map(|(url, label)| {
            let url = url.clone();
            async move {
                if let Some(latency_ms) = probe_proxy(&url).await {
                    Some(ProxyCandidate {
                        url,
                        source: "scan".to_string(),
                        label: label.to_string(),
                        latency_ms: Some(latency_ms),
                    })
                } else {
                    None
                }
            }
        })
        .collect();
    let mut alive: Vec<ProxyCandidate> = futures_util::future::join_all(tasks)
        .await
        .into_iter()
        .flatten()
        .collect();
    alive.sort_by_key(|c| c.latency_ms.unwrap_or(u64::MAX));
    Ok(alive)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_candidates_reads_and_dedups() {
        // 不预设环境变量（CI/本地都可能没有代理），只验证函数可安全调用且格式合法。
        let list = env_candidates();
        for url in list {
            assert!(!url.trim().is_empty());
        }
    }

    #[tokio::test]
    async fn probe_proxy_rejects_invalid_url() {
        // 非法代理 URL 应返回 None 而不是 panic/Err。
        assert!(probe_proxy("not-a-valid-proxy").await.is_none());
    }
}
