//! The interned form of [`SlotKey`]: one table per compilation, built
//! by the analysis and handed to the frozen width table with the
//! union-find. Inside the analysis a key is a [`SlotKeyId`] (Copy,
//! hashed as one word); lowering keeps asking with spellings, which
//! the table looks up without adding to.
//!
//! The table interns through `&self` (a `RefCell`, the usual shape of
//! an interner): the walk builds keys from inside closures and
//! iterator chains that already borrow the analysis.

use super::SlotKey;
use crate::ast::PropKey;
use crate::ssa::id_hash::{HashMap, HashSet};
use std::cell::{Ref, RefCell};
use torajs_wtf8::Wtf8;

/// Sets and maps keyed by slot key id.
pub(crate) type KeySet = HashSet<SlotKeyId>;
pub(crate) type KeyMap<V> = HashMap<SlotKeyId, V>;

/// An interned name or property key: binding names, fn names and
/// field names share one table (a name is the UTF-8 subset of WTF-8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct Atom(u32);

/// An interned [`SlotKey`]: an index into [`SlotKeys`]. Two ids are
/// equal exactly when the spellings they intern are.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct SlotKeyId(u32);

impl SlotKeyId {
    #[inline]
    pub(crate) fn index(self) -> usize {
        self.0 as usize
    }

    #[inline]
    pub(crate) fn from_index(i: usize) -> SlotKeyId {
        SlotKeyId(i as u32)
    }
}

/// A [`SlotKey`] with its names interned and its inner key an id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum SlotKeyData {
    Global(Atom),
    Local(Atom, Atom),
    Param(Atom, Atom),
    Ret(Atom),
    Class(Atom),
    Captured(Atom),
    Anon(u32),
    Elem(SlotKeyId),
    Field(SlotKeyId, Atom),
    Thrown,
}

#[derive(Default)]
struct Tables {
    atoms: Vec<PropKey>,
    atom_ids: HashMap<PropKey, Atom>,
    data: Vec<SlotKeyData>,
    /// Elem / Field nesting depth per key.
    depth: Vec<u32>,
    ids: HashMap<SlotKeyData, SlotKeyId>,
}

/// One compilation's slot-key table. The analysis interns every key
/// it builds; the frozen width table only looks spellings up, and a
/// spelling it cannot find was never seen by the analysis.
#[derive(Default)]
pub(crate) struct SlotKeys {
    t: RefCell<Tables>,
}

impl SlotKeys {
    pub(crate) fn atom(&self, s: &Wtf8) -> Atom {
        if let Some(a) = self.find_atom(s) {
            return a;
        }
        let mut t = self.t.borrow_mut();
        let a = Atom(t.atoms.len() as u32);
        t.atoms.push(PropKey::from(s));
        t.atom_ids.insert(PropKey::from(s), a);
        a
    }

    pub(crate) fn find_atom(&self, s: &Wtf8) -> Option<Atom> {
        self.t.borrow().atom_ids.get(s).copied()
    }

    /// The name an atom interned. Only binding / fn / class names are
    /// asked this way, and those come from `String`s.
    pub(crate) fn name(&self, a: Atom) -> Ref<'_, str> {
        Ref::map(self.t.borrow(), |t| {
            t.atoms[a.0 as usize]
                .as_str()
                .expect("slot names are UTF-8")
        })
    }

    pub(crate) fn prop(&self, a: Atom) -> Ref<'_, PropKey> {
        Ref::map(self.t.borrow(), |t| &t.atoms[a.0 as usize])
    }

    /// A name or field the desugar spelled (`__this`, `__ret`, …).
    pub(crate) fn dunder(&self, a: Atom) -> bool {
        self.t.borrow().atoms[a.0 as usize].starts_with("__")
    }

    pub(crate) fn is(&self, a: Atom, s: &str) -> bool {
        self.t.borrow().atoms[a.0 as usize] == *s
    }

    pub(crate) fn mk(&self, d: SlotKeyData) -> SlotKeyId {
        if let Some(id) = self.get(&d) {
            return id;
        }
        let depth = match d {
            SlotKeyData::Elem(x) | SlotKeyData::Field(x, _) => self.depth(x) + 1,
            _ => 0,
        };
        let mut t = self.t.borrow_mut();
        let id = SlotKeyId(t.data.len() as u32);
        t.data.push(d);
        t.depth.push(depth);
        t.ids.insert(d, id);
        id
    }

    pub(crate) fn get(&self, d: &SlotKeyData) -> Option<SlotKeyId> {
        self.t.borrow().ids.get(d).copied()
    }

    #[inline]
    pub(crate) fn data(&self, id: SlotKeyId) -> SlotKeyData {
        self.t.borrow().data[id.index()]
    }

    #[inline]
    pub(crate) fn depth(&self, id: SlotKeyId) -> u32 {
        self.t.borrow().depth[id.index()]
    }

    pub(crate) fn len(&self) -> usize {
        self.t.borrow().data.len()
    }

    /// The container key an `Elem` / `Field` key points into.
    pub(crate) fn inner(&self, id: SlotKeyId) -> Option<SlotKeyId> {
        match self.data(id) {
            SlotKeyData::Elem(x) | SlotKeyData::Field(x, _) => Some(x),
            _ => None,
        }
    }

    pub(crate) fn global(&self, n: &str) -> SlotKeyId {
        let n = self.atom(Wtf8::new(n));
        self.mk(SlotKeyData::Global(n))
    }

    pub(crate) fn local(&self, f: &str, n: &str) -> SlotKeyId {
        let (f, n) = (self.atom(Wtf8::new(f)), self.atom(Wtf8::new(n)));
        self.mk(SlotKeyData::Local(f, n))
    }

    pub(crate) fn param(&self, f: &str, n: &str) -> SlotKeyId {
        let (f, n) = (self.atom(Wtf8::new(f)), self.atom(Wtf8::new(n)));
        self.mk(SlotKeyData::Param(f, n))
    }

    pub(crate) fn ret(&self, f: &str) -> SlotKeyId {
        let f = self.atom(Wtf8::new(f));
        self.mk(SlotKeyData::Ret(f))
    }

    pub(crate) fn class(&self, c: &str) -> SlotKeyId {
        let c = self.atom(Wtf8::new(c));
        self.mk(SlotKeyData::Class(c))
    }

    pub(crate) fn captured(&self, n: &str) -> SlotKeyId {
        let n = self.atom(Wtf8::new(n));
        self.mk(SlotKeyData::Captured(n))
    }

    pub(crate) fn anon(&self, e: u32) -> SlotKeyId {
        self.mk(SlotKeyData::Anon(e))
    }

    pub(crate) fn elem(&self, x: SlotKeyId) -> SlotKeyId {
        self.mk(SlotKeyData::Elem(x))
    }

    pub(crate) fn field(&self, x: SlotKeyId, n: impl AsRef<Wtf8>) -> SlotKeyId {
        let n = self.atom(n.as_ref());
        self.mk(SlotKeyData::Field(x, n))
    }

    pub(crate) fn thrown(&self) -> SlotKeyId {
        self.mk(SlotKeyData::Thrown)
    }

    /// The `(fn, param)` names of a `Param` key.
    pub(crate) fn param_names(&self, id: SlotKeyId) -> Option<(String, String)> {
        match self.data(id) {
            SlotKeyData::Param(f, n) => Some((self.name(f).to_string(), self.name(n).to_string())),
            _ => None,
        }
    }

    /// `Global(n)` at the module top level (`f` empty), else
    /// `Local(f, n)`.
    pub(crate) fn binding(&self, f: &str, n: &str) -> SlotKeyId {
        if f.is_empty() {
            self.global(n)
        } else {
            self.local(f, n)
        }
    }

    pub(crate) fn intern(&self, k: &SlotKey) -> SlotKeyId {
        match k {
            SlotKey::Global(n) => self.global(n),
            SlotKey::Local(f, n) => self.local(f, n),
            SlotKey::Param(f, n) => self.param(f, n),
            SlotKey::Ret(f) => self.ret(f),
            SlotKey::Class(c) => self.class(c),
            SlotKey::Captured(n) => self.captured(n),
            SlotKey::Anon(e) => self.anon(*e),
            SlotKey::Elem(x) => {
                let x = self.intern(x);
                self.elem(x)
            }
            SlotKey::Field(x, n) => {
                let x = self.intern(x);
                self.field(x, n)
            }
            SlotKey::Thrown => self.thrown(),
        }
    }

    /// The id of `k`, if the table holds it.
    pub(crate) fn lookup(&self, k: &SlotKey) -> Option<SlotKeyId> {
        let a = |s: &str| self.find_atom(Wtf8::new(s));
        let d = match k {
            SlotKey::Global(n) => SlotKeyData::Global(a(n)?),
            SlotKey::Local(f, n) => SlotKeyData::Local(a(f)?, a(n)?),
            SlotKey::Param(f, n) => SlotKeyData::Param(a(f)?, a(n)?),
            SlotKey::Ret(f) => SlotKeyData::Ret(a(f)?),
            SlotKey::Class(c) => SlotKeyData::Class(a(c)?),
            SlotKey::Captured(n) => SlotKeyData::Captured(a(n)?),
            SlotKey::Anon(e) => SlotKeyData::Anon(*e),
            SlotKey::Elem(x) => SlotKeyData::Elem(self.lookup(x)?),
            SlotKey::Field(x, n) => SlotKeyData::Field(self.lookup(x)?, self.find_atom(n)?),
            SlotKey::Thrown => SlotKeyData::Thrown,
        };
        self.get(&d)
    }

    /// The spelling `id` interns.
    pub(crate) fn spell(&self, id: SlotKeyId) -> SlotKey {
        let n = |a: Atom| self.name(a).to_string();
        match self.data(id) {
            SlotKeyData::Global(x) => SlotKey::Global(n(x)),
            SlotKeyData::Local(f, x) => SlotKey::Local(n(f), n(x)),
            SlotKeyData::Param(f, x) => SlotKey::Param(n(f), n(x)),
            SlotKeyData::Ret(f) => SlotKey::Ret(n(f)),
            SlotKeyData::Class(c) => SlotKey::Class(n(c)),
            SlotKeyData::Captured(x) => SlotKey::Captured(n(x)),
            SlotKeyData::Anon(e) => SlotKey::Anon(e),
            SlotKeyData::Elem(x) => SlotKey::Elem(Box::new(self.spell(x))),
            SlotKeyData::Field(x, f) => {
                SlotKey::Field(Box::new(self.spell(x)), self.prop(f).clone())
            }
            SlotKeyData::Thrown => SlotKey::Thrown,
        }
    }

    /// `{:?}` of the spelling, for the `TORAJS_NUM_WIDTH_STATS` dumps.
    pub(crate) fn debug(&self, id: SlotKeyId) -> String {
        format!("{:?}", self.spell(id))
    }
}
