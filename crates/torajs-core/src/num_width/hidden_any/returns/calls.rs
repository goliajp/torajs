//! What a call the return walk of `hidden_any::returns` reads answers.

use std::collections::HashSet;

use super::kinds::Kind;
use super::receivers::Receiver;
use super::{Read, builtins};
use crate::ast::{Expr, ExprId};
use crate::check::Type;
use crate::num_width::hidden_any::{Graph, Node};
use crate::num_width::{Analysis, Scope};

impl Graph<'_> {
    /// [`Graph::spelled_any`] for call `e` of `callee`: the returns of
    /// the functions in sight it runs go to `from`; a call the walk does
    /// not follow answers what its callee declares.
    pub(super) fn call_spelled_any(
        &self,
        (a, sc): (&Analysis<'_>, &Scope),
        (e, callee): (ExprId, ExprId),
        read: Read,
        from: &mut Vec<Node>,
        seen: &mut HashSet<(ExprId, Read)>,
    ) -> bool {
        let declared = match read {
            Read::Value => self.lookups(a).declared(sc.fn_name),
            Read::Members => None,
        };
        let differs = |k: Kind| self.differs((sc.fn_name, read), declared.as_ref(), k);
        match self.callees(a, sc, e, callee) {
            Some(rets) => {
                let l = self.lookups(a);
                let open = (rets.iter()).any(|n| matches!(n, Node::RetAny(f) if l.returns_open(f)));
                self.hands_on((sc.fn_name, read), &rets);
                from.extend(rets);
                open
            }
            // a binding typed with a typed return answers it, though
            // the checker typed the call `any` (a variadic value's
            // boxed entry); a method answers what its declaration
            // does; any other call the walk does not follow answers
            // what its callee declares, which the checker typed `any`
            None => match (
                self.typed_callee(a, sc, callee),
                a.ast.get_expr(e),
                a.ast.get_expr(callee),
            ) {
                (Some(r), ..) => differs(Kind::of(r)),
                (
                    None,
                    Expr::Call { .. } | Expr::OptCall { .. },
                    Expr::Member { obj, name } | Expr::OptChain { obj, name },
                ) => match a.expr_types.get(obj) {
                    // a method of a union of classes the checker typed
                    // `any`; a receiver whose classes the walk cannot
                    // name may run a method of any kind; a method of a
                    // union of builtins answers what its library
                    // declares for each arm (`v.toString()` a string)
                    Some(Type::Any) => match self.receiver(a, sc, *obj) {
                        Receiver::Classes(u) => {
                            let at = from.len();
                            let hit = self.method_spelled_any(
                                a,
                                (Some(&u), name),
                                declared.as_ref(),
                                from,
                            );
                            self.hands_on((sc.fn_name, read), &from[at..]);
                            hit | self.spelled_any((a, sc), *obj, Read::Members, from, seen)
                        }
                        Receiver::Builtin(arms) => self.differs_any(
                            (sc.fn_name, read),
                            declared.as_ref(),
                            builtins::member(&arms, name, true),
                        ),
                        Receiver::Traced => {
                            self.spelled_any((a, sc), *obj, Read::Members, from, seen)
                        }
                        Receiver::Unknown => true,
                    },
                    recv @ Some(Type::ClassRef(_) | Type::Union(_) | Type::Nullable(_)) => {
                        let at = from.len();
                        let hit = self.method_spelled_any(a, (recv, name), declared.as_ref(), from);
                        self.hands_on((sc.fn_name, read), &from[at..]);
                        hit
                    }
                    // every method of `Math` answers a number
                    // (§21.3), whatever its arguments hold
                    Some(Type::Object("Math")) => differs(Kind::Number),
                    // another library method the checker types
                    // `any` answers what its library declares,
                    // whatever its arguments hold (`JSON.parse(s)`,
                    // `m.get(k)`)
                    _ => true,
                },
                _ => true,
            },
        }
    }
}
