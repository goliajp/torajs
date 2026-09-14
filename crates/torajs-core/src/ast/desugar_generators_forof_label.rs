//! Labels around a rewritten for-of (`desugar_generators_forof`,
//! RFC 20260915): the loop becomes `Multi[decls…, Try { While }]`
//! (or a bare While for a delegation protocol), and a label the
//! parser sank onto the loop (`parse_stmt::label_hoisted_for_of`)
//! must end up on the While again so the machine sees a labeled
//! LOOP (`continue label` needs one), not a labeled block. Split out
//! when the parent crossed the file-size line.

use super::Stmt;

/// The tail statement a rewritten for-of ends in: the While itself, or
/// the close wrap's Try holding it.
pub(super) fn is_loop_tail(s: &Stmt) -> bool {
    match s {
        Stmt::While { .. } => true,
        Stmt::Try { body, .. } => matches!(body.as_slice(), [Stmt::While { .. }]),
        _ => false,
    }
}

/// Put `label` on the While inside `tail` (see [`is_loop_tail`]).
pub(super) fn label_loop_tail(label: String, tail: Stmt) -> Stmt {
    match tail {
        Stmt::Try {
            mut body,
            had_catch,
            catch_param,
            catch_type,
            catch_body,
            finally_body,
        } => {
            let w = body.pop().expect("is_loop_tail matched a While");
            body.push(Stmt::Labeled {
                label,
                body: Box::new(w),
            });
            Stmt::Try {
                body,
                had_catch,
                catch_param,
                catch_type,
                catch_body,
                finally_body,
            }
        }
        w => Stmt::Labeled {
            label,
            body: Box::new(w),
        },
    }
}

/// The statement under a label chain — a labeled for-of whose source
/// was hoisted sits as `Labeled { ForOf }` right after its hoist
/// (`parse_stmt::label_hoisted_for_of`).
pub(super) fn unlabel(s: &Stmt) -> &Stmt {
    match s {
        Stmt::Labeled { body, .. } => unlabel(body),
        other => other,
    }
}
