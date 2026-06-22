//! 语言无关的类型表示（MIGRATION.md §5.1）。
//!
//! IrType 是整个重构的核心：所有「Java 类型名 → 语义」的解析（R<T> 解包、
//! MapString、IPage、int64→Long 等）在 `lower/` 完成后，结果用 IrType 表达；
//! 各语言后端只把 IrType 翻译成具体语法（见各 codegen 的 map_type）。
//!
//! **IrType 必须完全语言无关**，不得出现 number/int/String 等目标语言词汇。

/// 语言无关类型。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum IrType {
    /// 32 位整数。
    Int,
    /// int64 / long —— 在 TS/Dart 中通常映射为字符串以避免精度丢失。
    Long,
    /// 浮点数。
    Double,
    /// 布尔。
    Bool,
    /// 字符串。
    String,
    /// 数组 `T[]` / `List<T>`。
    Array(Box<IrType>),
    /// 字典 `{[key:string]:T}` / `Map<String,T>`。
    Map(Box<IrType>),
    /// 引用其它具名模型（值为模型名）。
    Ref(String),
    /// 分页泛型 `IPage<T>`。
    IPage(Box<IrType>),
    /// 枚举的非 Dto 形态（统一映射为通用 MsgType）。
    MsgType,
    /// 文件上传（TS `File` / Dart `XFile`）。
    File,
    /// 二进制流（byte/binary）—— 通常用于导出接口，TS `ArrayBuffer`。
    Binary,
    /// 任意对象（OpenAPI `type: object` 无 schema）。
    Any,
    /// 无返回值（R<Void> / 无 content）。
    Void,
}

impl IrType {
    /// 是否为数组类型（codegen 决定返回值是否补 `?? []`、Future 是否非空等）。
    pub fn is_array(&self) -> bool {
        matches!(self, IrType::Array(_))
    }

    /// 是否为分页类型（codegen 决定是否注入 Paging / pageNum,pageSize）。
    pub fn is_paging(&self) -> bool {
        matches!(self, IrType::IPage(_))
    }

    /// 是否表示「无返回值」。
    pub fn is_void(&self) -> bool {
        matches!(self, IrType::Void)
    }

    /// 若为数组，返回元素类型。
    pub fn array_elem(&self) -> Option<&IrType> {
        match self {
            IrType::Array(inner) => Some(inner),
            _ => None,
        }
    }

    /// 语言无关的可读描述，用于 diff 报告（非目标语言代码）。
    pub fn describe(&self) -> String {
        match self {
            IrType::Int => "int".into(),
            IrType::Long => "long".into(),
            IrType::Double => "double".into(),
            IrType::Bool => "bool".into(),
            IrType::String => "string".into(),
            IrType::Array(inner) => format!("Array<{}>", inner.describe()),
            IrType::Map(inner) => format!("Map<{}>", inner.describe()),
            IrType::Ref(name) => name.clone(),
            IrType::IPage(inner) => format!("IPage<{}>", inner.describe()),
            IrType::MsgType => "MsgType".into(),
            IrType::File => "File".into(),
            IrType::Binary => "Binary".into(),
            IrType::Any => "any".into(),
            IrType::Void => "void".into(),
        }
    }

    /// 按重命名表递归重写内部所有 Ref（含嵌套 Array/Map/IPage）。
    /// 用于模型重命名后同步更新所有引用（见 lower::normalize）。
    pub fn rewrite_refs(&mut self, renames: &std::collections::HashMap<String, String>) {
        match self {
            IrType::Ref(name) => {
                if let Some(new) = renames.get(name) {
                    *name = new.clone();
                }
            }
            IrType::Array(inner) | IrType::Map(inner) | IrType::IPage(inner) => {
                inner.rewrite_refs(renames);
            }
            _ => {}
        }
    }
}
