//! 数据模型的 IR（对应 components.schemas，MIGRATION.md §5）。

use super::types::IrType;

/// 一个数据模型（生成 TS interface / Dart class）。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct IrModel {
    /// 模型名（来自 schema key，如 `AppLoginDto`、`RIPageOrderVo`）。
    pub name: String,
    /// 字段列表，保持原始顺序。
    pub fields: Vec<IrField>,
}

/// 模型字段。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct IrField {
    /// 字段名。
    pub name: String,
    /// 字段类型。
    pub ty: IrType,
    /// 字段描述（来自 schema.description），用于生成注释。
    pub description: Option<String>,
}

impl IrField {
    /// 描述中是否含「枚举」标记。原版据此把字段类型强制为数字
    /// （TS `number` / Dart `int`），见 §7.1 / §7.2。
    pub fn is_enum_hint(&self) -> bool {
        self.description
            .as_deref()
            .is_some_and(|d| d.contains("枚举"))
    }
}
