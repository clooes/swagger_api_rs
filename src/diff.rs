//! IR 级 diff（#15）。
//!
//! 对比上次缓存的 IrModule 与本次新生成的 IrModule，产出结构化变更。
//! 匹配 key：接口 func_name、模型 name、字段 name、参数 name。
//! 重命名识别（#19）在本算法的「删/增集」基础上做后处理填充 `renamed`。

use crate::ir::{IrEndpoint, IrModel, IrModule};

/// 一次生成相对上次的全部变更。
#[derive(Debug, Default)]
pub struct IrDiff {
    pub endpoints: EndpointDiff,
    pub models: ModelDiff,
}

#[derive(Debug, Default)]
pub struct EndpointDiff {
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub modified: Vec<EndpointChange>,
    /// 由 #19 重命名启发式填充。
    pub renamed: Vec<Rename>,
}

#[derive(Debug, Default)]
pub struct ModelDiff {
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub modified: Vec<ModelChange>,
    /// 由 #19 重命名启发式填充。
    pub renamed: Vec<Rename>,
}

/// 重命名（from → to）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rename {
    pub from: String,
    pub to: String,
}

/// 一个被修改的接口及其具体变更。
#[derive(Debug)]
pub struct EndpointChange {
    pub name: String,
    pub changes: Vec<Change>,
}

/// 接口的单项变更。
#[derive(Debug, PartialEq, Eq)]
pub enum Change {
    Return { from: String, to: String },
    Deprecated { from: bool, to: bool },
    SummaryChanged,
    ParamAdded(String),
    ParamRemoved(String),
    ParamTypeChanged { name: String, from: String, to: String },
}

/// 一个被修改的模型及其字段级变更。
#[derive(Debug)]
pub struct ModelChange {
    pub name: String,
    pub fields: Vec<FieldChange>,
}

/// 模型字段的单项变更。
#[derive(Debug, PartialEq, Eq)]
pub enum FieldChange {
    Added(String),
    Removed(String),
    TypeChanged { name: String, from: String, to: String },
    DescChanged(String),
}

impl IrDiff {
    /// 是否无任何变更。
    pub fn is_empty(&self) -> bool {
        let e = &self.endpoints;
        let m = &self.models;
        e.added.is_empty()
            && e.removed.is_empty()
            && e.modified.is_empty()
            && e.renamed.is_empty()
            && m.added.is_empty()
            && m.removed.is_empty()
            && m.modified.is_empty()
            && m.renamed.is_empty()
    }
}

/// 模型重命名判定的字段集相似度阈值（Jaccard）。
const RENAME_MODEL_SIMILARITY: f64 = 0.7;

/// 计算 old → new 的 diff（含重命名启发式，#19）。
pub fn diff(old: &IrModule, new: &IrModule) -> IrDiff {
    let mut endpoints = diff_endpoints(old, new);
    let mut models = diff_models(old, new);
    detect_endpoint_renames(old, new, &mut endpoints);
    detect_model_renames(old, new, &mut models);
    IrDiff { endpoints, models }
}

/// 接口重命名：被删与新增之间，HTTP 方法/参数/返回类型/导出标志完全一致即视为重命名。
fn detect_endpoint_renames(old: &IrModule, new: &IrModule, d: &mut EndpointDiff) {
    let removed = d.removed.clone();
    let mut used_added: Vec<String> = Vec::new();

    let mut still_removed = Vec::new();
    for from in removed {
        let oe = old.endpoints.iter().find(|e| e.func_name == from).unwrap();
        let matched = d.added.iter().find(|an| {
            !used_added.contains(*an)
                && new
                    .endpoints
                    .iter()
                    .find(|e| &e.func_name == *an)
                    .is_some_and(|ne| same_endpoint_shape(oe, ne))
        });
        match matched {
            Some(to) => {
                let to = to.clone();
                used_added.push(to.clone());
                d.renamed.push(Rename { from, to });
            }
            None => still_removed.push(from),
        }
    }
    d.removed = still_removed;
    d.added.retain(|a| !used_added.contains(a));
}

/// 两个接口除名字/url 外是否结构一致。
fn same_endpoint_shape(a: &IrEndpoint, b: &IrEndpoint) -> bool {
    a.http_method == b.http_method
        && a.result == b.result
        && a.is_export == b.is_export
        && a.params == b.params
}

/// 模型重命名：被删与新增之间，字段集 Jaccard 相似度 ≥ 阈值即视为重命名。
/// 全局贪心配对（按相似度降序，名字次序兜底，保证确定性与一对一）。
fn detect_model_renames(old: &IrModule, new: &IrModule, d: &mut ModelDiff) {
    let mut pairs: Vec<(f64, String, String)> = Vec::new();
    for from in &d.removed {
        let om = old.models.iter().find(|m| &m.name == from).unwrap();
        for to in &d.added {
            let nm = new.models.iter().find(|m| &m.name == to).unwrap();
            let sim = field_similarity(om, nm);
            if sim >= RENAME_MODEL_SIMILARITY {
                pairs.push((sim, from.clone(), to.clone()));
            }
        }
    }
    // 相似度降序；并列时按 from、to 字典序，保证确定性
    pairs.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.1.cmp(&b.1))
            .then_with(|| a.2.cmp(&b.2))
    });

    let mut used_from: Vec<String> = Vec::new();
    let mut used_to: Vec<String> = Vec::new();
    for (_, from, to) in pairs {
        if used_from.contains(&from) || used_to.contains(&to) {
            continue;
        }
        used_from.push(from.clone());
        used_to.push(to.clone());
        d.renamed.push(Rename { from, to });
    }
    d.removed.retain(|x| !used_from.contains(x));
    d.added.retain(|x| !used_to.contains(x));
}

/// 两个模型字段集 (名+类型) 的 Jaccard 相似度。
fn field_similarity(a: &IrModel, b: &IrModel) -> f64 {
    let set_a: std::collections::HashSet<(String, String)> = a
        .fields
        .iter()
        .map(|f| (f.name.clone(), f.ty.describe()))
        .collect();
    let set_b: std::collections::HashSet<(String, String)> = b
        .fields
        .iter()
        .map(|f| (f.name.clone(), f.ty.describe()))
        .collect();
    if set_a.is_empty() && set_b.is_empty() {
        return 0.0; // 两个空模型不据此判定重命名
    }
    let inter = set_a.intersection(&set_b).count() as f64;
    let union = set_a.union(&set_b).count() as f64;
    inter / union
}

fn diff_endpoints(old: &IrModule, new: &IrModule) -> EndpointDiff {
    let find = |list: &'_ [IrEndpoint], name: &str| -> Option<usize> {
        list.iter().position(|e| e.func_name == name)
    };

    let mut d = EndpointDiff::default();

    // 新增 + 修改（按 new 顺序）
    for ne in &new.endpoints {
        match old.endpoints.iter().find(|e| e.func_name == ne.func_name) {
            None => d.added.push(ne.func_name.clone()),
            Some(oe) if oe != ne => {
                d.modified.push(EndpointChange {
                    name: ne.func_name.clone(),
                    changes: endpoint_changes(oe, ne),
                });
            }
            Some(_) => {}
        }
    }
    // 删除（按 old 顺序）
    for oe in &old.endpoints {
        if find(&new.endpoints, &oe.func_name).is_none() {
            d.removed.push(oe.func_name.clone());
        }
    }
    d
}

fn endpoint_changes(old: &IrEndpoint, new: &IrEndpoint) -> Vec<Change> {
    let mut changes = Vec::new();

    if old.result != new.result {
        changes.push(Change::Return {
            from: old.result.describe(),
            to: new.result.describe(),
        });
    }
    if old.deprecated != new.deprecated {
        changes.push(Change::Deprecated {
            from: old.deprecated,
            to: new.deprecated,
        });
    }
    if old.summary != new.summary {
        changes.push(Change::SummaryChanged);
    }

    // 参数按 name 匹配
    for np in &new.params {
        match old.params.iter().find(|p| p.name == np.name) {
            None => changes.push(Change::ParamAdded(np.name.clone())),
            Some(op) if op.ty != np.ty => changes.push(Change::ParamTypeChanged {
                name: np.name.clone(),
                from: op.ty.describe(),
                to: np.ty.describe(),
            }),
            Some(_) => {}
        }
    }
    for op in &old.params {
        if !new.params.iter().any(|p| p.name == op.name) {
            changes.push(Change::ParamRemoved(op.name.clone()));
        }
    }
    changes
}

fn diff_models(old: &IrModule, new: &IrModule) -> ModelDiff {
    let mut d = ModelDiff::default();

    for nm in &new.models {
        match old.models.iter().find(|m| m.name == nm.name) {
            None => d.added.push(nm.name.clone()),
            Some(om) if om != nm => {
                d.modified.push(ModelChange {
                    name: nm.name.clone(),
                    fields: model_field_changes(om, nm),
                });
            }
            Some(_) => {}
        }
    }
    for om in &old.models {
        if !new.models.iter().any(|m| m.name == om.name) {
            d.removed.push(om.name.clone());
        }
    }
    d
}

fn model_field_changes(old: &IrModel, new: &IrModel) -> Vec<FieldChange> {
    let mut changes = Vec::new();
    for nf in &new.fields {
        match old.fields.iter().find(|f| f.name == nf.name) {
            None => changes.push(FieldChange::Added(nf.name.clone())),
            Some(of) => {
                if of.ty != nf.ty {
                    changes.push(FieldChange::TypeChanged {
                        name: nf.name.clone(),
                        from: of.ty.describe(),
                        to: nf.ty.describe(),
                    });
                } else if of.description != nf.description {
                    changes.push(FieldChange::DescChanged(nf.name.clone()));
                }
            }
        }
    }
    for of in &old.fields {
        if !new.fields.iter().any(|f| f.name == of.name) {
            changes.push(FieldChange::Removed(of.name.clone()));
        }
    }
    changes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{IrEndpoint, IrField, IrModel, IrParam, IrType, ParamKind};
    use crate::openapi::HttpMethod;

    fn ep(func: &str, result: IrType, params: Vec<IrParam>) -> IrEndpoint {
        IrEndpoint {
            func_name: func.into(),
            http_method: HttpMethod::Get,
            url: format!("/{func}"),
            summary: Some("s".into()),
            deprecated: false,
            params,
            result,
            is_export: false,
        }
    }

    fn model(name: &str, fields: &[(&str, IrType)]) -> IrModel {
        IrModel {
            name: name.into(),
            fields: fields
                .iter()
                .map(|(n, t)| IrField { name: (*n).into(), ty: t.clone(), description: None })
                .collect(),
        }
    }

    #[test]
    fn empty_when_identical() {
        let m = IrModule {
            endpoints: vec![ep("get_x", IrType::Bool, vec![])],
            models: vec![model("V", &[("id", IrType::Long)])],
        };
        assert!(diff(&m, &m).is_empty());
    }

    #[test]
    fn detects_added_removed_endpoints() {
        // 形状不同（返回类型不同），避免被重命名启发式配对
        let old = IrModule { endpoints: vec![ep("a", IrType::Bool, vec![])], models: vec![] };
        let new = IrModule { endpoints: vec![ep("b", IrType::Int, vec![])], models: vec![] };
        let d = diff(&old, &new);
        assert_eq!(d.endpoints.added, vec!["b"]);
        assert_eq!(d.endpoints.removed, vec!["a"]);
        assert!(d.endpoints.renamed.is_empty());
    }

    #[test]
    fn detects_return_and_param_changes() {
        let p_old = IrParam { name: "id".into(), ty: IrType::Long, kind: ParamKind::Scalar, in_path: true };
        let p_new = IrParam { name: "id".into(), ty: IrType::Int, kind: ParamKind::Scalar, in_path: true };
        let old = IrModule { endpoints: vec![ep("x", IrType::Bool, vec![p_old])], models: vec![] };
        let new = IrModule { endpoints: vec![ep("x", IrType::Int, vec![p_new])], models: vec![] };
        let d = diff(&old, &new);
        assert_eq!(d.endpoints.modified.len(), 1);
        let ch = &d.endpoints.modified[0].changes;
        assert!(ch.contains(&Change::Return { from: "bool".into(), to: "int".into() }));
        assert!(ch.contains(&Change::ParamTypeChanged { name: "id".into(), from: "long".into(), to: "int".into() }));
    }

    #[test]
    fn detects_model_field_changes() {
        let old = IrModule { endpoints: vec![], models: vec![model("V", &[("a", IrType::Long), ("b", IrType::Bool)])] };
        let new = IrModule { endpoints: vec![], models: vec![model("V", &[("a", IrType::Int), ("c", IrType::String)])] };
        let d = diff(&old, &new);
        assert_eq!(d.models.modified.len(), 1);
        let fields = &d.models.modified[0].fields;
        assert!(fields.contains(&FieldChange::TypeChanged { name: "a".into(), from: "long".into(), to: "int".into() }));
        assert!(fields.contains(&FieldChange::Added("c".into())));
        assert!(fields.contains(&FieldChange::Removed("b".into())));
    }

    #[test]
    fn detects_endpoint_rename() {
        // 同结构、不同函数名（url 改了）→ 重命名而非删+增
        let old = IrModule { endpoints: vec![ep("get_v1_users", IrType::Bool, vec![])], models: vec![] };
        let new = IrModule { endpoints: vec![ep("get_v2_users", IrType::Bool, vec![])], models: vec![] };
        let d = diff(&old, &new);
        assert!(d.endpoints.added.is_empty(), "added 应为空");
        assert!(d.endpoints.removed.is_empty(), "removed 应为空");
        assert_eq!(d.endpoints.renamed, vec![Rename { from: "get_v1_users".into(), to: "get_v2_users".into() }]);
    }

    #[test]
    fn different_shape_not_rename() {
        // 返回类型不同 → 不算重命名
        let old = IrModule { endpoints: vec![ep("get_a", IrType::Bool, vec![])], models: vec![] };
        let new = IrModule { endpoints: vec![ep("get_b", IrType::Int, vec![])], models: vec![] };
        let d = diff(&old, &new);
        assert_eq!(d.endpoints.added, vec!["get_b"]);
        assert_eq!(d.endpoints.removed, vec!["get_a"]);
        assert!(d.endpoints.renamed.is_empty());
    }

    #[test]
    fn detects_model_rename_by_field_similarity() {
        // 字段集高度一致、仅名字不同 → 重命名
        let fields = &[("id", IrType::Long), ("name", IrType::String), ("age", IrType::Int)][..];
        let old = IrModule { endpoints: vec![], models: vec![model("UserVo", fields)] };
        let new = IrModule { endpoints: vec![], models: vec![model("UserInfoVo", fields)] };
        let d = diff(&old, &new);
        assert!(d.models.added.is_empty());
        assert!(d.models.removed.is_empty());
        assert_eq!(d.models.renamed, vec![Rename { from: "UserVo".into(), to: "UserInfoVo".into() }]);
    }

    #[test]
    fn dissimilar_models_not_rename() {
        let old = IrModule { endpoints: vec![], models: vec![model("A", &[("x", IrType::Long)])] };
        let new = IrModule { endpoints: vec![], models: vec![model("B", &[("y", IrType::Bool), ("z", IrType::String)])] };
        let d = diff(&old, &new);
        assert_eq!(d.models.added, vec!["B"]);
        assert_eq!(d.models.removed, vec!["A"]);
        assert!(d.models.renamed.is_empty());
    }

    #[test]
    fn detects_added_removed_models() {
        let old = IrModule { endpoints: vec![], models: vec![model("A", &[])] };
        let new = IrModule { endpoints: vec![], models: vec![model("B", &[])] };
        let d = diff(&old, &new);
        assert_eq!(d.models.added, vec!["B"]);
        assert_eq!(d.models.removed, vec!["A"]);
    }
}
