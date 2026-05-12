// V3-18 wedge — TS `readonly` modifier on a type-body field
// (interface / type-alias / inline-obj position):
//   interface X { readonly id: number }
//   type Y = { readonly id: number }
//   let p: { readonly x: number } = ...
// Per TS spec §3.10.2 the modifier is type-side only — once
// assigned, the field can't be reassigned. The subset accepts
// and discards the modifier (no enforcement yet); reads still
// work normally. Pre-fix tora's parser bailed at the type-body
// field reader with 'expected `:` after field name `readonly`,
// got Ident("id")'.
//
// Implementation: at parse_type_decl_field and the inline-obj
// branch of parse_type_ann, peek for `Ident("readonly")` +
// ident-shaped name and consume the modifier.

interface Frozen {
  readonly id: number;
  readonly tag: string;
}
let f: Frozen = { id: 1, tag: "x" }
console.log(f.id, f.tag)               // 1 x

type R = {
  readonly created: number;
  name: string;
}
let r: R = { created: 1700000000, name: "alice" }
console.log(r.created, r.name)         // 1700000000 alice

// Inline-obj position (no alias).
let p: { readonly x: number; y: number } = { x: 1, y: 2 }
console.log(p.x, p.y)                  // 1 2
