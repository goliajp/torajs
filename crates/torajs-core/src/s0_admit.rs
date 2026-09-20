//! S0 的实测闸 —— 判据 2 的机械全量版。
//!
//! `docs/language-spec/ch1-type-runtime-boundary.md` §2 的判据 2 是
//! 「同一语义的 `any` 拼写今天已经跑得对 → 档 B」。它是四条判据里最便宜
//! 的一条,但手写探针一条一条问 366 条**判不动**:一条手写的 `any` 拼写
//! 只是被拒程序的**近似**,近似错了整档就错。
//!
//! 这个闸把同一个问题问得精确:`TORAJS_S0_ADMIT=1` 下,checker 的 Error
//! 级诊断**不再阻止编译**,原样降级成 warning 继续走 lowering。于是
//! test262 那 366 条被拒的 case **自己就是探针** —— 跑一次全量 sweep,
//! 每条的结局直接给出它的档位:
//!
//! | 闸开后的结局 | 档位 | 含义 |
//! |---|---|---|
//! | pass | **B** | 运行期语义早已正确,差别只在 checker 拦不拦。S0.4 落地即通过 |
//! | 仍 incompatible / bug | **L1** | 运行期语义本身有缺口 —— 不在 S0 范围,先修运行期 |
//! | 编译期仍拒 / lower 崩 | **S0.4 的工作面** | typed 车道没有这条路,要为它 emit 那次 throw |
//!
//! **闸只降级第一处** —— `check_with_arity_warn` 里主 pipeline 的那处。
//! monomorphization 之后的那处**不降级**:那里的诊断说的不是「这个程序
//! 错了」而是「这个程序无法 lower」(一个泛型调用没有 retarget),而
//! lowerer 对它的回答是一个点名 callee 的 panic。降级它只会把一条诊断
//! 换成一次崩溃,问不出任何档位。
//!
//! **这不是一条生产路径。** 闸关着时行为逐字不变(`admit()` 唯一的副作用
//! 是读一次环境变量);闸开着时产出的二进制没有任何保证 —— 它的用途是
//! 回答「运行期语义在不在」,不是运行程序。

/// `TORAJS_S0_ADMIT=1` 时答 true —— checker 的 Error 降级为 warning。
///
/// 与 `TORAJS_TYPED_CENSUS` 同协议:只认字面量 `1`,别的值(含
/// `true` / `yes` / 空串)一律当没开。一个拼错的开关必须表现为
/// **闸关着**,不能表现为闸开着 —— 后者会让一次本该失败的验证静默通过。
pub(crate) fn admit() -> bool {
    std::env::var("TORAJS_S0_ADMIT").as_deref() == Ok("1")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 这两条在同一个测试里,因为它们共享进程级环境变量 —— 分成两个
    /// `#[test]` 会在并行下互相看见对方设的值。
    #[test]
    fn only_the_literal_one_opens_the_gate() {
        // SAFETY: 单线程测试体内设/清同一个变量,没有别的线程读它。
        unsafe {
            std::env::remove_var("TORAJS_S0_ADMIT");
            assert!(!admit(), "unset must read as closed");

            for spelling in ["0", "true", "yes", "", "1 "] {
                std::env::set_var("TORAJS_S0_ADMIT", spelling);
                assert!(!admit(), "`{spelling}` must read as closed, not open");
            }

            std::env::set_var("TORAJS_S0_ADMIT", "1");
            assert!(admit(), "`1` must open the gate");

            std::env::remove_var("TORAJS_S0_ADMIT");
        }
    }
}
