//! 语言无关的中间表示（IR / AST）。
//!
//! 这是整个重构的核心枢纽（MIGRATION.md §5）：
//!   - `lower/` 把 OpenAPI spec 解析（lower）成 `IrModule`
//!   - `codegen/` 各语言后端只消费 `IrModule`，做「IR → 语法」纯翻译
//!
//! 数据流：openapi::ApiDoc ──(lower)──> ir::IrModule ──(codegen)──> 代码字符串

pub mod api;
pub mod model;
pub mod types;

pub use api::{IrEndpoint, IrParam, ParamKind};
pub use model::{IrField, IrModel};
pub use types::IrType;

/// lowering 的产物：一组接口 + 一组数据模型，完全语言无关。
///
/// 对应一次代码生成的全部内容（可能由多个 api-docs 合并而来）。
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct IrModule {
    /// 所有接口端点，保持发现顺序。
    pub endpoints: Vec<IrEndpoint>,
    /// 所有数据模型，保持发现顺序。
    pub models: Vec<IrModel>,
}

impl IrModule {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.endpoints.is_empty() && self.models.is_empty()
    }
}

/// 缓存格式版本。IR 结构发生不兼容变化时递增，旧缓存将被降级为「首次生成」。
pub const CACHE_VERSION: u32 = 1;

/// 持久化到 `{output}/.swagger-ir.json` 的缓存外壳（#13/#14）。
///
/// 带 `version` 以便升级时安全降级；存的是**规范化后**的 IrModule（见 lower::normalize），
/// 保证第二阶段 diff 稳定。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct IrCache {
    pub version: u32,
    pub module: IrModule,
}

impl IrCache {
    pub fn new(module: IrModule) -> Self {
        Self { version: CACHE_VERSION, module }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::api::{IrEndpoint, IrParam, ParamKind};
    use crate::openapi::HttpMethod;

    #[test]
    fn ir_module_json_roundtrip() {
        let module = IrModule {
            endpoints: vec![IrEndpoint {
                func_name: "get_user".into(),
                http_method: HttpMethod::Get,
                url: "/user/{id}".into(),
                summary: Some("查询".into()),
                deprecated: false,
                params: vec![IrParam {
                    name: "id".into(),
                    ty: IrType::Long,
                    kind: ParamKind::Scalar,
                    in_path: true,
                }],
                result: IrType::IPage(Box::new(IrType::Ref("UserVo".into()))),
                is_export: false,
            }],
            models: vec![IrModel {
                name: "UserVo".into(),
                fields: vec![IrField {
                    name: "id".into(),
                    ty: IrType::Long,
                    description: Some("ID".into()),
                }],
            }],
        };
        let cache = IrCache::new(module.clone());
        let json = serde_json::to_string(&cache).unwrap();
        let back: IrCache = serde_json::from_str(&json).unwrap();
        assert_eq!(back.version, CACHE_VERSION);
        assert_eq!(back.module, module);
    }
}
