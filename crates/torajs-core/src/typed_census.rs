//! typed 覆盖率量具 —— design principles §7「类型参与 codegen 不可
//! 交易」的唯一机械证据,roadmap P-SURF 的 S0.2。
//!
//! 量的是:一次编译里,每个被 checker 定过型的表达式落进哪一类 ——
//! `typed`(定出了具体类型,类型参与了 codegen)还是 `any`(类型被
//! 放弃,运行期再判)。torajs 占的那一格要求两样都不放弃,而**收窄
//! 语义**与**交出类型**这两个方向分别通向已被占住的另外两格;这个
//! 尺子量的就是后一个方向有没有在滑。
//!
//! 为什么它需要自己的尺子而不能看 test262 簇数:S0 的 366 条里 **76%
//! 藏在 ≤3 条的残尾里**,gate 指标只露出 24% —— 方向判错在那个数上
//! 几乎看不见。
//!
//! # 读数怎么来
//!
//! [`crate::check::Checker::expr_types`] 已经记着每次成功 `type_of`
//! 的结果(`check/type_of.rs:21` 为 LSP hover 而建),每个 `ExprId` 一个
//! 答案。数它即可 —— **不需要新的计数字段**,也因此不触发
//! design principles §6.2 的 global-state 条款:这里根本没有新状态。
//! 形态上仍与 `torajs-egraph` 的 `*Stats`(`sext_elide.rs:45`)同族:
//! 普通 owned struct,返回值带出,不引 global。
//!
//! # 这个数会说谎的三种方式(读它之前必须知道)
//!
//! 1. **分母是 checker 走过的表达式,不是源码表达式。** lowering 自己
//!    合成的 any 装箱 checker 从没见过,不在分母里 ——
//!    `ssa_lower_any_box.rs` 的注释举了 for-of 协议的例子("an
//!    expression the checker never visited")。
//! 2. **`refused` 计的是 Error 级 diagnostic 条数,不是表达式数。**
//!    一个表达式可能产生多条。它是辅助信号,主信号是 typed/any 比值。
//! 3. **跨 commit 比比值时分母会漂。** desugar 与单态化会改变表达式
//!    总数,一次 desugar 重构就能让百分比动而语言能力没变。所以三个
//!    绝对数与比值**并列**报,比值只在同一份语料 + 相邻 commit 之间读。
//!
//! 第 3 条是这个量具最容易骗人的地方,与 perf 方法论「测量装置的失效
//! 长得跟数据一模一样」同族。
//!
//! 不计的表达式:receiver-polymorphic 孪生体(`__cmany_` / `__smany_` /
//! `__ctorany_`)的 body。它们是编译器自己克隆的方法体、参数全改
//! `any`,同一份源码表达式在 mono 里已经计过一次;再计一次只会把
//! 「多铸了一个孪生体」读成 typed 面缩小。见 `ast::twin_body_exprs`。

use std::collections::HashSet;

use crate::ast::{Ast, ExprId};
use crate::check::{Checker, Severity, Type};

/// 一次编译的类型面三分计数。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TypedCensus {
    /// 定出了具体类型的表达式 —— 类型参与了 codegen。
    pub(crate) typed: u64,
    /// 定出 `Type::Any` 的表达式 —— 类型被放弃到运行期。
    pub(crate) any: u64,
    /// Expressions the checker typed as a union, whose value rides the
    /// any lane: the members took part in the checks but not in
    /// codegen, so they count as neither typed nor plain any.
    pub(crate) union: u64,
    /// Expressions the checker typed as a dictionary (an index
    /// signature / `Record`): the value type checks the writes, the
    /// value rides the any lane. Scored like `union`, counted apart.
    pub(crate) dict: u64,
    /// Error 级 diagnostic 条数(见模块 doc 第 2 条,**不是**表达式数)。
    pub(crate) refused: u64,
}

impl TypedCensus {
    /// typed 占已定型表达式的比例。
    ///
    /// **分母为 0 时答 `None`,不答 1.0** —— 一个空语料、或一个打点
    /// 失效的编译,必须能被下游认出来是"没有读数"而不是"覆盖率
    /// 100%"。这是 `test/instrument-must-be-able-to-fail` 在这个量具
    /// 上的具体形式:量具要能红。
    pub(crate) fn typed_ratio(&self) -> Option<f64> {
        let scored = self.scored();
        (scored > 0).then(|| self.typed as f64 / scored as f64)
    }

    /// Every typed expression, all three buckets.
    pub(crate) fn scored(&self) -> u64 {
        self.typed + self.any + self.union + self.dict
    }

    /// 一行机器可读的读数,给 `hardev/autorun/` 的采集脚本解析。
    /// 协议照 `torajs-egraph/src/lib.rs` 的 `*_STATS` 开关。
    pub(crate) fn report_line(&self) -> String {
        let ratio = match self.typed_ratio() {
            Some(r) => format!("{r:.4}"),
            // 采集脚本按这个字面量判"无读数",不要改成 0 或 -1 ——
            // 那两个都会被读成一个真实的低覆盖率。
            None => "none".to_string(),
        };
        format!(
            "torajs-typed-census: typed={} any={} union={} dict={} refused={} scored={} typed_ratio={}",
            self.typed,
            self.any,
            self.union,
            self.dict,
            self.refused,
            self.scored(),
            ratio,
        )
    }
}

impl Checker {
    /// 数出本次编译的三分计数,`skip` 里的表达式不计(孪生体 body)。
    pub(crate) fn typed_census(&self, skip: &HashSet<ExprId>) -> TypedCensus {
        let mut census = TypedCensus::default();
        for (eid, ty) in &self.expr_types {
            if skip.contains(eid) {
                continue;
            }
            // a union whose literals widen to one type travels typed
            if let Some(t) = self.unions.types.get(eid)
                && matches!(ty, Type::Any)
            {
                match t {
                    Type::Dict(..) => census.dict += 1,
                    _ => census.union += 1,
                }
            } else if matches!(ty, Type::Any) {
                census.any += 1;
            } else {
                census.typed += 1;
            }
        }
        census.refused = self
            .errors
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .count() as u64;
        census
    }

    /// `TORAJS_TYPED_CENSUS=1` 下的一行读数,没开时为 `None`。
    ///
    /// 在 `check_entry::check_with_arity_warn` 里紧跟 `run_full_pipeline`
    /// 之后算出 —— 一次被拒的编译同样要有读数,否则 `refused` 永远是
    /// 0。打印由调用方决定:要带着生成器孪生请求重编的那一轮不打,
    /// 每次编译只打最后一轮的一行。
    ///
    /// `TORAJS_TYPED_CENSUS=any` prints the same line, then one
    /// `torajs-typed-census-any: <start> <end> <expr>` line per expression
    /// in the `any` bucket, in source order, so a drop in the ratio can
    /// be read site by site (`<expr>` is the node's debug form, cut at 80
    /// characters). Span 0 0 means the node carries no span: desugared
    /// and injected nodes, but also every identifier and assignment the
    /// parser adds, so it does not by itself tell injected code apart.
    pub(crate) fn typed_census_line(&self, ast: &Ast) -> Option<String> {
        let mode = std::env::var("TORAJS_TYPED_CENSUS").ok()?;
        if mode != "1" && mode != "any" {
            return None;
        }
        let skip = crate::ast::twin_body_exprs(ast);
        let mut line = self.typed_census(&skip).report_line();
        if mode == "any" {
            line.push_str(&self.any_sites(ast, &skip));
        }
        Some(line)
    }

    /// The `any` bucket of [`Self::typed_census`], one line per site.
    fn any_sites(&self, ast: &Ast, skip: &HashSet<ExprId>) -> String {
        let mut sites: Vec<(u32, u32, String)> = self
            .expr_types
            .iter()
            .filter(|(eid, ty)| {
                !skip.contains(eid)
                    && matches!(ty, Type::Any)
                    && !self.unions.types.contains_key(eid)
            })
            .map(|(eid, _)| {
                let span = ast.expr_spans.get(eid.0 as usize).copied();
                let (start, end) = span.map_or((0, 0), |s| (s.start, s.end));
                let mut kind = format!("{:?}", ast.exprs[eid.0 as usize]);
                if let Some((cut, _)) = kind.char_indices().nth(80) {
                    kind.truncate(cut);
                }
                (start, end, kind)
            })
            .collect();
        sites.sort();
        sites
            .iter()
            .map(|(start, end, kind)| format!("\ntorajs-typed-census-any: {start} {end} {kind}"))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_corpus_has_no_ratio_rather_than_full_coverage() {
        let census = TypedCensus::default();
        assert_eq!(census.typed_ratio(), None);
        assert!(census.report_line().contains("typed_ratio=none"));
    }

    #[test]
    fn ratio_counts_only_scored_expressions() {
        let census = TypedCensus {
            typed: 3,
            any: 1,
            union: 0,
            dict: 0,
            refused: 7,
        };
        assert_eq!(census.typed_ratio(), Some(0.75));
        // refused 不进分母:它计的是 diagnostic 条数,与表达式不同量纲。
        assert!(census.report_line().contains("scored=4"));
        assert!(census.report_line().contains("typed_ratio=0.7500"));
    }

    #[test]
    fn all_any_reads_as_zero_not_as_missing() {
        let census = TypedCensus {
            typed: 0,
            any: 5,
            union: 0,
            dict: 0,
            refused: 0,
        };
        assert_eq!(census.typed_ratio(), Some(0.0));
        assert!(census.report_line().contains("typed_ratio=0.0000"));
    }

    #[test]
    fn union_bucket_is_scored_but_not_typed() {
        let census = TypedCensus {
            typed: 2,
            any: 1,
            union: 1,
            dict: 1,
            refused: 0,
        };
        assert_eq!(census.typed_ratio(), Some(0.4));
        let line = census.report_line();
        assert!(line.contains("any=1 union=1 dict=1"));
        assert!(line.contains("scored=5"));
    }
}
