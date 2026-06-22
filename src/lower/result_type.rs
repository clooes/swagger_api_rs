//! 返回类型解析（MIGRATION.md §5.3，对应两份 `spliceApiResultType`）。
//!
//! 后端统一响应包装为 `R<T>`，类型名常带 `R` 前缀（如 `RBoolean`、`RIPageOrderVo`）。
//! 这里把它解包成语言无关的 IrType。
//!
//! 设计决策（§13）：原 TS 与 Flutter 两版在少数分支存在细微差异（多为历史不一致，
//! 而非有意为之）。由于 IrType 语言无关、表层差异由各 codegen 渲染（如 Map(Any) 在
//! TS 渲染为 `{[key:string]:any}`、Flutter 为 `Map<String,dynamic>`），此处**统一**为
//! 一套更完整的映射，取两版并集中更正确的语义。

use crate::ir::IrType;
use crate::openapi::Response;

/// 解析 `responses["200"]` 为返回类型；无内容则 Void。
pub fn lower_result_type(response: Option<&Response>) -> IrType {
    let Some(resp) = response else {
        return IrType::Void;
    };
    // 原版只读 `content["*/*"]`
    let Some(media) = resp.content.get("*/*") else {
        return IrType::Void;
    };
    let Some(schema) = &media.schema else {
        return IrType::Void;
    };

    // 分支 A：schema 自带 type（object / array / 标量）
    if let Some(t) = schema.schema_type.as_deref() {
        if t == "object" {
            return IrType::Any;
        }
        if t == "array" {
            if let Some(items) = &schema.items {
                if items.format.as_deref() == Some("byte") {
                    return IrType::Binary;
                }
                if let Some(r) = items.ref_name() {
                    return IrType::Array(Box::new(IrType::Ref(r.to_string())));
                }
                return IrType::Array(Box::new(scalar_type(
                    items.schema_type.as_deref(),
                    items.format.as_deref(),
                )));
            }
            if schema.format.as_deref() == Some("byte") {
                return IrType::Binary;
            }
            return IrType::Array(Box::new(IrType::String));
        }
        // type 为标量（原版此情形会落空/崩溃，这里稳健地按基础类型映射）
        if schema.format.as_deref() == Some("byte") {
            return IrType::Binary;
        }
        return scalar_type(Some(t), schema.format.as_deref());
    }

    // 分支 B：$ref → R<T> 解包
    match schema.ref_name() {
        Some(types) => resolve_wrapped(types),
        None => IrType::Void,
    }
}

/// 解包带 `R` 前缀的统一响应类型名（§5.3 的核心规则链）。
fn resolve_wrapped(types: &str) -> IrType {
    // 非 R 包装 → 直接作为类型（基础类型名 → 基础类型，否则模型引用）
    if !types.starts_with('R') {
        return ref_or_primitive(types);
    }
    let rest = &types[1..];

    // 精确匹配优先
    match rest {
        "Void" => return IrType::Void,
        "Boolean" => return IrType::Bool,
        "Int" | "Integer" => return IrType::Int,
        "Long" | "String" => return IrType::String,
        "SetString" => return IrType::Array(Box::new(IrType::String)),
        _ => {}
    }

    // 前缀匹配（注意顺序：IPage / MapLocalDate / MapString 须在通用 Map 之前）
    if let Some(inner) = rest.strip_prefix("IPage") {
        return IrType::IPage(Box::new(ref_or_primitive(inner)));
    }
    if let Some(s) = rest.strip_prefix("MapLocalDate") {
        return IrType::Map(Box::new(ref_or_primitive(s)));
    }
    if let Some(s) = rest.strip_prefix("MapString") {
        // 值类型可能再嵌 List/Set
        if let Some(elem) = s.strip_prefix("List") {
            return IrType::Map(Box::new(IrType::Array(Box::new(ref_or_primitive(elem)))));
        }
        if let Some(elem) = s.strip_prefix("Set") {
            return IrType::Map(Box::new(IrType::Array(Box::new(ref_or_primitive(elem)))));
        }
        return IrType::Map(Box::new(ref_or_primitive(s)));
    }
    if rest.starts_with("Map") {
        return IrType::Map(Box::new(IrType::Any));
    }
    if let Some(t) = rest.strip_prefix("List") {
        return resolve_list_elem(t);
    }

    // 兜底：去掉 R 前缀后作为类型（基础类型名 → 基础类型，否则模型引用）
    ref_or_primitive(rest)
}

/// 把拼接在类型名里的「Java 类型名」映射为 IrType：
/// 基础类型名（Integer/Long/String/Boolean/Double/Object/日期…）→ 对应基础类型；
/// 否则视为模型引用 Ref。用于 R<T> 解包时避免 `Integer`/`Boolean` 等泄漏到产物。
fn ref_or_primitive(name: &str) -> IrType {
    match name {
        "Integer" | "Int" => IrType::Int,
        "Long" => IrType::Long,
        "String" => IrType::String,
        "Boolean" | "Bool" => IrType::Bool,
        "Double" | "Float" | "BigDecimal" | "Number" => IrType::Double,
        "Object" => IrType::Any,
        "Void" => IrType::Void,
        // 日期时间统一为字符串
        "Date" | "LocalDate" | "LocalDateTime" | "LocalTime" | "Instant" => IrType::String,
        "" => IrType::Any,
        other => IrType::Ref(other.to_string()),
    }
}

/// 解析 `RList{X}` 中的元素类型 `t`（§5.3 List 分支，统一 TS/Flutter）。
fn resolve_list_elem(t: &str) -> IrType {
    match t {
        // 长整型/字符串/日期 列表 → 字符串数组
        "Long" | "String" | "LocalDate" => IrType::Array(Box::new(IrType::String)),
        // 业务特例（原 TS）：DztccCarType 列表 → MsgType 数组
        "DztccCarType" => IrType::Array(Box::new(IrType::MsgType)),
        "MapStringString" => {
            IrType::Array(Box::new(IrType::Map(Box::new(IrType::String))))
        }
        _ => {
            // List<MapStringXxx>：统一为「Map 数组」（§13：原 TS 历史写法返回 Map，此处更正为数组）
            if let Some(s) = t.strip_prefix("MapString") {
                let val = if let Some(elem) = s.strip_prefix("List") {
                    IrType::Array(Box::new(ref_or_primitive(elem)))
                } else if let Some(elem) = s.strip_prefix("Set") {
                    IrType::Array(Box::new(ref_or_primitive(elem)))
                } else {
                    ref_or_primitive(s)
                };
                return IrType::Array(Box::new(IrType::Map(Box::new(val))));
            }
            IrType::Array(Box::new(ref_or_primitive(t)))
        }
    }
}

/// 基础标量类型映射（用于数组元素 / 标量响应）。
fn scalar_type(ty: Option<&str>, fmt: Option<&str>) -> IrType {
    match ty {
        Some("integer") | Some("int") => {
            if fmt == Some("int64") {
                IrType::Long
            } else {
                IrType::Int
            }
        }
        Some("string") => IrType::String,
        Some("boolean") => IrType::Bool,
        Some("number") => {
            if fmt == Some("double") {
                IrType::Double
            } else {
                IrType::Int
            }
        }
        _ => IrType::Any,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openapi::Response;

    fn resp(json: &str) -> Response {
        serde_json::from_str(json).unwrap()
    }

    fn wrap(name: &str) -> IrType {
        // 构造一个返回 $ref 末段为 name 的 200 响应
        let json = format!(
            r##"{{"content":{{"*/*":{{"schema":{{"$ref":"#/c/{name}"}}}}}}}}"##
        );
        lower_result_type(Some(&resp(&json)))
    }

    #[test]
    fn no_content_is_void() {
        assert_eq!(lower_result_type(None), IrType::Void);
        assert_eq!(lower_result_type(Some(&resp(r#"{}"#))), IrType::Void);
    }

    #[test]
    fn scalar_wrappers() {
        assert_eq!(wrap("RVoid"), IrType::Void);
        assert_eq!(wrap("RBoolean"), IrType::Bool);
        assert_eq!(wrap("RInt"), IrType::Int);
        assert_eq!(wrap("RInteger"), IrType::Int);
        assert_eq!(wrap("RLong"), IrType::String);
        assert_eq!(wrap("RString"), IrType::String);
        assert_eq!(wrap("RSetString"), IrType::Array(Box::new(IrType::String)));
    }

    #[test]
    fn non_r_prefix_is_ref() {
        // 不以 R 开头 → 原样引用
        assert_eq!(wrap("OrderVo"), IrType::Ref("OrderVo".into()));
    }

    #[test]
    fn object_ref_wrapper() {
        // RXxxVo → 去 R 前缀引用
        assert_eq!(wrap("RDztccInfoVo"), IrType::Ref("DztccInfoVo".into()));
    }

    #[test]
    fn ipage() {
        assert_eq!(
            wrap("RIPageOrderVo"),
            IrType::IPage(Box::new(IrType::Ref("OrderVo".into())))
        );
    }

    #[test]
    fn maps() {
        assert_eq!(
            wrap("RMapStringOrderVo"),
            IrType::Map(Box::new(IrType::Ref("OrderVo".into())))
        );
        assert_eq!(
            wrap("RMapStringListOrderVo"),
            IrType::Map(Box::new(IrType::Array(Box::new(IrType::Ref("OrderVo".into())))))
        );
        assert_eq!(
            wrap("RMapLocalDateOrderVo"),
            IrType::Map(Box::new(IrType::Ref("OrderVo".into())))
        );
        assert_eq!(wrap("RMapAnything"), IrType::Map(Box::new(IrType::Any)));
        // Java 基础类型名作 map 值不应泄漏为 Ref
        assert_eq!(wrap("RMapStringInteger"), IrType::Map(Box::new(IrType::Int)));
        assert_eq!(wrap("RMapStringBoolean"), IrType::Map(Box::new(IrType::Bool)));
        assert_eq!(wrap("RMapStringLong"), IrType::Map(Box::new(IrType::Long)));
    }

    #[test]
    fn list_of_primitive_not_ref() {
        // List<Integer> 等基础类型名不应泄漏为 Ref
        assert_eq!(wrap("RListInteger"), IrType::Array(Box::new(IrType::Int)));
        assert_eq!(wrap("RListBoolean"), IrType::Array(Box::new(IrType::Bool)));
    }

    #[test]
    fn lists() {
        assert_eq!(wrap("RListLong"), IrType::Array(Box::new(IrType::String)));
        assert_eq!(
            wrap("RListOrderVo"),
            IrType::Array(Box::new(IrType::Ref("OrderVo".into())))
        );
        assert_eq!(
            wrap("RListDztccCarType"),
            IrType::Array(Box::new(IrType::MsgType))
        );
        assert_eq!(
            wrap("RListMapStringString"),
            IrType::Array(Box::new(IrType::Map(Box::new(IrType::String))))
        );
    }

    #[test]
    fn array_response_branch() {
        let r = resp(
            r##"{"content":{"*/*":{"schema":{"type":"array","items":{"$ref":"#/c/OrderVo"}}}}}"##,
        );
        assert_eq!(
            lower_result_type(Some(&r)),
            IrType::Array(Box::new(IrType::Ref("OrderVo".into())))
        );
    }

    #[test]
    fn byte_array_is_binary() {
        let r = resp(
            r#"{"content":{"*/*":{"schema":{"type":"array","items":{"format":"byte"}}}}}"#,
        );
        assert_eq!(lower_result_type(Some(&r)), IrType::Binary);
    }

    #[test]
    fn object_response_is_any() {
        let r = resp(r#"{"content":{"*/*":{"schema":{"type":"object"}}}}"#);
        assert_eq!(lower_result_type(Some(&r)), IrType::Any);
    }
}
