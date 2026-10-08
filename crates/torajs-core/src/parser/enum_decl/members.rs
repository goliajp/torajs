//! The members of one enum declaration: names, initializers, and the
//! values they get (a constant, a number counted up from the previous
//! member, or a hoisted temporary holding a computed value).

use super::super::*;
use super::const_eval::{EnumConst, EnumValue, EvalScope};
use crate::ast::PropKey;
use std::ops::Range;

/// One declaration's head, as the member loop needs it.
pub(super) struct EnumDecl {
    pub(super) name: String,
    pub(super) is_const: bool,
    /// An earlier declaration of a merged enum already omitted its
    /// first member's initializer.
    pub(super) omitted_before: bool,
    /// The whole declaration: names inside it bind nothing outside.
    pub(super) span: Range<usize>,
}

impl Parser<'_> {
    /// One declaration's members, appended to `members` (which holds
    /// the earlier declarations' members when merging): the hoisted
    /// computed initializers, and whether the first member has no
    /// initializer.
    pub(super) fn parse_enum_members(
        &mut self,
        d: &EnumDecl,
        members: &mut Vec<(PropKey, EnumValue)>,
    ) -> Result<(Vec<Stmt>, bool), String> {
        let name = &d.name;
        let first_new = members.len();
        let mut next: Option<f64> = Some(0.0);
        let mut hoisted: Vec<Stmt> = Vec::new();
        let mut omitted = false;
        while !matches!(self.peek(), Token::RBrace) {
            let key = self.parse_enum_member_name()?;
            let value = if matches!(self.peek(), Token::Eq) {
                self.pos += 1;
                let init = self.parse_assign()?;
                self.enum_member_init(d, init, members, &mut hoisted)?
            } else {
                if members.len() == first_new {
                    if d.omitted_before {
                        return Err(format!(
                            "type error: only one declaration of the merged enum `{name}` can omit the initializer of its first member (TS2432) at {}",
                            self.at()
                        ));
                    }
                    omitted = true;
                }
                let Some(n) = next else {
                    return Err(format!(
                        "type error: enum member `{}` of `{name}` must have an initializer (TS1061) at {}",
                        key.lossy(),
                        self.at()
                    ));
                };
                EnumValue::Const(EnumConst::Num(n))
            };
            next = match &value {
                EnumValue::Const(EnumConst::Num(n)) => Some(n + 1.0),
                _ => None,
            };
            members.push((key, value));
            match self.peek() {
                Token::Comma => self.pos += 1,
                Token::RBrace => {}
                t => {
                    return Err(format!(
                        "expected `,` or `}}` in enum body, got {t:?} at {}",
                        self.at()
                    ));
                }
            }
        }
        Ok((hoisted, omitted))
    }

    /// A member's value: the constant its initializer evaluates to, or
    /// (outside a const enum) the hoisted temporary computing it.
    fn enum_member_init(
        &mut self,
        d: &EnumDecl,
        init: ExprId,
        members: &[(PropKey, EnumValue)],
        hoisted: &mut Vec<Stmt>,
    ) -> Result<EnumValue, String> {
        let scope = EvalScope {
            members,
            enum_name: &d.name,
            at: self.pos,
            skip: d.span.clone(),
        };
        if let Some(c) = self.enum_const_eval(init, &scope) {
            return Ok(EnumValue::Const(c));
        }
        if d.is_const {
            return Err(format!(
                "type error: member initializers of const enum `{}` must be constant expressions (TS2474) at {}",
                d.name,
                self.at()
            ));
        }
        let tmp = format!("__enum_{}_{}", d.name, self.mint_desugar_id());
        self.resolve_member_refs(init, members, &d.name);
        hoisted.push(Stmt::LetDecl {
            mutable: false,
            name: tmp.clone(),
            type_ann: None,
            init,
            is_var: false,
        });
        Ok(EnumValue::Computed(tmp))
    }

    fn parse_enum_member_name(&mut self) -> Result<PropKey, String> {
        let key = match self.peek() {
            Token::Ident(n) => PropKey::from(n),
            Token::String(s) => PropKey::from(s.clone()),
            t => match Self::keyword_property_name(t) {
                Some(kw) => PropKey::from(kw),
                None => {
                    return Err(format!(
                        "expected enum member name, got {t:?} at {}",
                        self.at()
                    ));
                }
            },
        };
        self.pos += 1;
        Ok(key)
    }
}
