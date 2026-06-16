// S131-3 narrow: top-level `: MapIter<T>` / `: ArrIter<T>` generic
// type-ann acceptance. Pre-fix, `const it: MapIter<string> = m.keys()`
// was rejected at typecheck phase with "unknown type `MapIter<string>`"
// because the generic-arg arm in check_type_ann.rs only enumerated
// Map / Set / WeakMap / WeakSet for the parametric `Foo<T>` decoder.
// The bare-ident arm (lower-case `mapiter` / `arriter`) already
// accepted both — generic `MapIter<T>` is the spelling users actually
// write at TS surface, mirroring how `Map<K,V>` / `Set<T>` work.
//
// Same as a4f39cf9..ba1fa498's Date arm: the lower-case spelling is
// what `type_to_ann` emits internally; the PascalCase generic form is
// what surface code writes.
//
// Class-field init inference (`class H { it: MapIter<string> = … }`)
// hits the same `init has Number` __this-struct bug as Date — deep
// substrate, deferred L3b.

const m = new Map<string, number>();
m.set("a", 1);
m.set("b", 2);
m.set("c", 3);

const keys_iter: MapIter<string> = m.keys();
console.log(keys_iter.next().value);
console.log(keys_iter.next().value);
console.log(keys_iter.next().value);
console.log(keys_iter.next().done);

const values_iter: MapIter<number> = m.values();
console.log(values_iter.next().value);
console.log(values_iter.next().value);

// ArrIter<T> generic ann — Array<Any>.values() returns the
// canonical iterator handle in tr's subset (typed Array<T> elem-tag
// substrate is independent work; see existing array-iter-001.ts).
const arr: any[] = [10, 20, 30];
const arr_iter: ArrIter<any> = arr.values();
console.log(arr_iter.next().value);
console.log(arr_iter.next().value);
console.log(arr_iter.next().done);
