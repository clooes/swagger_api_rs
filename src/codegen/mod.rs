//! 代码生成后端（backend）的统一接口（MIGRATION.md §7、§10）。
//!
//! 每种目标语言实现 `CodeGenerator` trait，仅负责「IR 节点 → 目标语法字符串」的
//! 纯翻译。新增一门语言 = 新增一个 trait 实现，**不改动 frontend / lowering**。

use crate::config::Language;
use crate::ir::{IrEndpoint, IrModel, IrModule, IrType};

/// 语言后端统一接口。
pub trait CodeGenerator {
    /// IrType → 目标语言类型字符串（如 IrType::Long → TS `string` / Dart `String`）。
    fn map_type(&self, ty: &IrType) -> String;

    /// 生成单个接口端点代码。
    fn gen_endpoint(&self, ep: &IrEndpoint) -> String;

    /// 生成单个数据模型代码。
    fn gen_model(&self, model: &IrModel) -> String;

    /// 内置通用类型块（IPage / MsgType / Paging 等，见 const.js）。
    fn builtin_types(&self) -> &'static str;

    /// 输出文件后缀（`ts` / `js` / `dart`）。
    fn file_ext(&self) -> &'static str;

    /// 组装整个模块：内置类型 → 接口 → 模型。
    ///
    /// 顺序对齐原 analyze.js（先 paths 后 schemas）；内置类型置顶，
    /// 供接口/模型引用 IPage/MsgType/Paging。header 由 emit 层拼接（§8）。
    fn generate(&self, module: &IrModule) -> String {
        let mut out = String::new();
        out.push_str(self.builtin_types());
        out.push('\n');
        for ep in &module.endpoints {
            out.push_str(&self.gen_endpoint(ep));
            out.push('\n');
        }
        for model in &module.models {
            out.push_str(&self.gen_model(model));
            out.push('\n');
        }
        out
    }
}

/// 按配置语言选择后端实现（§7）。
///
/// 具体生成器在 #8–#10 实现；此工厂是 emit 层（#11）的唯一入口。
pub fn for_language(language: Language) -> Box<dyn CodeGenerator> {
    match language {
        Language::Typescript => Box::new(typescript::TypescriptGen),
        Language::Javascript => Box::new(javascript::JavascriptGen),
        Language::Flutter => Box::new(flutter::FlutterGen),
    }
}

pub mod flutter;
pub mod javascript;
pub mod typescript;
