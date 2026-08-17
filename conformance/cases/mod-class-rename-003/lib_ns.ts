// Namespace lane: every export injects un-requested, so a colliding
// class rides the same census mangle; the ns field must point at the
// mangled binding while .name keeps reflecting the source spelling.
export class NC {
  v() { return 21; }
}
export function jn() { return new NC().v(); }
export function ncName() { return NC.name; }
