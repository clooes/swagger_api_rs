//! 字段/参数基础类型映射（MIGRATION.md §5.2，对应两份 `integerFc` 的合并）。
//!
//! 这里合并了 TS 与 Flutter 两版 integerFc 的全部分支，产出语言无关的 IrType。

use crate::ir::IrType;
use crate::openapi::Schema;

/// 把一个 schema 映射为 IrType。
///
/// `is_dot` 表示该 schema 来自「query 引用模型的参数上下文」或「Dto 模型字段」，
/// 影响枚举处理：枚举在 dot/Dto 语境下视为数字（Int），否则视为通用 MsgType。
pub fn lower_type(schema: &Schema, is_dot: bool) -> IrType {
    // 1. additionalProperties → Map
    if let Some(ap) = &schema.additional_properties {
        return match ap.as_schema() {
            Some(inner) => {
                // 特例：值对象含 andIncrement 字段 → 值类型强制为 Int（原版 number）
                let has_increment = inner
                    .properties
                    .as_ref()
                    .is_some_and(|p| p.contains_key("andIncrement"));
                let val = if has_increment {
                    IrType::Int
                } else {
                    lower_type(inner, false)
                };
                IrType::Map(Box::new(val))
            }
            // additionalProperties: true → 值类型未知
            None => IrType::Map(Box::new(IrType::Any)),
        };
    }

    // 2. enum
    if schema.is_enum() {
        return if is_dot { IrType::Int } else { IrType::MsgType };
    }

    let ty = schema.schema_type.as_deref();
    let fmt = schema.format.as_deref();

    // 3. int / integer
    if matches!(ty, Some("int") | Some("integer")) {
        return if fmt == Some("int64") {
            IrType::Long
        } else {
            IrType::Int
        };
    }

    // 4. file
    if ty == Some("file") {
        return IrType::File;
    }

    // 5 / 6. array
    if ty == Some("array") {
        let elem = match &schema.items {
            Some(items) => {
                if let Some(r) = items.ref_name() {
                    IrType::Ref(r.to_string())
                } else if matches!(items.schema_type.as_deref(), Some("int") | Some("integer")) {
                    if items.format.as_deref() == Some("int64") {
                        IrType::Long
                    } else {
                        IrType::Int
                    }
                } else {
                    // 原版数组元素默认 String
                    IrType::String
                }
            }
            None => IrType::String,
        };
        // binary 覆盖（§5.2 step 13）：有效 format 为 binary → 文件数组
        let eff_fmt = fmt.or_else(|| schema.items.as_deref().and_then(|i| i.format.as_deref()));
        if eff_fmt == Some("binary") {
            return IrType::Array(Box::new(IrType::File));
        }
        return IrType::Array(Box::new(elem));
    }

    // 7. long
    if ty == Some("long") {
        return IrType::Long;
    }

    // 8. $ref
    if let Some(r) = schema.ref_name() {
        if r == "LocalTime" {
            return IrType::String;
        }
        return IrType::Ref(r.to_string());
    }

    // 单值 binary（§5.2 step 13）：format binary 优先于基础类型映射
    if fmt == Some("binary") {
        return IrType::File;
    }

    // 9 - 12. 基础类型
    match ty {
        Some("object") => IrType::Any,
        Some("string") | Some("LocalTime") => IrType::String,
        Some("boolean") | Some("Boolean") => IrType::Bool,
        Some("number") => {
            if fmt == Some("double") {
                IrType::Double
            } else {
                IrType::Int
            }
        }
        // 未知/缺省类型兜底
        _ => IrType::Any,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema(json: &str) -> Schema {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn int_and_long() {
        assert_eq!(lower_type(&schema(r#"{"type":"integer"}"#), false), IrType::Int);
        assert_eq!(
            lower_type(&schema(r#"{"type":"integer","format":"int64"}"#), false),
            IrType::Long
        );
    }

    #[test]
    fn enum_depends_on_context() {
        let s = schema(r#"{"type":"string","enum":["A"]}"#);
        assert_eq!(lower_type(&s, false), IrType::MsgType);
        assert_eq!(lower_type(&s, true), IrType::Int);
    }

    #[test]
    fn arrays() {
        assert_eq!(
            lower_type(&schema(r#"{"type":"array","items":{"type":"string"}}"#), false),
            IrType::Array(Box::new(IrType::String))
        );
        assert_eq!(
            lower_type(&schema(r##"{"type":"array","items":{"$ref":"#/c/OrderVo"}}"##), false),
            IrType::Array(Box::new(IrType::Ref("OrderVo".into())))
        );
        // 无 items → string[]
        assert_eq!(
            lower_type(&schema(r#"{"type":"array"}"#), false),
            IrType::Array(Box::new(IrType::String))
        );
    }

    #[test]
    fn map_and_increment() {
        assert_eq!(
            lower_type(&schema(r#"{"type":"object","additionalProperties":{"type":"string"}}"#), false),
            IrType::Map(Box::new(IrType::String))
        );
        let s = schema(
            r#"{"type":"object","additionalProperties":{"type":"object","properties":{"andIncrement":{"type":"integer"}}}}"#,
        );
        assert_eq!(lower_type(&s, false), IrType::Map(Box::new(IrType::Int)));
    }

    #[test]
    fn ref_and_localtime() {
        assert_eq!(
            lower_type(&schema(r##"{"$ref":"#/c/AppLoginDto"}"##), false),
            IrType::Ref("AppLoginDto".into())
        );
        assert_eq!(
            lower_type(&schema(r##"{"$ref":"#/c/LocalTime"}"##), false),
            IrType::String
        );
    }

    #[test]
    fn binary_files() {
        assert_eq!(lower_type(&schema(r#"{"type":"string","format":"binary"}"#), false), IrType::File);
        // 数组 + items.format binary → 文件数组
        assert_eq!(
            lower_type(&schema(r#"{"type":"array","items":{"type":"string","format":"binary"}}"#), false),
            IrType::Array(Box::new(IrType::File))
        );
    }

    #[test]
    fn primitives() {
        assert_eq!(lower_type(&schema(r#"{"type":"string"}"#), false), IrType::String);
        assert_eq!(lower_type(&schema(r#"{"type":"boolean"}"#), false), IrType::Bool);
        assert_eq!(lower_type(&schema(r#"{"type":"number","format":"double"}"#), false), IrType::Double);
        assert_eq!(lower_type(&schema(r#"{"type":"number"}"#), false), IrType::Int);
        assert_eq!(lower_type(&schema(r#"{"type":"object"}"#), false), IrType::Any);
        assert_eq!(lower_type(&schema(r#"{"type":"long"}"#), false), IrType::Long);
    }
}
