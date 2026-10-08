//! Which declaration a name inside an enum declaration refers to.
//!
//! A member initializer is a constant when every name in it resolves
//! to a constant: a member of this enum, a constant member of another
//! enum, or a `const` without a type annotation whose initializer is
//! itself constant. The parser answers "which declaration does `x`
//! mean here" on the token stream: a declaration is in scope when no
//! `}` between it and the use closes a block it was in, and nothing
//! in a block opened after it (and still open at the use) binds the
//! same name. The second test errs towards "shadowed": a name it
//! cannot rule out keeps the member computed, which evaluates the same
//! value at runtime.

use super::super::*;
use std::ops::Range;

/// A `const` without a type annotation; its initializer is evaluated
/// only when an enum member names it.
pub(super) struct ConstBinding {
    name: String,
    init: ExprId,
    /// Token index just past the initializer.
    pos: usize,
}

impl Parser<'_> {
    /// Every `let` / `const` / `var` declarator with an initializer.
    pub(in crate::parser) fn note_decl_init(
        &mut self,
        name: &str,
        init: ExprId,
        plain_const: bool,
    ) {
        self.register_class_value_alias(name, init);
        if plain_const {
            let pos = self.pos;
            self.enums.consts.push(ConstBinding {
                name: name.to_string(),
                init,
                pos,
            });
        }
    }

    /// No `}` in `tokens[from..at]` closes a block open at `from`.
    pub(super) fn scope_reaches(&self, from: usize, at: usize) -> bool {
        let mut depth = 0i32;
        for t in &self.tokens[from..at] {
            match t.token {
                Token::LBrace => depth += 1,
                Token::RBrace if depth == 0 => return false,
                Token::RBrace => depth -= 1,
                _ => {}
            }
        }
        true
    }

    /// Whether `name` may be bound in a block opened in
    /// `tokens[from..at]` that is still open at `at`; tokens in `skip`
    /// (the enum being declared) do not count.
    fn shadowed(&self, name: &str, from: usize, at: usize, skip: &Range<usize>) -> bool {
        let mut open: Vec<usize> = Vec::new();
        for i in from..at {
            match self.tokens[i].token {
                Token::LBrace => open.push(i),
                Token::RBrace => {
                    open.pop();
                }
                _ => {}
            }
        }
        open.iter().any(|&b| {
            let region = self.block_head(b)..self.group_end(b);
            region.filter(|i| !skip.contains(i)).any(|i| {
                matches!(&self.tokens[i].token, Token::Ident(n) if n == name) && self.binds_at(i)
            })
        })
    }

    /// The first token of the parameter list (or catch / loop head)
    /// in front of the block opening at `b`, else `b`.
    fn block_head(&self, b: usize) -> usize {
        let mut j = b;
        if j >= 2 && matches!(self.tokens[j - 1].token, Token::FatArrow) {
            j -= 1;
            if matches!(self.tokens[j - 1].token, Token::Ident(_)) {
                return j - 1;
            }
        }
        // step back over a return type annotation to the `)`
        let mut depth = 0i32;
        while j > 0 {
            j -= 1;
            match self.tokens[j].token {
                Token::RParen if depth == 0 => return self.group_start(j),
                Token::RBrace | Token::RBracket | Token::Gt => depth += 1,
                Token::LBrace | Token::LBracket | Token::Lt if depth > 0 => depth -= 1,
                Token::Ident(_) | Token::Colon | Token::Dot | Token::Comma | Token::Pipe => {}
                Token::Amp | Token::Question | Token::String(_) | Token::Number(_) => {}
                Token::FatArrow | Token::Null | Token::Void | Token::TypeOf => {}
                _ if depth > 0 => {}
                _ => break,
            }
        }
        b
    }

    /// Index just past the group closing the bracket at `open`.
    pub(super) fn group_end(&self, open: usize) -> usize {
        let mut depth = 0i32;
        for (i, t) in self.tokens.iter().enumerate().skip(open) {
            match t.token {
                Token::LBrace | Token::LParen | Token::LBracket => depth += 1,
                Token::RBrace | Token::RParen | Token::RBracket => {
                    depth -= 1;
                    if depth == 0 {
                        return i + 1;
                    }
                }
                _ => {}
            }
        }
        self.tokens.len()
    }

    /// The bracket opening the group that closes at `close`.
    fn group_start(&self, close: usize) -> usize {
        let mut depth = 0i32;
        for i in (0..=close).rev() {
            match self.tokens[i].token {
                Token::RBrace | Token::RParen | Token::RBracket => depth += 1,
                Token::LBrace | Token::LParen | Token::LBracket => {
                    depth -= 1;
                    if depth == 0 {
                        return i;
                    }
                }
                _ => {}
            }
        }
        0
    }

    /// The innermost bracket still open at token `i`.
    fn open_group(&self, i: usize) -> Option<usize> {
        let mut depth = 0i32;
        for j in (0..i).rev() {
            match self.tokens[j].token {
                Token::RBrace | Token::RParen | Token::RBracket => depth += 1,
                Token::LBrace | Token::LParen | Token::LBracket if depth == 0 => return Some(j),
                Token::LBrace | Token::LParen | Token::LBracket => depth -= 1,
                _ => {}
            }
        }
        None
    }

    /// Whether the identifier at token `i` may be a binding: a
    /// declaration's name or pattern element, a parameter, a catch
    /// parameter. References that look like one count too.
    fn binds_at(&self, i: usize) -> bool {
        if matches!(
            self.tokens.get(i + 1).map(|t| &t.token),
            Some(Token::FatArrow)
        ) {
            return true;
        }
        let mut at = i;
        loop {
            let prev = at.checked_sub(1).map(|j| &self.tokens[j].token);
            match prev {
                Some(Token::Let | Token::Var | Token::Const | Token::Function | Token::Class) => {
                    return true;
                }
                Some(Token::Comma) if self.declarator_list(at - 1) => return true,
                Some(
                    Token::Comma
                    | Token::Colon
                    | Token::LBrace
                    | Token::LBracket
                    | Token::LParen
                    | Token::DotDotDot,
                ) => {}
                _ => return false,
            }
            let Some(g) = self.open_group(at) else {
                return false;
            };
            if matches!(self.tokens[g].token, Token::LParen) {
                return self.binding_parens(g);
            }
            at = g;
        }
    }

    /// The `,` at `comma` separates the declarators of a `let` /
    /// `const` / `var` statement.
    fn declarator_list(&self, comma: usize) -> bool {
        let floor = self.open_group(comma).map_or(0, |g| g + 1);
        let mut depth = 0i32;
        for j in (floor..comma).rev() {
            match self.tokens[j].token {
                Token::RBrace | Token::RParen | Token::RBracket => depth += 1,
                Token::LBrace | Token::LParen | Token::LBracket => depth -= 1,
                Token::Semi if depth == 0 => return false,
                Token::Let | Token::Var | Token::Const if depth == 0 => return true,
                _ => {}
            }
        }
        false
    }

    /// The parentheses at `open` hold parameters or a catch binding:
    /// a body or a return type follows them, and they are not the
    /// head of `if` / `while` / `switch` / `for` (a `for` head binds
    /// only behind `let` / `const` / `var`, which the caller saw).
    fn binding_parens(&self, open: usize) -> bool {
        let before = open.checked_sub(1).map(|j| &self.tokens[j].token);
        if matches!(
            before,
            Some(Token::If | Token::While | Token::Switch | Token::For)
        ) {
            return false;
        }
        let after = self.tokens.get(self.group_end(open)).map(|t| &t.token);
        matches!(after, Some(Token::LBrace | Token::FatArrow | Token::Colon))
    }

    /// The enum declaration named `name` in scope at `at`.
    pub(super) fn visible_enum(&self, name: &str, at: usize, skip: &Range<usize>) -> Option<usize> {
        let i = self.enums.states.iter().rposition(|s| {
            s.name == name && s.start_pos < at && self.scope_reaches(s.start_pos, at)
        })?;
        let from = self.enums.states[i].start_pos;
        (!self.shadowed(name, from, at, skip)).then_some(i)
    }

    /// The `const` named `name` in scope at `at`.
    pub(super) fn visible_const(
        &self,
        name: &str,
        at: usize,
        skip: &Range<usize>,
    ) -> Option<usize> {
        let i = self
            .enums
            .consts
            .iter()
            .rposition(|c| c.name == name && c.pos < at && self.scope_reaches(c.pos, at))?;
        let from = self.enums.consts[i].pos;
        (!self.shadowed(name, from, at, skip)).then_some(i)
    }

    pub(super) fn const_binding(&self, i: usize) -> (ExprId, usize) {
        let c = &self.enums.consts[i];
        (c.init, c.pos)
    }

    /// Cursor on a type name: an enum's name reads as its type alias,
    /// and `E.M` naming one of its members reads as `E` (the member's
    /// type is the enum's own). Steps past the name.
    pub(in crate::parser) fn enum_type_ref(&mut self, name: String) -> String {
        let at = self.pos;
        self.pos += 1;
        // a value binding does not shadow a type name
        let found = self.enums.states.iter().rposition(|s| {
            s.name == name && s.start_pos < at && self.scope_reaches(s.start_pos, at)
        });
        let Some(i) = found else {
            return name;
        };
        let tok = |p: &Self, k: usize| p.tokens.get(p.pos + k).map(|t| &t.token);
        if matches!(tok(self, 0), Some(Token::Dot)) && matches!(tok(self, 1), Some(Token::Ident(_)))
        {
            self.pos += 2;
        }
        self.enums.states[i].alias.clone()
    }
}
