// sec 14.14 — a thrown function value must arrive at the catch
// binding as the SAME callable object, not a raw code address.
function bar(n: number) { console.log("bar ran", n); return n + 1; }
try { throw bar } catch (e) {
  console.log(typeof e);
  console.log((e as any)(41));
}
// strict this: plain call through the caught value sees undefined
function useThis() { (this as any).marker = 1; }
try { throw useThis } catch (e) {
  try { (e as any)(); } catch (inner) {
    console.log("inner", (inner as any)?.constructor?.name);
  }
}
console.log("survived");
