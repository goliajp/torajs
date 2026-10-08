//! `enum E { A, B = 1, C = A | B }` — the TypeScript enum declaration.
//!
//! An enum is two things the rest of the compiler already knows: a
//! type alias (`E` annotates a number, or a string for a string enum)
//! and a value (`E` is an object holding the members and, for numeric
//! members, the reverse mapping `E[0] === "A"`). The parser assembles
//! exactly those — a `TypeDecl` flushed through the top-level synth
//! lane, and a `const E = { … }` whose `type_ann` carries the
//! `__enum__` marker (`__enum_const__` for a const enum) so
//! `desugar_enums` can fold member reads to the constants and drop
//! the object when nothing reads it as a value.
//!
//! Member values that evaluate at compile time are stored as
//! constants (`enum_const_eval`); anything else is a computed member:
//! its initializer is hoisted into a `const` in front of the
//! declaration so it evaluates once and serves both as the member's
//! value and as the reverse mapping's key. A const enum has no
//! computed members (TS2474).
//!
//! A later declaration of the same name in the same block merges into
//! the first: its members continue the first one's scope, and it
//! becomes a marker call holding the new members' object literal
//! (`ENUM_MERGE_ANN`), which `desugar_enums` turns into property
//! writes on the first object at this point of the program. A
//! `declare enum` emits the type alias only — its members have no
//! runtime value; a `declare const enum` is inlined like a const enum.

mod const_eval;
mod members;
mod object;
mod scope;

use super::*;
use crate::ast::{ENUM_CONST_ANN, ENUM_MERGE_ANN, ENUM_OBJECT_ANN, PropKey};
use const_eval::EnumValue;
use members::EnumDecl;
use scope::ConstBinding;

#[derive(Default)]
pub(super) struct EnumCtx {
    states: Vec<EnumState>,
    consts: Vec<ConstBinding>,
    /// Set while parsing the declaration behind `export`.
    in_export_inner: bool,
}

struct EnumState {
    name: String,
    /// The type alias the name annotates with: the name itself at the
    /// top level, a name of its own in a nested scope.
    alias: String,
    objlit: ExprId,
    members: Vec<(PropKey, EnumValue)>,
    /// Token index of the declaration's first token.
    start_pos: usize,
    /// Token index just past the declaration's `}`.
    end_pos: usize,
    /// Some declaration's first member has no initializer (TS2432
    /// allows one).
    first_omitted: bool,
    is_const: bool,
    is_declare: bool,
}

impl<'a> Parser<'a> {
    /// `enum E` / `const enum E` / `declare enum E` / `declare const
    /// enum E` statement heads. `enum` is a reserved word, so a name
    /// after it is unambiguous; `declare` is only a modifier with the
    /// keyword on the same line.
    pub(super) fn try_parse_enum_head(&mut self) -> Option<Result<Stmt, String>> {
        let tok = |p: &Self, i: usize| p.tokens.get(p.pos + i).map(|t| &t.token);
        let is_word = |t: Option<&Token>, w: &str| matches!(t, Some(Token::Ident(s)) if s == w);
        let declare =
            is_word(Some(self.peek()), "declare") && !self.has_newline_before(self.pos + 1);
        let at = usize::from(declare);
        let is_const = matches!(tok(self, at), Some(Token::Const));
        let skip = at + usize::from(is_const);
        if !is_word(tok(self, skip), "enum")
            || !matches!(tok(self, skip + 1), Some(Token::Ident(_)))
        {
            return None;
        }
        let start = self.pos;
        self.pos += skip;
        Some(self.parse_enum_decl(start, declare, is_const))
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
                && !s.is_declare
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
    fn parse_enum_decl(
        &mut self,
        start: usize,
        is_declare: bool,
        is_const: bool,
    ) -> Result<Stmt, String> {
        self.pos += 1; // consume `enum`
        let Token::Ident(name) = self.peek().clone() else {
            unreachable!("try_parse_enum_head saw the name");
        };
        self.pos += 1;
        if !matches!(self.peek(), Token::LBrace) {
            let t = self.peek();
            return Err(format!(
                "expected `{{` to begin enum body, got {t:?} at {}",
                self.at()
            ));
        }
        let span = start..self.group_end(self.pos);
        self.pos += 1;
        let existing = if is_declare {
            None
        } else {
            self.enum_merge_target(&name, start)
        };
        if let Some(i) = existing
            && self.enums.states[i].is_const != is_const
        {
            return Err(format!(
                "type error: enum declarations of `{name}` must all be const or non-const (TS2473; tsc 6 reports TS2567) at {}",
                self.at()
            ));
        }
        let mut members = existing
            .map(|i| self.enums.states[i].members.clone())
            .unwrap_or_default();
        let first_new = members.len();
        let decl = EnumDecl {
            name: name.clone(),
            is_const,
            omitted_before: existing.is_some_and(|i| self.enums.states[i].first_omitted),
            span,
        };
        let (hoisted, omitted) = self.parse_enum_members(&decl, &mut members)?;
        self.pos += 1; // consume `}`
        if matches!(self.peek(), Token::Semi) {
            self.pos += 1;
        }
        let fields = self.enum_object_fields(&members[first_new..]);
        let objlit = self.ast.add_expr(Expr::ObjectLit { fields });
        if let Some(i) = existing {
            let state = &mut self.enums.states[i];
            state.members = members;
            state.end_pos = self.pos;
            state.first_omitted |= omitted;
            self.update_enum_alias(i);
            self.enum_prelude().extend(hoisted);
            return Ok(self.enum_merge_call(i, objlit));
        }
        let alias = self.register_enum_type(&name, &members);
        self.enums.states.push(EnumState {
            name: name.clone(),
            alias,
            objlit,
            members,
            start_pos: start,
            end_pos: self.pos,
            first_omitted: omitted,
            is_const,
            is_declare,
        });
        if is_declare && !is_const {
            return Ok(Stmt::Block(Vec::new()));
        }
        self.enum_prelude().extend(hoisted);
        let marker = if is_const {
            ENUM_CONST_ANN
        } else {
            ENUM_OBJECT_ANN
        };
        Ok(Stmt::LetDecl {
            mutable: false,
            name,
            type_ann: Some(marker.to_string()),
            init: objlit,
            is_var: false,
        })
    }

    /// A merged declaration's statement: the marker call carrying its
    /// new members' object literal, in front of an empty block (the
    /// first declaration already binds and exports the name).
    fn enum_merge_call(&mut self, state: usize, objlit: ExprId) -> Stmt {
        let primary = f64::from(self.enums.states[state].objlit.0);
        let name = self.enums.states[state].name.clone();
        let callee = self.ast.add_expr(Expr::Ident(ENUM_MERGE_ANN.to_string()));
        let target = self.ast.add_expr(Expr::Ident(name));
        let primary = self.ast.add_expr(Expr::Number(primary));
        let args = vec![target, primary, objlit];
        let call = self.ast.add_expr(Expr::Call { callee, args });
        self.enum_prelude().push(Stmt::Expr(call));
        Stmt::Block(Vec::new())
    }
}
