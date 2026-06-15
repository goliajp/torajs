// W-J Phase D — empty named class. struct_print.rs's
// `field_count == 0` branch emits `Name {}` single-line (no '\n'
// after `}`; inspect/any.rs Tag::Obj arm appends '\n' at the
// top-level console.log boundary). Mirrors bun's pretty form.
class Marker {}

const m = new Marker();
console.log(m);
