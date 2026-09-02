//! ES ToPropertyKey (§7.1.19) compile-time fold for literal keys —
//! chunk 745. One spelling shared by the parser's object-literal
//! numeric-key arm and the checker/SSA struct-index lanes, so
//! `{ 0: v }`, `g[0]`, and `g["0"]` all agree on the field name "0".

use core::fmt;
use core::ops::Deref;

use torajs_wtf8::{Wtf8, Wtf8Buf};

use super::{Ast, Expr, ExprId};

/// A property key as the program spelled it — a UTF-16 code-unit
/// sequence (§6.1.7), so a lone surrogate (`{ "\uD800": 1 }`) is a
/// key of its own and never collapses into U+FFFD. Backed by WTF-8:
/// byte equality is code-unit equality, so the derived `Eq` / `Hash`
/// / `Ord` are the spec's SameValue on keys. Identifier-shaped keys
/// (the overwhelming majority) are plain UTF-8 and `as_str` answers
/// `Some`; every site that needs a `&str` — a `format!` into a
/// synthetic name, a `HashMap<String, _>` lookup — reads `as_str()`
/// and decides what an ill-formed key means there.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct PropKey(Wtf8Buf);

impl PropKey {
    /// `prefix` + `key`: the synthetic-name spelling (`__getter_x`).
    pub fn prefixed(prefix: &str, key: &Wtf8) -> PropKey {
        let mut b = Wtf8Buf::with_capacity(prefix.len() + key.len());
        b.push_str(prefix);
        b.push_wtf8(key);
        PropKey(b)
    }

    #[inline]
    pub fn as_wtf8(&self) -> &Wtf8 {
        self.0.as_wtf8()
    }

    /// Display-only spelling: lone surrogates become U+FFFD. Never
    /// feed this back into a key.
    pub fn to_string_lossy_owned(&self) -> String {
        self.0.to_string_lossy_owned()
    }
}

impl Deref for PropKey {
    type Target = Wtf8;
    #[inline]
    fn deref(&self) -> &Wtf8 {
        self.0.as_wtf8()
    }
}

impl core::borrow::Borrow<Wtf8> for PropKey {
    #[inline]
    fn borrow(&self) -> &Wtf8 {
        self.0.as_wtf8()
    }
}

impl AsRef<Wtf8> for PropKey {
    #[inline]
    fn as_ref(&self) -> &Wtf8 {
        self.0.as_wtf8()
    }
}

impl From<String> for PropKey {
    #[inline]
    fn from(s: String) -> PropKey {
        PropKey(Wtf8Buf::from(s))
    }
}

impl From<&str> for PropKey {
    #[inline]
    fn from(s: &str) -> PropKey {
        PropKey(Wtf8Buf::from(s))
    }
}

impl From<&String> for PropKey {
    #[inline]
    fn from(s: &String) -> PropKey {
        PropKey(Wtf8Buf::from(s.as_str()))
    }
}

impl From<Wtf8Buf> for PropKey {
    #[inline]
    fn from(b: Wtf8Buf) -> PropKey {
        PropKey(b)
    }
}

impl From<&Wtf8> for PropKey {
    #[inline]
    fn from(w: &Wtf8) -> PropKey {
        PropKey(Wtf8Buf::from(w))
    }
}

impl PartialEq<str> for PropKey {
    #[inline]
    fn eq(&self, o: &str) -> bool {
        self.0 == *o
    }
}

impl PartialEq<&str> for PropKey {
    #[inline]
    fn eq(&self, o: &&str) -> bool {
        self.0 == **o
    }
}

impl PartialEq<String> for PropKey {
    #[inline]
    fn eq(&self, o: &String) -> bool {
        self.0 == *o
    }
}

impl PartialEq<Wtf8> for PropKey {
    #[inline]
    fn eq(&self, o: &Wtf8) -> bool {
        self.0 == *o
    }
}

impl PartialEq<Wtf8Buf> for PropKey {
    #[inline]
    fn eq(&self, o: &Wtf8Buf) -> bool {
        self.0 == *o
    }
}

impl PartialEq<PropKey> for Wtf8Buf {
    #[inline]
    fn eq(&self, o: &PropKey) -> bool {
        *self == o.0
    }
}

impl PartialEq<PropKey> for str {
    #[inline]
    fn eq(&self, o: &PropKey) -> bool {
        o.0 == *self
    }
}

impl PartialEq<PropKey> for &str {
    #[inline]
    fn eq(&self, o: &PropKey) -> bool {
        o.0 == **self
    }
}

impl PartialEq<PropKey> for String {
    #[inline]
    fn eq(&self, o: &PropKey) -> bool {
        o.0 == *self
    }
}

impl fmt::Display for PropKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl fmt::Debug for PropKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}

/// ES Number-to-property-key spelling: integral finite values print
/// as integers (`0` not `0.0`), everything else takes the shortest
/// float form — matching bun's serialization.
pub fn number_prop_key(n: f64) -> PropKey {
    if n.is_finite() && n == n.trunc() && n.abs() < 1e21 {
        PropKey::from(format!("{}", n as i64))
    } else {
        PropKey::from(format!("{n}"))
    }
}

/// A compile-time literal index expression folded to its property
/// key (`g[0]` → "0", `g["0"]` → "0"); `None` for dynamic indices.
/// Identifier-shaped string literals never reach the Index shape
/// (the parser's V3-18 wedge folds them to Member), so hits here
/// are numeric keys and non-identifier string keys.
pub fn literal_prop_key(ast: &Ast, index: ExprId) -> Option<PropKey> {
    match ast.get_expr(index) {
        Expr::Number(n) => Some(number_prop_key(*n)),
        Expr::String(s) => Some(PropKey::from(s.clone())),
        _ => None,
    }
}
