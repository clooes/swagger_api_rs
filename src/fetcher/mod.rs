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
    // Swagger 服务通常位于内网。reqwest 默认读取 macOS 系统代理，但不会应用
    // ClashX/macOS 中配置的忽略主机列表，导致内网域名被送到代理后返回 502。
    // 因此该客户端始终直连，不受系统代理影响。
    let client = Client::builder()
        .no_proxy()
        .user_agent(concat!("swagger-api-rs/", env!("CARGO_PKG_VERSION")))
        .build()
        .context("初始化 HTTP client 失败")?;

    let config_url = config.swagger_config_url();
    println!("拉取 swagger-config: {config_url}");
    let swagger_config: SwaggerConfig = get_json(&client, &config_url)
        .with_context(|| format!("拉取 swagger-config 失败: {config_url}"))?;

    let doc_urls = swagger_config.doc_urls();
    if doc_urls.is_empty() {
        bail!("没有地址"); // 对齐原版 index.js 的报错文案
    }

    let mut docs = Vec::with_capacity(doc_urls.len());
    for doc_url in doc_urls {
        // 对齐原版 getOhterUrls：完整地址 = Base_url + url
        let full = format!("{}{}", config.url, doc_url);
        println!("加载配置文件 {full}");
        let doc: ApiDoc =
            get_json(&client, &full).with_context(|| format!("拉取 api-docs 失败: {full}"))?;
        docs.push(doc);
    }

    Ok(docs)
}

/// GET 一个 URL 并反序列化为目标类型。
fn get_json<T: serde::de::DeserializeOwned>(client: &Client, url: &str) -> Result<T> {
    let resp = client
        .get(url)
        .send()
        .with_context(|| format!("请求失败: {url}"))?
        .error_for_status()
        .with_context(|| format!("响应状态错误: {url}"))?;
    let text = resp
        .text()
        .with_context(|| format!("读取响应体失败: {url}"))?;
    let value =
        serde_json::from_str(&text).with_context(|| format!("响应 JSON 解析失败: {url}"))?;
    Ok(value)
}
