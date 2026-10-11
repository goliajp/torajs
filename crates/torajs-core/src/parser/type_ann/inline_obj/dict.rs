//! The dictionary spellings of an object type: an interface whose body
//! declares an index signature, the `Record<K, V>` utility type, and
//! the `__dict(K|V)` annotation both read as. Moved out of [`super`]
//! whole.

use super::*;
use crate::ast::DictSite;

impl Parser<'_> {
    /// The rest of `interface Name { .. }` whose body declares an index
    /// signature (cursor at `{`): the interface is the dictionary the
    /// body spells, held as a type alias.
    pub(in crate::parser) fn finish_dict_interface(
        &mut self,
        name: String,
        type_params: Vec<String>,
        heritage: Vec<(crate::ast::PropKey, String)>,
    ) -> Result<Stmt, String> {
        let body_at = self.at();
        self.pos += 1;
        let (mut fields, index_sig) =
            self.parse_obj_type_members(Token::RBrace, "interface body")?;
        self.pos += 1;
        if matches!(self.peek(), Token::Semi) {
            self.pos += 1;
        }
        // an inherited member must fit the signature as a declared one
        // does; the parent's members are checked as one `extends` row
        for (key, parent) in heritage {
            fields.push((key.lossy().to_string(), parent.clone(), parent, body_at));
        }
        let sig = index_sig.expect("the lookahead saw an index signature");
        let ann = self.dict_ann(sig, fields, false);
        Ok(Stmt::TypeDecl {
            name,
            type_params,
            fields: vec![(crate::ast::PropKey::from("__alias__"), ann)],
        })
    }

    /// `Record<K, V>` (its type arguments already read, the first
    /// starting at token `args_pos`) as the dictionary `__dict(K|V)`;
    /// any other head or arity is `None`. `Record` is read as the
    /// builtin utility type, the way `Array` and `Map` are.
    pub(in crate::parser::type_ann) fn record_dict_ann(
        &mut self,
        head: &str,
        args: &[String],
        args_pos: usize,
    ) -> Option<String> {
        let ("Record", [key, value]) = (head, args) else {
            return None;
        };
        let sig = IndexSig {
            key: key.clone(),
            key_src: key.clone(),
            key_at: self.tokens[args_pos].span.start,
            value: value.clone(),
            value_src: value.clone(),
        };
        Some(self.dict_ann(sig, Vec::new(), true))
    }

    /// The dictionary spelling `__dict(K|V)`, its site recorded for the
    /// checker's diagnostics.
    pub(super) fn dict_ann(
        &mut self,
        sig: IndexSig,
        mut members: Vec<ObjTypeMember>,
        record: bool,
    ) -> String {
        let ann = format!("__dict({}|{})", sig.key, sig.value);
        for m in &mut members {
            m.0 = m.0.trim_end_matches('?').to_string();
        }
        self.ast.dict_sites.push(DictSite {
            record,
            key_at: sig.key_at,
            key: sig.key,
            key_src: sig.key_src,
            value: sig.value,
            value_src: sig.value_src,
            members,
        });
        ann
    }
}
