// V3-18 wedge — static inheritance per ES spec §15.7.10. When
// `class Sub extends Base { ... }`, `Sub.greet` resolves to
// `Base.greet` and `Sub.count` to `Base.count` (unless Sub
// overrides them with its own static). Pre-fix tora's
// `static_member_rewrites` table only recorded each class's
// own statics, so `Sub.<inherited_static>` failed at typecheck
// with 'unknown identifier `Sub`'.
//
// Implementation: in ast::desugar_classes after collecting the
// per-class static_member_rewrites entries, walk every class's
// parent chain and `entry().or_insert` an alias from
// `(SubClass, member_name)` → `__sf_<ParentClass>__<name>` /
// `__sm_<ParentClass>__<name>`. Sub's own statics already take
// precedence (entered first). Multi-level chains (Sub → Mid →
// Base) work transitively because the loop visits the chain in
// order, and the entry-API only inserts when the key is absent.

class Base {
  static greet(): string { return "Base.greet" }
  static x: number = 10
}

class Sub extends Base {
  static y: number = 20
  static byeSub(): string { return "Sub.byeSub" }
}

class GrandSub extends Sub {}

// Direct static call on owner.
console.log(Base.greet())              // Base.greet
console.log(Base.x)                    // 10

// Sub inherits Base.greet / Base.x and adds its own.
console.log(Sub.greet())               // Base.greet
console.log(Sub.x)                     // 10
console.log(Sub.y)                     // 20
console.log(Sub.byeSub())              // Sub.byeSub

// GrandSub transitively inherits both Base and Sub statics.
console.log(GrandSub.greet())          // Base.greet
console.log(GrandSub.x)                // 10
console.log(GrandSub.y)                // 20
console.log(GrandSub.byeSub())         // Sub.byeSub

// Override: subclass declares its own static with the same
// name → subclass entry wins (already in the table before the
// inheritance walk's or_insert fires).
class A2 {
  static label(): string { return "A2.label" }
}
class B2 extends A2 {
  static label(): string { return "B2.label" }
}
console.log(A2.label())                // A2.label
console.log(B2.label())                // B2.label
