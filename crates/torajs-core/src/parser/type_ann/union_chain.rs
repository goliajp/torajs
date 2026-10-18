//! Union type annotations — `A | B | ...` (TS §3.4 union types).
//!
//! The flat annotation string already spends `|` as a separator inside
//! `__inlobj(..)`, `__fn(..)` and generic argument lists, so a union
//! gets a marker of its own: `__union(A|B)`. Every decoder splits it
//! with the depth-aware `split_top_pipe`, which keeps a member's own
//! `|` (an inline object's fields, a generic's arguments) intact.
//!
//! The spelling is canonical: members are flattened, deduplicated and
//! sorted, so `string | number` and `number | string` are the same
//! annotation (annotations compare as strings everywhere downstream).
//! The nullish members fold into the `__nullable(..)` wrapper the
//! `T | null` shape has always used, `any` / `unknown` absorb the
//! whole union, and a single remaining member is that member.

use super::*;
use crate::check_type_ann::split_top_pipe;

impl Parser<'_> {
    /// A trailing `| member` chain after an already-read member.
    ///
    /// The rest of the chain is read as one annotation and combined,
    /// which flattens it: the combine step takes nested markers apart
    /// again. The rest is read through the unrecorded inner reader so
    /// the arrow-return context (`type_ann_depth`) of the whole union
    /// is the one its first member saw.
    pub(super) fn consume_union_suffix(&mut self, first: String) -> Result<String, String> {
        if !matches!(self.peek(), Token::Pipe) {
            return Ok(first);
        }
        self.pos += 1;
        let rest = self.parse_type_ann_inner()?;
        Ok(union_combine(&[first.as_str(), rest.as_str()]))
    }

    /// Whether the `{` at the cursor opens an object type that is only
    /// part of a longer type: the first member of a union (`{ .. } |
    /// ..`, `{ .. }[] | ..`) or the element of an array type (`{ ..
    /// }[]`), so a type alias must read its right side as one
    /// annotation.
    pub(in crate::parser) fn brace_type_is_operand(&self) -> bool {
        let mut depth = 0usize;
        let mut i = self.pos;
        while let Some(t) = self.tokens.get(i) {
            match t.token {
                Token::LBrace => depth += 1,
                Token::RBrace => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
            i += 1;
        }
        i += 1;
        let body_end = i;
        while matches!(self.tokens.get(i).map(|t| &t.token), Some(Token::LBracket))
            && matches!(
                self.tokens.get(i + 1).map(|t| &t.token),
                Some(Token::RBracket)
            )
        {
            i += 2;
        }
        i > body_end || matches!(self.tokens.get(i).map(|t| &t.token), Some(Token::Pipe))
    }
}

/// The canonical annotation of the union of `parts`.
pub(crate) fn union_combine(parts: &[&str]) -> String {
    let mut acc = UnionAcc::default();
    for p in parts {
        acc.add(p);
    }
    acc.finish()
}

#[derive(Default)]
struct UnionAcc {
    members: Vec<String>,
    first_nullish: Option<&'static str>,
    top: Option<&'static str>,
}

impl UnionAcc {
    fn add(&mut self, ann: &str) {
        match ann {
            "null" => self.nullish("null"),
            "undefined" => self.nullish("undefined"),
            "any" => self.top = Some("any"),
            "unknown" => {
                if self.top.is_none() {
                    self.top = Some("unknown");
                }
            }
            _ => {
                if let Some(inner) = marker_inner(ann, "__nullable(") {
                    self.nullish("undefined");
                    self.add(inner);
                } else if let Some(inner) = marker_inner(ann, "__union(") {
                    for m in split_top_pipe(inner) {
                        self.add(m);
                    }
                } else if !self.members.iter().any(|m| m == ann) {
                    self.members.push(ann.to_string());
                }
            }
        }
    }

    fn nullish(&mut self, name: &'static str) {
        if self.first_nullish.is_none() {
            self.first_nullish = Some(name);
        }
    }

    fn finish(mut self) -> String {
        if let Some(top) = self.top {
            return top.to_string();
        }
        self.members.sort();
        let core = match self.members.len() {
            0 => {
                // only nullish members: `null | undefined` keeps the
                // spelling the two-member form always produced
                return match self.first_nullish {
                    Some(n) => format!("__nullable({n})"),
                    None => "never".to_string(),
                };
            }
            1 => self.members.pop().expect("one member"),
            _ => format!("__union({})", self.members.join("|")),
        };
        if self.first_nullish.is_some() {
            format!("__nullable({core})")
        } else {
            core
        }
    }
}

/// The text inside `prefix ... )` when the parenthesis that `prefix`
/// opens is the one that closes the whole annotation.
fn marker_inner<'a>(ann: &'a str, prefix: &str) -> Option<&'a str> {
    let inner = ann.strip_prefix(prefix)?.strip_suffix(')')?;
    let mut depth: i32 = 0;
    for b in inner.bytes() {
        match b {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth < 0 {
                    return None;
                }
            }
            _ => {}
        }
    }
    (depth == 0).then_some(inner)
}

#[cfg(test)]
mod tests {
    use super::union_combine;

    #[test]
    fn canonical_order_and_dedup() {
        assert_eq!(
            union_combine(&["string", "number"]),
            "__union(number|string)"
        );
        assert_eq!(
            union_combine(&["number", "__union(number|string)"]),
            "__union(number|string)"
        );
    }

    #[test]
    fn nullish_members_fold_into_nullable() {
        assert_eq!(
            union_combine(&["Node", "__nullable(undefined)"]),
            "__nullable(Node)"
        );
        assert_eq!(union_combine(&["null", "number"]), "__nullable(number)");
        assert_eq!(
            union_combine(&["string", "__nullable(number)"]),
            "__nullable(__union(number|string))"
        );
        assert_eq!(union_combine(&["null", "undefined"]), "__nullable(null)");
    }

    #[test]
    fn top_types_absorb() {
        assert_eq!(union_combine(&["any", "number"]), "any");
        assert_eq!(union_combine(&["unknown", "__nullable(number)"]), "unknown");
    }

    #[test]
    fn member_with_its_own_pipes_stays_whole() {
        assert_eq!(
            union_combine(&["__inlobj(a:number|b:string)", "string"]),
            "__union(__inlobj(a:number|b:string)|string)"
        );
        assert_eq!(
            union_combine(&["__nullable(x)[]", "y"]),
            "__union(__nullable(x)[]|y)"
        );
    }
}
