// Annex B §B.3.3 block functions that capture, reached before their text
// through a peer. A block's function declarations are all initialized on
// block entry (§14.2.3), so `g()` above both declarations runs `f` too.
//
// bun is not an oracle here — it reads every file as strict code, where
// there is no var binding (`typeof h` after the block). node follows the
// spec, so this checks against a `.expected`.

function viaPeer() {
  let c = 1;
  let r = 0;
  {
    r = g();
    function f() { return c; }
    function g() { return f(); }
  }
  return r;
}
console.log(viaPeer());

function chain() {
  let c = 10;
  let r = 0;
  {
    r = a();
    function c3() { return c; }
    function b() { return c3() + 1; }
    function a() { return b() + 1; }
  }
  return r;
}
console.log(chain());

// a peer nothing above reaches still gets its var binding
function split() {
  let c = 2;
  let r = 0;
  {
    r = g();
    function g() { return c; }
    function h() { return c * 3; }
  }
  return [r, typeof h, h()].join(",");
}
console.log(split());
