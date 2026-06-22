//! 名称规范化 / 多文档合并（#21）。
//!
//! 多个 api-docs 分组合并到单文件时，可能出现同名 schema（interface/class 重名）
//! 或同名接口。本模块在合并时消解冲突：
//!   - 模型同名 + 结构相同 → 去重（保留一份，引用不变）
//!   - 模型同名 + 结构不同 → 重命名（加序号后缀）并**递归重写本文档内所有引用**
//!   - 接口同名（method+url 一致）→ 相同则去重，不同则函数名加后缀
//!
//! 重命名规则确定性（同输入同输出），保证第二阶段 diff 稳定。

use std::collections::HashMap;

use crate::ir::IrModule;

/// 把 `incoming` 合并进 `merged`，消解重名冲突。
pub fn merge_into(merged: &mut IrModule, mut incoming: IrModule) {
    // 1. 收集模型重命名表（仅「同名异构」需要改名）
    let mut renames: HashMap<String, String> = HashMap::new();
    for model in &incoming.models {
        if let Some(existing) = merged.models.iter().find(|m| m.name == model.name) {
            if existing.fields != model.fields {
                let new_name = fresh_model_name(&model.name, merged, &incoming, &renames);
                renames.insert(model.name.clone(), new_name);
            }
            // 同名同构：保留已有，稍后去重，无需改名
        }
    }

    // 2. 重写 incoming 内所有引用（字段 / 参数 / 返回类型）
    if !renames.is_empty() {
        for m in &mut incoming.models {
            for f in &mut m.fields {
                f.ty.rewrite_refs(&renames);
            }
        }
        for e in &mut incoming.endpoints {
            for p in &mut e.params {
                p.ty.rewrite_refs(&renames);
            }
            e.result.rewrite_refs(&renames);
        }
    }

    // 3. 合并模型：应用重命名；同名同构则去重跳过
    for mut model in incoming.models {
        if let Some(new) = renames.get(&model.name) {
            model.name = new.clone();
        }
        if merged.models.iter().any(|m| m.name == model.name) {
            // 走到这里只可能是「同名同构」的去重情形
            continue;
        }
        merged.models.push(model);
    }

    // 4. 合并接口：func_name 冲突时，相同去重、不同加后缀
    for mut ep in incoming.endpoints {
        match merged.endpoints.iter().find(|e| e.func_name == ep.func_name) {
            Some(existing) if *existing == ep => continue, // 完全相同 → 去重
            Some(_) => {
                ep.func_name = fresh_func_name(&ep.func_name, merged);
                merged.endpoints.push(ep);
            }
            None => merged.endpoints.push(ep),
        }
    }
}

/// 为重名模型生成一个未占用的新名：base2, base3, …（确定性）。
fn fresh_model_name(
    base: &str,
    merged: &IrModule,
    incoming: &IrModule,
    renames: &HashMap<String, String>,
) -> String {
    let taken = |name: &str| {
        merged.models.iter().any(|m| m.name == name)
            || incoming.models.iter().any(|m| m.name == name)
            || renames.values().any(|v| v == name)
    };
    let mut n = 2u32;
    loop {
        let candidate = format!("{base}{n}");
        if !taken(&candidate) {
            return candidate;
        }
        n += 1;
    }
}

/// 为重名接口生成未占用的新函数名。
fn fresh_func_name(base: &str, merged: &IrModule) -> String {
    let mut n = 2u32;
    loop {
        let candidate = format!("{base}_{n}");
        if !merged.endpoints.iter().any(|e| e.func_name == candidate) {
            return candidate;
        }
        n += 1;
    }
}

/// 供单元测试与外部按需调用：对单个 IrModule 内部做引用重写。
#[cfg(test)]
pub fn rewrite_all_refs(module: &mut IrModule, renames: &HashMap<String, String>) {
    for m in &mut module.models {
        for f in &mut m.fields {
            f.ty.rewrite_refs(renames);
        }
    }
    for e in &mut module.endpoints {
        for p in &mut e.params {
            p.ty.rewrite_refs(renames);
        }
        e.result.rewrite_refs(renames);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{IrEndpoint, IrField, IrModel, IrType, ParamKind};
    use crate::openapi::HttpMethod;

    fn model(name: &str, fields: &[(&str, IrType)]) -> IrModel {
        IrModel {
            name: name.into(),
            fields: fields
                .iter()
                .map(|(n, t)| IrField { name: (*n).into(), ty: t.clone(), description: None })
                .collect(),
        }
    }

    fn endpoint(func: &str, result: IrType) -> IrEndpoint {
        IrEndpoint {
            func_name: func.into(),
            http_method: HttpMethod::Get,
            url: format!("/{func}"),
            summary: None,
            deprecated: false,
            params: vec![],
            result,
            is_export: false,
        }
    }

    #[test]
    fn dedup_identical_model() {
        let mut merged = IrModule::new();
        merge_into(&mut merged, IrModule {
            models: vec![model("UserVo", &[("id", IrType::Long)])],
            endpoints: vec![],
        });
        merge_into(&mut merged, IrModule {
            models: vec![model("UserVo", &[("id", IrType::Long)])],
            endpoints: vec![],
        });
        assert_eq!(merged.models.len(), 1, "同名同构应去重");
    }

    #[test]
    fn rename_conflicting_model_and_rewrite_refs() {
        let mut merged = IrModule::new();
        merge_into(&mut merged, IrModule {
            models: vec![model("UserVo", &[("id", IrType::Long)])],
            endpoints: vec![],
        });
        // 第二个 doc 的 UserVo 结构不同，且被一个接口/模型引用
        let incoming = IrModule {
            models: vec![
                model("UserVo", &[("name", IrType::String)]),
                model("Wrap", &[("user", IrType::Ref("UserVo".into()))]),
            ],
            endpoints: vec![endpoint("get_user", IrType::Ref("UserVo".into()))],
        };
        merge_into(&mut merged, incoming);

        // 两个不同结构的 UserVo 都在
        assert!(merged.models.iter().any(|m| m.name == "UserVo"));
        assert!(merged.models.iter().any(|m| m.name == "UserVo2"));
        // 引用被重写到 UserVo2
        let wrap = merged.models.iter().find(|m| m.name == "Wrap").unwrap();
        assert_eq!(wrap.fields[0].ty, IrType::Ref("UserVo2".into()));
        let ep = merged.endpoints.iter().find(|e| e.func_name == "get_user").unwrap();
        assert_eq!(ep.result, IrType::Ref("UserVo2".into()));
    }

    #[test]
    fn rewrite_refs_nested() {
        let mut renames = HashMap::new();
        renames.insert("A".to_string(), "A2".to_string());
        let mut m = IrModule {
            models: vec![model(
                "M",
                &[
                    ("list", IrType::Array(Box::new(IrType::Ref("A".into())))),
                    ("page", IrType::IPage(Box::new(IrType::Ref("A".into())))),
                    ("map", IrType::Map(Box::new(IrType::Ref("B".into())))),
                ],
            )],
            endpoints: vec![],
        };
        rewrite_all_refs(&mut m, &renames);
        let f = &m.models[0].fields;
        assert_eq!(f[0].ty, IrType::Array(Box::new(IrType::Ref("A2".into()))));
        assert_eq!(f[1].ty, IrType::IPage(Box::new(IrType::Ref("A2".into()))));
        assert_eq!(f[2].ty, IrType::Map(Box::new(IrType::Ref("B".into())))); // 未命中保持
    }

    #[test]
    fn dedup_identical_endpoint_and_rename_diff() {
        let mut merged = IrModule::new();
        merge_into(&mut merged, IrModule {
            models: vec![],
            endpoints: vec![endpoint("get_x", IrType::Bool)],
        });
        // 相同接口 → 去重
        merge_into(&mut merged, IrModule {
            models: vec![],
            endpoints: vec![endpoint("get_x", IrType::Bool)],
        });
        assert_eq!(merged.endpoints.len(), 1);
        // 同名不同接口 → 加后缀
        merge_into(&mut merged, IrModule {
            models: vec![],
            endpoints: vec![endpoint("get_x", IrType::Int)],
        });
        assert_eq!(merged.endpoints.len(), 2);
        assert!(merged.endpoints.iter().any(|e| e.func_name == "get_x_2"));
    }

    // 触发 ParamKind 使用，避免 import 警告
    #[allow(dead_code)]
    fn _kind() -> ParamKind {
        ParamKind::Scalar
    }
}
