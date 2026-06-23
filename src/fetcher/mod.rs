//! HTTP 拉取层（对应原 JS index.js）。
//!
//! 流程（MIGRATION.md §2）：
//!   1. GET `{url}{suffix}/v3/api-docs/swagger-config` → SwaggerConfig
//!   2. 取其中 `urls`（或单个 `url`），逐个 GET `{url}{doc_url}` → ApiDoc
//!   3. 返回所有 ApiDoc；若一个文档地址都没有则报错「没有地址」

use anyhow::{Context, Result, bail};
use reqwest::blocking::Client;

use crate::config::Config;
use crate::openapi::{ApiDoc, SwaggerConfig};

/// 拉取并解析全部分组的 api-docs。
pub fn fetch_all(config: &Config) -> Result<Vec<ApiDoc>> {
    // 显式设置 User-Agent：部分网关/WAF 会拦截无 UA 或库默认指纹的请求。
    let client = Client::builder()
        .user_agent(concat!("swagger-api-rs/", env!("CARGO_PKG_VERSION")))
        .build()
        .context("初始化 HTTP client 失败")?;

    let config_url = config.swagger_config_url();
    println!("拉取 swagger-config: {config_url}");
    let swagger_config: SwaggerConfig = get_json(&client, &config_url).with_context(|| {
        format!("拉取 swagger-config 失败: {config_url}\n{}", proxy_hint(&config.url))
    })?;

    let doc_urls = swagger_config.doc_urls();
    if doc_urls.is_empty() {
        bail!("没有地址"); // 对齐原版 index.js 的报错文案
    }

    let mut docs = Vec::with_capacity(doc_urls.len());
    for doc_url in doc_urls {
        // 对齐原版 getOhterUrls：完整地址 = Base_url + url
        let full = format!("{}{}", config.url, doc_url);
        println!("加载配置文件 {full}");
        let doc: ApiDoc = get_json(&client, &full)
            .with_context(|| format!("拉取 api-docs 失败: {full}\n{}", proxy_hint(&config.url)))?;
        docs.push(doc);
    }

    Ok(docs)
}

/// 拉取失败时的友好提示：多数失败（连接错误 / 502 等）是本机代理拦截内网域名所致。
fn proxy_hint(url: &str) -> String {
    let host = host_of(url);
    format!(
        "提示：若处于代理后（如本机 Clash/VPN），代理可能拦截了该域名（常见表现为连接失败或 502）。\n\
         可绕过代理重试：  NO_PROXY={host} swagger\n\
         或把 {host} 加入系统/代理的「绕过」名单。"
    )
}

/// 从 URL 提取主机名（去掉 scheme、路径与端口）。
fn host_of(url: &str) -> String {
    let after = url.split("://").nth(1).unwrap_or(url);
    let hostport = after.split('/').next().unwrap_or(after);
    hostport.split(':').next().unwrap_or(hostport).to_string()
}

/// GET 一个 URL 并反序列化为目标类型。
fn get_json<T: serde::de::DeserializeOwned>(client: &Client, url: &str) -> Result<T> {
    let resp = client
        .get(url)
        .send()
        .with_context(|| format!("请求失败: {url}"))?
        .error_for_status()
        .with_context(|| format!("响应状态错误: {url}"))?;
    let text = resp.text().with_context(|| format!("读取响应体失败: {url}"))?;
    let value = serde_json::from_str(&text)
        .with_context(|| format!("响应 JSON 解析失败: {url}"))?;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::host_of;

    #[test]
    fn extracts_host() {
        assert_eq!(host_of("http://ywtg.host/api-admin"), "ywtg.host");
        assert_eq!(host_of("https://a.b.com:8080/x/y"), "a.b.com");
        assert_eq!(host_of("http://127.0.0.1:8799"), "127.0.0.1");
    }
}
