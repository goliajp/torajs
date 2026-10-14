// §14.2.1 — a Block's LexicallyDeclaredNames may repeat only when the
// code is not strict and every duplicate is a plain FunctionDeclaration
// (the Annex B allowance). This file is a sloppy script, so both blocks
// are legal and the last declaration wins, giving the block ONE
// binding. The same shapes in strict code are a SyntaxError.
{
  function f() {
    console.log("f1");
  }
  function f() {
    console.log("f2");
  }
  f();
}

function outer() {
  {
    function twin() {
      return "first";
    }
    function twin() {
      return "second";
    }
    console.log(twin());
  }
}
outer();

const viaExpr = function () {
  {
    function g() {
      return 1;
    }
    function g() {
      return 2;
    }
    return g();
  }
};
console.log(viaExpr());
