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

use crate::check::{Checker, Severity, Type};

/// 一次编译的类型面三分计数。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TypedCensus {
    /// 定出了具体类型的表达式 —— 类型参与了 codegen。
    pub(crate) typed: u64,
    /// 定出 `Type::Any` 的表达式 —— 类型被放弃到运行期。
    pub(crate) any: u64,
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
        let scored = self.typed + self.any;
        (scored > 0).then(|| self.typed as f64 / scored as f64)
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
            "torajs-typed-census: typed={} any={} refused={} scored={} typed_ratio={}",
            self.typed,
            self.any,
            self.refused,
            self.typed + self.any,
            ratio,
        )
    }
}

impl Checker {
    /// 数出本次编译的三分计数。
    pub(crate) fn typed_census(&self) -> TypedCensus {
        let mut census = TypedCensus::default();
        for ty in self.expr_types.values() {
            if matches!(ty, Type::Any) {
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

    /// 在 `TORAJS_TYPED_CENSUS=1` 下打一行读数到 stderr。
    ///
    /// 调用点在 `check_entry::check_with_arity_warn`,紧跟
    /// `run_full_pipeline` 之后、错误分支提前返回**之前** —— 一次被
    /// 拒的编译同样要有读数,否则 `refused` 永远是 0。
    pub(crate) fn emit_typed_census(&self) {
        if std::env::var("TORAJS_TYPED_CENSUS").as_deref() != Ok("1") {
            return;
        }
        eprintln!("{}", self.typed_census().report_line());
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
            refused: 0,
        };
        assert_eq!(census.typed_ratio(), Some(0.0));
        assert!(census.report_line().contains("typed_ratio=0.0000"));
    }
}
