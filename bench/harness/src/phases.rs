//! Compile-phase timings: one extra, untimed compile with the runner's
//! phase-timing switch set, its `phase <indent><label> <ms>` stderr
//! lines folded into `label` (top level) and `parent/label` (one level
//! down) keys. A nested phase prints before its parent, so children
//! wait in a per-depth list until the parent's line names them.

use std::collections::BTreeMap;

use crate::exec::{PER_EXEC_TIMEOUT, spawn_output_bounded};
use crate::runner::split_cmd;

pub(crate) fn compile_phases(compile_cmd: &str, switch: &str) -> Option<BTreeMap<String, f64>> {
    let parts = split_cmd(compile_cmd);
    let args: Vec<&str> = parts.get(1..)?.iter().map(String::as_str).collect();
    let env = [(switch.to_string(), "1".to_string())];
    let out = spawn_output_bounded(parts.first()?, &args, &env, PER_EXEC_TIMEOUT).ok()?;
    if !out.status.success() {
        return None;
    }
    let phases = fold(&String::from_utf8_lossy(&out.stderr));
    (!phases.is_empty()).then_some(phases)
}

fn fold(stderr: &str) -> BTreeMap<String, f64> {
    let mut out = BTreeMap::new();
    let mut pending: Vec<Vec<(String, f64)>> = Vec::new();
    for line in stderr.lines() {
        let Some(rest) = line.strip_prefix("phase ") else {
            continue;
        };
        let depth = (rest.len() - rest.trim_start().len()) / 2;
        let mut it = rest.split_whitespace();
        let (Some(label), Some(ms), None) = (it.next(), it.next(), it.next()) else {
            continue;
        };
        let Ok(ms) = ms.parse::<f64>() else {
            continue;
        };
        if pending.len() < depth + 2 {
            pending.resize(depth + 2, Vec::new());
        }
        let children = std::mem::take(&mut pending[depth + 1]);
        pending.truncate(depth + 1);
        if depth == 0 {
            *out.entry(label.to_string()).or_insert(0.0) += ms;
            for (child, cms) in children {
                *out.entry(format!("{label}/{child}")).or_insert(0.0) += cms;
            }
        } else {
            pending[depth].push((label.to_string(), ms));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::fold;

    #[test]
    fn children_attach_to_the_parent_printed_after_them() {
        let got = fold(
            "phase   lex 0.10\nphase   check 0.50\nphase lower_to_ssa 1.00\n\
             phase     reach 2.00\nphase   link_a 3.00\nphase   EGRAPH 0.25\n\
             phase   EGRAPH 0.25\nphase egraph 4.00\nwarning: x\n",
        );
        assert_eq!(got["lower_to_ssa"], 1.0);
        assert_eq!(got["lower_to_ssa/check"], 0.5);
        assert_eq!(got["egraph"], 4.0);
        assert_eq!(got["egraph/EGRAPH"], 0.5);
        assert_eq!(got["egraph/link_a"], 3.0);
        assert!(!got.keys().any(|k| k.contains("reach")));
        assert_eq!(got.len(), 6);
    }
}
