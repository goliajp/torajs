// Legal shapes the block/CaseBlock redeclaration early-error pass
// (§14.2.1 / §14.12.1, ast_early_redecl) must NOT reject. The
// illegal matrix (lexical dup, lexical∩var, dup default) is
// REJ-BOTH-verified against bun and stays out of the case set —
// negative shapes have no stdout to compare.

// var × var in one block is legal everywhere
{
  var a = 1;
  var a = 2;
  console.log(a);
}

// same lexical name in sibling blocks
{
  let b = 1;
  console.log(b);
}
{
  let b = 2;
  console.log(b);
}

// plain function × plain function in one block (Annex B.3.3.4 —
// bun accepts; the second declaration wins)
{
  function f() {
    console.log("f1");
  }
  function f() {
    console.log("f2");
  }
  f();
}

// fn-body top level var + fn stays outside the pass's block scope
function g() {
  var h = 1;
  console.log(h);
}
g();

// switch clauses may reuse a name inside their OWN nested blocks
switch (1) {
  case 1: {
    let c = 1;
    console.log(c);
    break;
  }
  default: {
    let c = 2;
    console.log(c);
  }
}
