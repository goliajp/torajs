// §8.2.6 BlockDeclarationInstantiation creates a binding for every
// lexical declaration of a block when the block is ENTERED, so a
// closure minted earlier in the block already names the block's
// binding rather than an outer one of the same name.
//
// Only the let-initializer shape was looked at before, so every other
// statement position that mints a closure captured the outer binding.

function assignment(): string {
  var x = "outer";
  var probe: any;
  {
    probe = function () { return x; };
    let x = "block";
  }
  return probe();
}

function callArgument(): string {
  var x = "outer";
  var kept: any[] = [];
  {
    kept.push(function () { return x; });
    let x = "block";
  }
  return kept[0]();
}

function nestedBlock(): string {
  var x = "outer";
  var probe: any;
  {
    { probe = function () { return x; }; }
    let x = "block";
  }
  return probe();
}

function loopBody(): string {
  var x = "outer";
  var probe: any;
  {
    for (let i = 0; i < 1; i++) { probe = function () { return x; }; }
    let x = "block";
  }
  return probe();
}

function condition(): string {
  var x = "outer";
  var probe: any;
  {
    if ((probe = function () { return x; })) {}
    let x = "block";
  }
  return probe();
}

function constBinding(): string {
  var x = "outer";
  var probe: any;
  {
    probe = function () { return x; };
    const x = "block";
  }
  return probe();
}

// The write is what makes the outer binding capturable at all, so the
// mutated form is the one that used to answer "outer".
function writtenThrough(): string {
  var x = "outer";
  var probe: any;
  {
    probe = function () { x = x + "!"; return x; };
    let x = "block";
  }
  return probe();
}

// A closure that captures a binding of the block it is IN keeps that
// binding — the enclosing list's same-named one is untouched.
function innerShadow(): string {
  let x = "outer";
  var probe: any;
  {
    { probe = function () { return x; }; let x = "inner"; }
    let x = "middle";
  }
  return probe() + "/" + x;
}

console.log(assignment());
console.log(callArgument());
console.log(nestedBlock());
console.log(loopBody());
console.log(condition());
console.log(constBinding());
console.log(writtenThrough());
console.log(innerShadow());

// The same at top level, where the outer binding is a data global.
var g = "outer";
var topProbe: any;
{
  topProbe = function () { return g; };
  let g = "block";
}
console.log(topProbe());
