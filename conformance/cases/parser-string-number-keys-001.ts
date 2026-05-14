// P0.10 — string-literal and numeric-literal property names in
// object literals per ES spec §12.7.6 PropertyName ::
// StringLiteral | NumericLiteral. Pre-fix tora's parser only
// accepted Ident keys (plus reserved words via the existing
// keyword_property_name path), bailing on `{ "x": v }` or
// `{ 0: v }` with 'expected field name in object literal, got
// String/Number'. Test262 uses these pervasively for array-like
// object literals (`{ length: 7, 0: 2, 1: 4, ... }` style) and
// quoted keys — 600+ cases blocked on this single shape.
//
// Implementation: extend `parse_object_field` to accept
// Token::String → key = the string value, Token::Number → key =
// integer-formatted name when finite + integer, otherwise the
// f64 default print. The key stays a String at AST level (tora's
// struct fields are name-keyed). Member access via the same
// string name still works for ident-shape names; numeric-shape
// names typically need bracket-index, which is a separate item.
// Getter/setter shorthand also extended to accept string and
// numeric prop names since they share the same property-name
// surface per spec.

// Identifier-shape numeric key — accessible via member access if
// the integer printable form happens to look like an ident (it
// doesn't for digits, so skip member access on those — only the
// length / mixed-ident cases are tested for member here).
let a = { length: 2, 0: 100, 1: 200 }
console.log(a.length)                        // 2

// String-literal key — both quoted-as-ident and any string.
let b = { "key": "v1", "x": "v2" }
console.log(b.key)                           // v1
console.log(b.x)                             // v2

// Mixed ident / string / numeric keys.
let c = { foo: "bar", 0: "zero", "baz": "qux" }
console.log(c.foo)                           // bar
console.log(c.baz)                           // qux

// Reserved-word key (regression — already worked via the
// keyword_property_name path).
let d = { type: "T", default: "D" }
console.log(d.type)                          // T
console.log(d.default)                       // D

// String-keyed getter shorthand (parse-only stub — no real
// accessor dispatch).
let e = { foo: 1, get "bar"() { return 2; } }
console.log(e.foo)                           // 1
