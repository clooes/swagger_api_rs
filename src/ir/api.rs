//! 接口（API）的 IR（对应 paths 下的 operation，MIGRATION.md §5.5）。

use super::types::IrType;
use crate::openapi::HttpMethod;

/// 参数的语义分类（§5.5）。原版用裸字符串 name=="dot"/"vo"/"file" 区分，
/// 这里建模为枚举，避免魔法字符串。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ParamKind {
    /// 普通标量参数（path 路径参数或 query 标量），对应原版普通 parameter。
    Scalar,
    /// query 引用模型（原版 name=="dot"）：一个 $ref 模型整体作为 query 参数。
    QueryRef,
    /// JSON 请求体模型（原版 name=="vo"）。
    Body,
    /// 文件上传（原版 name=="file"）。
    File,
}

/// 单个参数。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct IrParam {
    /// 参数名（QueryRef/Body/File 时为占位名 dot/vo/file 的语义已由 kind 表达，
    /// name 保留原始值供 codegen 需要时使用）。
    pub name: String,
    /// 参数类型。
    pub ty: IrType,
    /// 语义分类。
    pub kind: ParamKind,
    /// 该标量参数是否出现在 URL 路径模板中（即 `{name}`），
    /// 供 codegen 决定是做路径插值还是放进 query。仅 Scalar 有意义。
    pub in_path: bool,
}

/// 单个接口端点。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct IrEndpoint {
    /// 函数名（§6 规则：method + 规整后的 url）。
    pub func_name: String,
    /// HTTP 方法。
    pub http_method: HttpMethod,
    /// URL 模板，保留 `{param}` 占位（codegen 各自做插值，§6）。
    pub url: String,
    /// 接口描述（来自 summary）。
    pub summary: Option<String>,
    /// 是否已废弃。
    pub deprecated: bool,
    /// 全部参数（含 Scalar/QueryRef/Body/File）。
    pub params: Vec<IrParam>,
    /// 返回类型（Void 表示无返回）。
    pub result: IrType,
    /// 是否为导出接口（强制 Binary 返回，§5.4）。
    pub is_export: bool,
}

impl IrEndpoint {
    /// 是否含文件上传参数（§5.5）。
    pub fn has_file(&self) -> bool {
        self.params.iter().any(|p| p.kind == ParamKind::File)
    }

    /// 返回是否分页（§5.6）。
    pub fn is_paging(&self) -> bool {
        self.result.is_paging()
    }

    /// JSON 请求体参数（原版 vo）。
    pub fn body_param(&self) -> Option<&IrParam> {
        self.params.iter().find(|p| p.kind == ParamKind::Body)
    }

    /// query 引用模型参数（原版 dot）。
    pub fn query_ref_param(&self) -> Option<&IrParam> {
        self.params.iter().find(|p| p.kind == ParamKind::QueryRef)
    }

    /// 普通标量参数。
    pub fn scalar_params(&self) -> impl Iterator<Item = &IrParam> {
        self.params.iter().filter(|p| p.kind == ParamKind::Scalar)
    }
}
