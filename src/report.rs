//! diff 报告输出（#16）。
//!
//! 把 IrDiff 渲染为控制台文本：先摘要、后明细。ANSI 颜色：+绿 / -红 / ~黄 / ↻青；
//! 非 tty（管道/重定向）时自动关闭颜色。render() 返回纯文本便于测试。

use std::io::IsTerminal;

use crate::diff::{Change, FieldChange, IrDiff};

const GREEN: &str = "32";
const RED: &str = "31";
const YELLOW: &str = "33";
const CYAN: &str = "36";

fn paint(s: &str, code: &str, color: bool) -> String {
    if color {
        format!("\x1b[{code}m{s}\x1b[0m")
    } else {
        s.to_string()
    }
}

/// 打印 diff 到 stdout，自动检测是否启用颜色。
pub fn print(diff: &IrDiff) {
    let color = std::io::stdout().is_terminal();
    print!("{}", render(diff, color));
}

/// 渲染 diff 为文本。`color` 控制是否输出 ANSI 颜色。
pub fn render(diff: &IrDiff, color: bool) -> String {
    if diff.is_empty() {
        return "本次无变更\n".to_string();
    }

    let mut out = String::from("本次变更:\n");

    // 摘要
    let e = &diff.endpoints;
    let m = &diff.models;
    out.push_str(&format!(
        "  接口: +{} ~{} -{} ↻{}\n",
        e.added.len(),
        e.modified.len(),
        e.removed.len(),
        e.renamed.len()
    ));
    out.push_str(&format!(
        "  模型: +{} ~{} -{} ↻{}\n",
        m.added.len(),
        m.modified.len(),
        m.removed.len(),
        m.renamed.len()
    ));
    out.push('\n');

    // 接口明细
    for r in &e.renamed {
        out.push_str(&paint(&format!("  ↻ 重命名接口 {} → {}\n", r.from, r.to), CYAN, color));
    }
    for name in &e.added {
        out.push_str(&paint(&format!("  + 新增接口 {name}\n"), GREEN, color));
    }
    for c in &e.modified {
        out.push_str(&paint(&format!("  ~ 修改接口 {}\n", c.name), YELLOW, color));
        for ch in &c.changes {
            out.push_str(&render_change(ch, color));
        }
    }
    for name in &e.removed {
        out.push_str(&paint(&format!("  - 删除接口 {name}\n"), RED, color));
    }

    // 模型明细
    for r in &m.renamed {
        out.push_str(&paint(&format!("  ↻ 重命名模型 {} → {}\n", r.from, r.to), CYAN, color));
    }
    for name in &m.added {
        out.push_str(&paint(&format!("  + 新增模型 {name}\n"), GREEN, color));
    }
    for c in &m.modified {
        out.push_str(&paint(&format!("  ~ 修改模型 {}\n", c.name), YELLOW, color));
        for f in &c.fields {
            out.push_str(&render_field(f, color));
        }
    }
    for name in &m.removed {
        out.push_str(&paint(&format!("  - 删除模型 {name}\n"), RED, color));
    }

    out
}

fn render_change(ch: &Change, color: bool) -> String {
    match ch {
        Change::Return { from, to } => format!("      返回类型: {from} → {to}\n"),
        Change::Deprecated { from, to } => format!("      标记废弃: {from} → {to}\n"),
        Change::SummaryChanged => "      描述变更\n".to_string(),
        Change::ParamAdded(n) => paint(&format!("      + 参数 {n}\n"), GREEN, color),
        Change::ParamRemoved(n) => paint(&format!("      - 参数 {n}\n"), RED, color),
        Change::ParamTypeChanged { name, from, to } => {
            paint(&format!("      ~ 参数 {name}: {from} → {to}\n"), YELLOW, color)
        }
    }
}

fn render_field(f: &FieldChange, color: bool) -> String {
    match f {
        FieldChange::Added(n) => paint(&format!("      + 字段 {n}\n"), GREEN, color),
        FieldChange::Removed(n) => paint(&format!("      - 字段 {n}\n"), RED, color),
        FieldChange::TypeChanged { name, from, to } => {
            paint(&format!("      ~ 字段 {name}: {from} → {to}\n"), YELLOW, color)
        }
        FieldChange::DescChanged(n) => {
            paint(&format!("      ~ 字段 {n} 描述变更\n"), YELLOW, color)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::{EndpointChange, EndpointDiff, ModelChange, ModelDiff, Rename};

    #[test]
    fn empty_diff() {
        let d = IrDiff::default();
        assert_eq!(render(&d, false), "本次无变更\n");
    }

    #[test]
    fn renders_summary_and_detail_plain() {
        let d = IrDiff {
            endpoints: EndpointDiff {
                added: vec!["post_new".into()],
                removed: vec!["get_old".into()],
                modified: vec![EndpointChange {
                    name: "get_x".into(),
                    changes: vec![Change::Return { from: "bool".into(), to: "int".into() }],
                }],
                renamed: vec![Rename { from: "a".into(), to: "b".into() }],
            },
            models: ModelDiff {
                added: vec![],
                removed: vec![],
                modified: vec![ModelChange {
                    name: "OrderVo".into(),
                    fields: vec![FieldChange::Added("remark".into())],
                }],
                renamed: vec![],
            },
        };
        let out = render(&d, false);
        assert!(out.contains("接口: +1 ~1 -1 ↻1"));
        assert!(out.contains("↻ 重命名接口 a → b"));
        assert!(out.contains("+ 新增接口 post_new"));
        assert!(out.contains("~ 修改接口 get_x"));
        assert!(out.contains("返回类型: bool → int"));
        assert!(out.contains("- 删除接口 get_old"));
        assert!(out.contains("~ 修改模型 OrderVo"));
        assert!(out.contains("+ 字段 remark"));
        // 纯文本无 ANSI
        assert!(!out.contains('\x1b'));
    }

    #[test]
    fn color_adds_ansi() {
        let d = IrDiff {
            endpoints: EndpointDiff { added: vec!["x".into()], ..Default::default() },
            models: ModelDiff::default(),
        };
        let out = render(&d, true);
        assert!(out.contains('\x1b'));
    }
}
