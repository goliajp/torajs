// The cycle collector's collect_white second sweep dropped every
// surviving child with rc > 0 — but a walkable child that survived
// as BLACK had this dying parent's edge trial-decremented in mark,
// and scan_black only restores edges out of BLACK parents. The drop
// charged the same edge twice.
//
// Three sibling subclasses of one base are the smallest shape where
// %Object.prototype% collects two WHITE class-proto parents on top
// of its BLACK-restored rc of two: the two extra decs ran it
// straight through zero at the at-exit drain, freeing the live
// singleton out from under the BLACK half of the prototype graph
// (with two subclasses the same double-charge left rc at one and
// stayed invisible). The `.length` read inside a function is what
// materializes the class-object graph on the failing path.
class Base {
  v: number = 5;
}
class D1 extends Base {
  a: number = 1;
}
class D2 extends Base {
  b: number = 2;
}
class D3 extends Base {
  c: number = 3;
}
function main(): void {
  console.log(D1.length);
}
main();
