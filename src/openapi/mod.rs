//! OpenAPI / Swagger 原始 spec 的反序列化模型（对应 MIGRATION.md §4）。
//!
//! 这里只做「忠实反序列化」，不做任何类型推导/映射 —— 所有语义解析都在 `lower/`。
//! 用 `IndexMap` 保留 paths / schemas / properties 的插入顺序，
//! 以匹配原 JS `for..in` 的遍历顺序，保证输出稳定。

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// `/v3/api-docs/swagger-config` 响应：可能是分组列表 `urls`，或单个 `url`（§2）。
#[derive(Debug, Clone, Deserialize)]
pub struct SwaggerConfig {
    #[serde(default)]
    pub urls: Option<Vec<UrlEntry>>,
    #[serde(default)]
    pub url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UrlEntry {
    pub url: String,
    /// 分组名，仅用于反序列化兼容，当前未消费。
    #[serde(default)]
    #[allow(dead_code)]
    pub name: Option<String>,
}

impl SwaggerConfig {
    /// 收集所有待拉取的 api-docs 文档地址（§2）。
    /// 优先 `urls`，否则单个 `url`；都没有则返回空（调用方报错「没有地址」）。
    pub fn doc_urls(&self) -> Vec<String> {
        if let Some(urls) = &self.urls {
            urls.iter().map(|e| e.url.clone()).collect()
        } else if let Some(url) = &self.url {
            vec![url.clone()]
        } else {
            Vec::new()
        }
    }
}

/// 单个分组的 api-docs 文档。
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ApiDoc {
    #[serde(default)]
    pub paths: IndexMap<String, PathItem>,
    #[serde(default)]
    pub components: Components,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Components {
    #[serde(default)]
    pub schemas: IndexMap<String, Schema>,
}

/// 一个路径下的各 HTTP 方法操作。OpenAPI 中还可能有 path 级 parameters/summary，
/// 原版只处理 get/post/put/delete 四种动词，未知字段忽略。
#[derive(Debug, Clone, Default, Deserialize)]
pub struct PathItem {
    #[serde(default)]
    pub get: Option<Operation>,
    #[serde(default)]
    pub post: Option<Operation>,
    #[serde(default)]
    pub put: Option<Operation>,
    #[serde(default)]
    pub delete: Option<Operation>,
}

/// HTTP 方法。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Delete,
}

impl HttpMethod {
    /// 大写动词，用于请求调用（如 `server.GET`）。
    pub fn as_upper(self) -> &'static str {
        match self {
            HttpMethod::Get => "GET",
            HttpMethod::Post => "POST",
            HttpMethod::Put => "PUT",
            HttpMethod::Delete => "DELETE",
        }
    }

    /// 小写动词，用于函数名前缀（原版 funcName 用 JSON 里的小写 key，如 `get_user_info`）。
    pub fn as_lower(self) -> &'static str {
        match self {
            HttpMethod::Get => "get",
            HttpMethod::Post => "post",
            HttpMethod::Put => "put",
            HttpMethod::Delete => "delete",
        }
    }
}

impl PathItem {
    /// 按固定顺序返回该路径已定义的 (方法, 操作)，供 lowering 遍历。
    pub fn operations(&self) -> Vec<(HttpMethod, &Operation)> {
        let mut out = Vec::new();
        if let Some(op) = &self.get {
            out.push((HttpMethod::Get, op));
        }
        if let Some(op) = &self.post {
            out.push((HttpMethod::Post, op));
        }
        if let Some(op) = &self.put {
            out.push((HttpMethod::Put, op));
        }
        if let Some(op) = &self.delete {
            out.push((HttpMethod::Delete, op));
        }
        out
    }
}

/// 单个接口操作。
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Operation {
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub deprecated: bool,
    #[serde(default)]
    pub parameters: Vec<Parameter>,
    #[serde(rename = "requestBody", default)]
    pub request_body: Option<RequestBody>,
    #[serde(default)]
    pub responses: IndexMap<String, Response>,
}

/// query / path 参数。
#[derive(Debug, Clone, Deserialize)]
pub struct Parameter {
    pub name: String,
    pub schema: Schema,
}

/// 请求体。原版只读 `content["application/json"].schema`。
#[derive(Debug, Clone, Default, Deserialize)]
pub struct RequestBody {
    #[serde(default)]
    pub content: IndexMap<String, MediaType>,
}

/// 响应。原版只读 `responses["200"].content["*/*"].schema`。
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Response {
    #[serde(default)]
    pub content: IndexMap<String, MediaType>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct MediaType {
    #[serde(default)]
    pub schema: Option<Schema>,
}

/// 类型 schema。所有字段可选，由 lowering 按 §5 规则解读。
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Schema {
    #[serde(rename = "type", default)]
    pub schema_type: Option<String>,
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub items: Option<Box<Schema>>,
    #[serde(rename = "$ref", default)]
    pub ref_path: Option<String>,
    /// 仅判断「是否枚举」，故只保留原始值列表。
    #[serde(rename = "enum", default)]
    pub enum_values: Option<Vec<Value>>,
    #[serde(rename = "additionalProperties", default)]
    pub additional_properties: Option<AdditionalProperties>,
    #[serde(default)]
    pub properties: Option<IndexMap<String, Schema>>,
    #[serde(default)]
    pub description: Option<String>,
}

/// `additionalProperties` 可能是布尔或子 schema（OpenAPI 允许两者）。
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum AdditionalProperties {
    Schema(Box<Schema>),
    // 接受 `additionalProperties: true` 形态，值本身不消费
    Bool(#[allow(dead_code)] bool),
}

impl AdditionalProperties {
    /// 取出子 schema（布尔形态返回 None）。
    pub fn as_schema(&self) -> Option<&Schema> {
        match self {
            AdditionalProperties::Schema(s) => Some(s),
            AdditionalProperties::Bool(_) => None,
        }
    }
}

impl Schema {
    /// `$ref` 的末段类型名（`#/components/schemas/Xxx` → `Xxx`）。
    pub fn ref_name(&self) -> Option<&str> {
        self.ref_path.as_deref().and_then(|r| r.rsplit('/').next())
    }

    /// 是否为枚举。
    pub fn is_enum(&self) -> bool {
        self.enum_values.as_ref().is_some_and(|v| !v.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_swagger_config_urls_and_single() {
        let c: SwaggerConfig = serde_json::from_str(
            r#"{ "urls": [ { "url": "/v3/api-docs/a", "name": "A" } ] }"#,
        )
        .unwrap();
        assert_eq!(c.doc_urls(), vec!["/v3/api-docs/a"]);

        let c: SwaggerConfig =
            serde_json::from_str(r#"{ "url": "/v3/api-docs/x" }"#).unwrap();
        assert_eq!(c.doc_urls(), vec!["/v3/api-docs/x"]);

        let c: SwaggerConfig = serde_json::from_str(r#"{}"#).unwrap();
        assert!(c.doc_urls().is_empty());
    }

    #[test]
    fn parses_api_doc_with_operations_in_order() {
        let json = r##"{
            "paths": {
                "/user/info": {
                    "get": { "summary": "查询", "responses": { "200": {
                        "content": { "*/*": { "schema": { "$ref": "#/components/schemas/RUserVo" } } }
                    } } },
                    "post": { "summary": "新增", "deprecated": true }
                }
            },
            "components": { "schemas": {
                "UserVo": { "type": "object", "properties": {
                    "id": { "type": "integer", "format": "int64" },
                    "name": { "type": "string", "description": "名称" }
                } }
            } }
        }"##;
        let doc: ApiDoc = serde_json::from_str(json).unwrap();

        let path = &doc.paths["/user/info"];
        let ops = path.operations();
        assert_eq!(ops.len(), 2);
        assert_eq!(ops[0].0, HttpMethod::Get);
        assert_eq!(ops[1].0, HttpMethod::Post);
        assert!(ops[1].1.deprecated);

        // 返回 schema 的 $ref 末段
        let resp = &ops[0].1.responses["200"];
        let schema = resp.content["*/*"].schema.as_ref().unwrap();
        assert_eq!(schema.ref_name(), Some("RUserVo"));

        // 模型字段顺序保留
        let vo = &doc.components.schemas["UserVo"];
        let props = vo.properties.as_ref().unwrap();
        let keys: Vec<_> = props.keys().cloned().collect();
        assert_eq!(keys, vec!["id", "name"]);
        assert_eq!(props["id"].format.as_deref(), Some("int64"));
    }

    #[test]
    fn parses_additional_properties_both_forms() {
        let s: Schema = serde_json::from_str(
            r#"{ "type": "object", "additionalProperties": { "type": "string" } }"#,
        )
        .unwrap();
        assert_eq!(
            s.additional_properties.unwrap().as_schema().unwrap().schema_type.as_deref(),
            Some("string")
        );

        let s: Schema = serde_json::from_str(
            r#"{ "type": "object", "additionalProperties": true }"#,
        )
        .unwrap();
        assert!(s.additional_properties.unwrap().as_schema().is_none());
    }

    #[test]
    fn detects_enum() {
        let s: Schema =
            serde_json::from_str(r#"{ "type": "string", "enum": ["A", "B"] }"#).unwrap();
        assert!(s.is_enum());
        let s: Schema = serde_json::from_str(r#"{ "type": "string" }"#).unwrap();
        assert!(!s.is_enum());
    }
}
