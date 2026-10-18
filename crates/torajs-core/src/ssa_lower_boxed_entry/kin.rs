//! The tests of an adapter's user slots ([`Kins::slot_tests`]), and
//! which cells a slot typed by a class admits.
//!
//! Classes with the same fields share one sid, so the layout test
//! alone admits an instance of an unrelated class into the slot. The
//! body still treats the cell as an instance of the declared class: a
//! method no subclass overrides is called as that class's own function
//! (`__cm_<C>__<m>`), and `instanceof` compares class tags. A cell of
//! another class reads `p.m` off its own prototype chain (§13.3.2.1),
//! which only the all-`any` twin does, so the slot's test also lists
//! the class tags its cell may wear: the declared class and its
//! subclasses (the `instanceof` answer set), plus every generic row
//! of one of them, which Pass 2 mints per factory
//! ([`admit_generic_rows`]).
//!
//! A list is rodata: a u32 count, then that many u32 tags.

use std::collections::HashMap;

use crate::ast::Ast;
use crate::ssa::{StringId, StringLiteral, StructId, Type};
use crate::ssa_lower_anon_stamp::AnonStampPoolCell;
use crate::ssa_lower_instanceof::tags::descendant_tags;

use super::fit_tag::{FitTag, SlotTest, slot_test};
use super::targets::BoxedEntryTarget;

/// The slot tests of one module's adapters; their class lists are
/// interned once each.
pub(super) struct Kins<'a> {
    class_parents: &'a HashMap<String, Option<String>>,
    class_name_to_tag: &'a HashMap<String, u32>,
    aliases: &'a HashMap<String, Type>,
    arr_layouts: &'a crate::ssa::ArrLayouts,
    pool: &'a AnonStampPoolCell,
    named_sid_to_tag: HashMap<StructId, u32>,
    type_aliases: HashMap<String, String>,
    interned: HashMap<Vec<u32>, StringId>,
}

impl<'a> Kins<'a> {
    pub(super) fn new(
        ast: &'a Ast,
        class_name_to_tag: &'a HashMap<String, u32>,
        aliases: &'a HashMap<String, Type>,
        arr_layouts: &'a crate::ssa::ArrLayouts,
        pool: &'a AnonStampPoolCell,
    ) -> Self {
        // S2.36 — sid → class_tag for the struct-param coercion call.
        // Named classes resolve through their name tag; anonymous shapes
        // through the Pass-1.5 stamp pool (same split the ObjectLit
        // alloc stamping uses).
        // Classes with the same layout share one sid; the lowest tag
        // answers for it, so the choice never follows hash order.
        let mut named_sid_to_tag: HashMap<StructId, u32> = HashMap::new();
        for (cname, &tag) in class_name_to_tag {
            if let Some(Type::Obj(sid)) = aliases.get(cname) {
                let t = named_sid_to_tag.entry(*sid).or_insert(tag);
                *t = (*t).min(tag);
            }
        }
        Kins {
            class_parents: &ast.class_parents,
            class_name_to_tag,
            aliases,
            arr_layouts,
            pool,
            named_sid_to_tag,
            type_aliases: crate::ast::scalar_alias_inline::unshadowed_aliases(ast),
            interned: HashMap::new(),
        }
    }

    /// The test of each user slot of `t`.
    pub(super) fn slot_tests(
        &mut self,
        strings: &mut Vec<StringLiteral>,
        t: &BoxedEntryTarget,
        undef_misses: bool,
    ) -> Vec<SlotTest> {
        let arr_layouts = self.arr_layouts;
        (0..t.user_tys.len())
            .map(|i| {
                let ty = t.user_tys[i];
                let test = slot_test(ty, arr_layouts, &|sid| self.sid_tag(sid), undef_misses);
                let ann = t.type_anns.get(i).and_then(|a| a.as_deref());
                self.slot(strings, test, ty, ann)
            })
            .collect()
    }

    fn sid_tag(&self, sid: StructId) -> u32 {
        self.named_sid_to_tag
            .get(&sid)
            .copied()
            .unwrap_or_else(|| self.pool.borrow_mut().assign_or_get(sid))
    }

    /// `test` of a slot of type `ty` annotated `ann`: when the
    /// annotation names a declared class whose layout is the slot's,
    /// the test asks for that class's tag and its class list.
    fn slot(
        &mut self,
        strings: &mut Vec<StringLiteral>,
        test: SlotTest,
        ty: Type,
        ann: Option<&str>,
    ) -> SlotTest {
        let Some(fit) = test.fit else { return test };
        let Some(name) = ann.and_then(|a| ann_class(a, fit.depth, &self.type_aliases)) else {
            return test;
        };
        let elem = (0..fit.depth).fold(ty, |t, _| match t {
            Type::Arr(a) => self.arr_layouts[a.0 as usize],
            t => t,
        });
        let (Some(&tag), Some(&decl)) = (self.class_name_to_tag.get(name), self.aliases.get(name))
        else {
            return test;
        };
        let tags = descendant_tags(self.class_parents, self.class_name_to_tag, name);
        if decl != elem || tags.is_empty() {
            return test;
        }
        let pool = self.pool;
        let kin = *self.interned.entry(tags).or_insert_with_key(|tags| {
            strings.push(StringLiteral::from_latin1_bytes(encode(tags)));
            let id = StringId(strings.len() as u32 - 1);
            pool.borrow_mut().note_kin_list(id);
            id
        });
        SlotTest {
            fit: Some(FitTag {
                tag,
                kin: Some(kin),
                ..fit
            }),
            ..test
        }
    }
}

/// Add to every class list the generic rows Pass 2 minted for one of
/// its classes: such a row's cells are instances of that class.
pub(crate) fn admit_generic_rows(
    pool: &AnonStampPoolCell,
    class_name_to_tag: &HashMap<String, u32>,
    strings: &mut [StringLiteral],
) {
    let pool = pool.borrow();
    let widen = |tags: &mut Vec<u32>| {
        let n = tags.len();
        for (row, _, base) in pool.generic_rows() {
            if class_name_to_tag
                .get(base)
                .is_some_and(|t| tags[..n].contains(t))
            {
                tags.push(*row);
            }
        }
    };
    for &list in pool.kin_lists() {
        let lit = &mut strings[list.0 as usize];
        let mut tags = decode(&lit.bytes);
        let n = tags.len();
        widen(&mut tags);
        if tags.len() > n {
            *lit = StringLiteral::from_latin1_bytes(encode(&tags));
        }
    }
    for &plan in pool.cast_plans() {
        let lit = &mut strings[plan.0 as usize];
        let mut bytes = lit.bytes.clone();
        crate::ssa_lower_checked_cast::plan::rewrite_class_lists(&mut bytes, &widen);
        if bytes != lit.bytes {
            *lit = StringLiteral::from_latin1_bytes(bytes);
        }
    }
}

/// A class list's rodata bytes.
pub(crate) fn encode(tags: &[u32]) -> Vec<u8> {
    std::iter::once(tags.len() as u32)
        .chain(tags.iter().copied())
        .flat_map(u32::to_le_bytes)
        .collect()
}

fn decode(bytes: &[u8]) -> Vec<u32> {
    let (words, _) = bytes.as_chunks::<4>();
    words
        .iter()
        .skip(1)
        .map(|w| u32::from_le_bytes(*w))
        .collect()
}

/// The class name an annotation spells behind `depth` levels of array
/// (`C[]`, `Array<C>`), seen through `C | null` / `C | undefined`,
/// `readonly` and type aliases; `None` for any other shape.
fn ann_class<'a>(
    ann: &'a str,
    depth: u32,
    aliases: &'a HashMap<String, String>,
) -> Option<&'a str> {
    let mut a = unwrap_ann(ann, aliases);
    for _ in 0..depth {
        a = match a.strip_suffix("[]") {
            Some(inner) => inner.trim(),
            None => a.strip_prefix("Array<")?.strip_suffix('>')?.trim(),
        };
        a = unwrap_ann(a, aliases);
    }
    a.chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '$')
        .then_some(a)
}

/// `ann` with the spellings that name the same slot type peeled off:
/// a nullable union (its `null` is the slot's in-band 0, never a
/// cell), `readonly`, and an alias (bounded, an alias cycle is a type
/// error the checker reports).
fn unwrap_ann<'a>(ann: &'a str, aliases: &'a HashMap<String, String>) -> &'a str {
    let mut a = ann.trim();
    for _ in 0..16 {
        a = if let Some(inner) = a
            .strip_prefix("__nullable(")
            .and_then(|r| r.strip_suffix(')'))
        {
            inner.trim()
        } else if let Some(inner) = a.strip_prefix("readonly ") {
            inner.trim()
        } else if let Some(target) = aliases.get(a) {
            target.trim()
        } else {
            break;
        };
    }
    a
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{ann_class, decode, encode};

    #[test]
    fn ann_class_peels_array_levels() {
        let none = HashMap::new();
        assert_eq!(ann_class("P", 0, &none), Some("P"));
        assert_eq!(ann_class("P[]", 1, &none), Some("P"));
        assert_eq!(ann_class("Array<P>[]", 2, &none), Some("P"));
        assert_eq!(ann_class("P[]", 0, &none), None);
        assert_eq!(ann_class("P", 1, &none), None);
        assert_eq!(ann_class("Box<number>", 0, &none), None);
    }

    #[test]
    fn ann_class_sees_through_nullable_readonly_and_aliases() {
        let aliases = HashMap::from([
            ("A".to_string(), "P".to_string()),
            ("B".to_string(), "__nullable(A)".to_string()),
            ("L".to_string(), "readonly A[]".to_string()),
        ]);
        assert_eq!(ann_class("__nullable(P)", 0, &aliases), Some("P"));
        assert_eq!(ann_class("A", 0, &aliases), Some("P"));
        assert_eq!(ann_class("B", 0, &aliases), Some("P"));
        assert_eq!(ann_class("L", 1, &aliases), Some("P"));
        assert_eq!(ann_class("__nullable(A)[]", 1, &aliases), Some("P"));
        assert_eq!(ann_class("__nullable(P[])", 1, &aliases), Some("P"));
        assert_eq!(ann_class("__union(P,Q)", 0, &aliases), None);
    }

    #[test]
    fn list_is_count_then_tags() {
        let bytes = encode(&[3, 70_000]);
        assert_eq!(bytes.len(), 12);
        assert_eq!(&bytes[..4], &2u32.to_le_bytes());
        assert_eq!(decode(&bytes), vec![3, 70_000]);
    }
}
