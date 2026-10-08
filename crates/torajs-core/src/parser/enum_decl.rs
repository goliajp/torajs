//! `enum E { A, B = 1, C = A | B }` — the TypeScript enum declaration.
//!
//! An enum is two things the rest of the compiler already knows: a
//! type alias (`E` annotates a number, or a string for a string enum)
//! and a value (`E` is an object holding the members and, for numeric
//! members, the reverse mapping `E[0] === "A"`). The parser assembles
//! exactly those — a `TypeDecl` flushed through the top-level synth
//! lane, and a `const E = { … }` whose `type_ann` carries the
//! `__enum__` marker so `desugar_enums` can fold member reads to the
//! constants and drop the object when nothing reads it as a value.
//!
//! Member values that evaluate at compile time are stored as
//! constants (`enum_const_eval`); anything else is a computed member:
//! its initializer is hoisted into a `const` in front of the
//! declaration so it evaluates once and serves both as the member's
//! value and as the reverse mapping's key.
//!
//! A later declaration of the same name in the same block merges into
//! the first: its members continue the first one's scope, and it
//! becomes a marker call holding the new members' object literal
//! (`ENUM_MERGE_ANN`), which `desugar_enums` turns into property
//! writes on the first object at this point of the program. `declare enum` emits the type alias
//! only — its members have no runtime value.

use super::enum_const_eval::{EnumConst, EnumValue, const_eval};
use super::*;
use crate::ast::{ENUM_MERGE_ANN, ENUM_OBJECT_ANN, PropKey};

#[derive(Default)]
pub(super) struct EnumCtx {
    pub(super) states: Vec<EnumState>,
    pub(super) type_names: std::collections::HashSet<String>,
    /// Set while parsing the declaration behind `export`.
    pub(super) in_export_inner: bool,
}

pub(super) struct EnumState {
    name: String,
    objlit: ExprId,
    members: Vec<(PropKey, EnumValue)>,
    /// Token index just past the declaration's `}`.
    end_pos: usize,
}

impl<'a> Parser<'a> {
    /// `enum E` / `const enum E` / `declare enum E` / `declare const
    /// enum E` statement heads. `enum` is a reserved word, so a name
    /// after it is unambiguous; `declare` is only a modifier with the
    /// keyword on the same line.
    pub(super) fn try_parse_enum_head(&mut self) -> Option<Result<Stmt, String>> {
        let tok = |p: &Self, i: usize| p.tokens.get(p.pos + i).map(|t| &t.token);
        let is_word = |t: Option<&Token>, w: &str| matches!(t, Some(Token::Ident(s)) if s == w);
        let (skip, is_declare) = if is_word(Some(self.peek()), "enum") {
            (0, false)
        } else if matches!(self.peek(), Token::Const) && is_word(tok(self, 1), "enum") {
            (1, false)
        } else if is_word(Some(self.peek()), "declare") && !self.has_newline_before(self.pos + 1) {
            if is_word(tok(self, 1), "enum") {
                (1, true)
            } else if matches!(tok(self, 1), Some(Token::Const)) && is_word(tok(self, 2), "enum") {
                (2, true)
            } else {
                return None;
            }
        } else {
            return None;
        };
        if !matches!(tok(self, skip + 1), Some(Token::Ident(_))) {
            return None;
        }
        let start = self.pos;
        self.pos += skip;
        Some(self.parse_enum_decl(start, is_declare))
    }

    /// Cursor on the type name `E`: in `E.M` naming a member of enum
    /// `E`, the member's type is the enum's own, so the `.M` tokens
    /// (2) are skipped and the annotation reads `E`.
    pub(super) fn enum_member_type_len(&self, name: &str) -> usize {
        let tok = |i: usize| self.tokens.get(self.pos + i).map(|t| &t.token);
        let member = matches!(tok(1), Some(Token::Dot)) && matches!(tok(2), Some(Token::Ident(_)));
        if member && self.enums.type_names.contains(name) {
            2
        } else {
            0
        }
    }

    /// The declaration behind `export`, parsed with the flag that
    /// `enum_exported` reads.
    pub(super) fn parse_export_inner_stmt(&mut self) -> Result<Stmt, String> {
        let was = std::mem::replace(&mut self.enums.in_export_inner, true);
        let inner = self.parse_stmt();
        self.enums.in_export_inner = was;
        inner
    }

    /// The declaration right behind a module-level `export`.
    pub(super) fn enum_exported(&self) -> bool {
        self.enums.in_export_inner && self.stmt_depth == 2
    }

    /// Where the statements an enum declaration needs in front of it
    /// go. Behind `export` that is the top-level lane: the local lane
    /// would wrap them into the export with the declaration, and only
    /// the declaration is what is exported.
    fn enum_prelude(&mut self) -> &mut Vec<Stmt> {
        if self.enum_exported() {
            &mut self.synth_classes
        } else {
            &mut self.synth_classes_local
        }
    }

    /// The earlier declaration of `name` in the block the cursor is
    /// in: no `}` between its end and `at` closes a block it was in.
    fn enum_merge_target(&self, name: &str, at: usize) -> Option<usize> {
        self.enums.states.iter().rposition(|s| {
            s.name == name
                && s.end_pos <= at
                && self.tokens[s.end_pos..at]
                    .iter()
                    .try_fold(0i32, |depth, t| {
                        let depth = match t.token {
                            Token::LBrace => depth + 1,
                            Token::RBrace => depth - 1,
                            _ => depth,
                        };
                        (depth >= 0).then_some(depth)
                    })
                    == Some(0)
        })
    }

    /// Cursor on `enum`; `const` / `declare` were consumed by the
    /// caller, which passes the statement's first token in `start`.
    pub(super) fn parse_enum_decl(
        &mut self,
        start: usize,
        is_declare: bool,
    ) -> Result<Stmt, String> {
        self.pos += 1; // consume `enum`
        let name = match self.peek() {
            Token::Ident(n) => n.clone(),
            t => {
                return Err(format!(
                    "expected enum name after `enum`, got {t:?} at {}",
                    self.at()
                ));
            }
        };
        self.pos += 1;
        match self.peek() {
            Token::LBrace => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `{{` to begin enum body, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        let existing = if is_declare {
            None
        } else {
            self.enum_merge_target(&name, start)
        };
        let mut members: Vec<(PropKey, EnumValue)> = existing
            .map(|i| self.enums.states[i].members.clone())
            .unwrap_or_default();
        let first_new = members.len();
        // A merged declaration must initialize its first member
        // (TS2432); the auto-increment chain does not cross it.
        let mut next: Option<f64> = if existing.is_some() { None } else { Some(0.0) };
        let mut hoisted: Vec<Stmt> = Vec::new();
        while !matches!(self.peek(), Token::RBrace) {
            let key = self.parse_enum_member_name()?;
            let value = if matches!(self.peek(), Token::Eq) {
                self.pos += 1;
                let init = self.parse_assign()?;
                match const_eval(&self.ast, init, &members, &name) {
                    Some(c) => EnumValue::Const(c),
                    None => {
                        let tmp = format!("__enum_{name}_{}", self.mint_desugar_id());
                        self.resolve_member_refs(init, &members, &name);
                        hoisted.push(Stmt::LetDecl {
                            mutable: false,
                            name: tmp.clone(),
                            type_ann: None,
                            init,
                            is_var: false,
                        });
                        EnumValue::Computed(tmp)
                    }
                }
            } else {
                match next {
                    Some(n) => EnumValue::Const(EnumConst::Num(n)),
                    None => {
                        return Err(format!(
                            "type error: enum member `{}` of `{name}` must have an initializer (TS1061) at {}",
                            key.lossy(),
                            self.at()
                        ));
                    }
                }
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
        self.pos += 1; // consume `}`
        if matches!(self.peek(), Token::Semi) {
            self.pos += 1;
        }
        self.register_enum_type(&name, &members);
        if is_declare {
            return Ok(Stmt::Block(Vec::new()));
        }
        self.enum_prelude().extend(hoisted);
        let fields = self.enum_object_fields(&members[first_new..]);
        let objlit = self.ast.add_expr(Expr::ObjectLit { fields });
        if let Some(i) = existing {
            let state = &mut self.enums.states[i];
            state.members = members;
            state.end_pos = self.pos;
            let primary = f64::from(state.objlit.0);
            let callee = self.ast.add_expr(Expr::Ident(ENUM_MERGE_ANN.to_string()));
            let target = self.ast.add_expr(Expr::Ident(name));
            let primary = self.ast.add_expr(Expr::Number(primary));
            let args = vec![target, primary, objlit];
            let call = self.ast.add_expr(Expr::Call { callee, args });
            // The first declaration already exports `name`.
            self.enum_prelude().push(Stmt::Expr(call));
            return Ok(Stmt::Block(Vec::new()));
        }
        self.enums.states.push(EnumState {
            name: name.clone(),
            objlit,
            members,
            end_pos: self.pos,
        });
        Ok(Stmt::LetDecl {
            mutable: false,
            name,
            type_ann: Some(ENUM_OBJECT_ANN.to_string()),
            init: objlit,
            is_var: false,
        })
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
