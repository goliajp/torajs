// unshift over an any[] with holes or an accessor index moves each index
// through HasProperty / Get / Set / Delete (§23.1.3.36): a hole stays a
// hole one place up, a getter runs.
{ const a: any[] = [1.5, 2.5]; a[4] = 3.5; a.unshift(9); console.log(a, 1 in a, 3 in a); }
{ const a: any[] = [1.5, 2.5, , , 3.5]; a.unshift(9, 8); console.log(a, a.length); }
{ const a: any = [1, 2]; a[4] = "x"; console.log(a.unshift(0), a); }
{
  const a: any[] = [1, 2, 3];
  let seen = 0;
  Object.defineProperty(a, "1", { get() { seen++; return 42; }, set(v) {}, configurable: true });
  a.unshift(0);
  console.log(a[2], a[3], a.length, seen > 0);
}
{ const a: any[] = [1, 2]; a.unshift(0); console.log(a); }
