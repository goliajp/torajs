// RFC 20260716-primitive-wrapper-substrate 刀 5 — `+` / `<` / `>`
// with wrapper operands. Two substrate fixes ship together:
//
// 1. is_heap_str (compare.rs) recognises Tag::StringWrapper so the
//    ES §13.15.3 string-concat / §7.2.13 lexicographic path fires;
//    pre-fix `new String("1") + new String("1")` fell into the
//    numeric arm and answered `2`.
//
// 2. binop_inner_any_arith retags a null-shaped pack (ConstPtrNull →
//    ANY_NULL=0) as ANY_UNDEF=5 whenever the checker knows the
//    operand's frontend type is Type::Undefined. Mirrors the P1.5
//    `Number(undefined) === NaN` shortcut in ssa_lower_call_coercion
//    so `s + undefined` concats "undefined" instead of "null".

// Wrapper + wrapper — both sides String-shaped, concat via ToString.
console.log(new String("1") + new String("1"));  // "11"

// Wrapper + primitive — primitive coerces to String because at least
// one side is String (spec §13.15.3 step 4).
console.log(new String("1") + 1);                // "11"
console.log(new String("1") + true);             // "1true"
console.log(new String("1") + null);             // "1null"
console.log(new String("1") + undefined);        // "1undefined"

// Primitive + wrapper (mirror).
console.log(true + new String("1"));             // "true1"
console.log(1 + new String("1"));                // "11"

// Wrapper < wrapper — lexicographic byte compare via
// str_effective_ptr unwrap of [[StringData]].
console.log(new String("a") < new String("b"));  // true
console.log(new String("b") < new String("a"));  // false
console.log(new String("hi") == new String("hi")); // false (identity)
