// Tail calls through an `any` callee in strict code (ES2015 proper tail
// calls, sec-isintailposition): the callee is a call's result, a
// tagged template's tag read off a call, or a binding typed `any`.
// 100k rounds overflow the stack unless each round's frame is gone
// before the next one runs.
const DEPTH = 100000;

// a callee that is a call's result is held by the frame, which keeps
// a reference on it until the call returns; the depth stays small
const OWNED_DEPTH = 1000;

// the callee is what a per-round closure answers; the closure itself
// is a local of the round and is released before the next round
let calls = 0;
(function f(n: any): any {
  if (n === 0) {
    calls += 1;
    return;
  }
  function getF() {
    return f;
  }
  return getF()(n - 1);
})(OWNED_DEPTH);
console.log("call result", calls);

// a tagged template whose tag is a call's result
const tagged = (function () {
  let finished = false;
  function getF() {
    return f;
  }
  function f(_: any, n: any): any {
    if (n === 0) {
      finished = true;
      return;
    }
    return getF()`${n - 1}`;
  }
  f(null, OWNED_DEPTH);
  return finished;
})();
console.log("tagged", tagged);

// the callee is a binding typed any, two functions taking turns
const pingPong = (function () {
  let ping: any;
  let pong: any;
  ping = function (n: any): any {
    if (n === 0) return "ping";
    return pong(n - 1);
  };
  pong = function (n: any): any {
    if (n === 0) return "pong";
    return ping(n - 1);
  };
  return ping;
})();
console.log("ping-pong", pingPong(DEPTH), pingPong(DEPTH + 1));

// a throw at the bottom passes up through every round
const deepThrow = (function () {
  let g: any;
  g = function (n: any): any {
    if (n === 0) throw new Error("bottom");
    return g(n - 1);
  };
  return g;
})();
try {
  deepThrow(DEPTH);
} catch (e: any) {
  console.log("caught", e.message);
}

// shapes the callee must still see right; the depth stays small

// a callee only the call's own result holds: it is released after it
const fresh = (function () {
  function make(k: any): any {
    return (n: any): any => n * 10 + k;
  }
  function f(n: any): any {
    return make(n)(n + 1);
  }
  return f;
})();
console.log("fresh callee", fresh(4), fresh(7));

// a callee only a local of the frame holds
const localOnly = (function () {
  function f(n: any): any {
    const g: any = (x: any): any => x + 1;
    return g(n);
  }
  return f;
})();
console.log("local callee", localOnly(41));

// fewer and more arguments than parameters
const arity = (function () {
  let h: any = function (a: any, b: any, c: any): any {
    return [a, b, c].join("/");
  };
  function fewer(n: any): any {
    return h(n);
  }
  function more(n: any): any {
    return h(n, 2, 3, 4, 5);
  }
  return [fewer(1), more(1)];
})();
console.log("arity", arity[0], arity[1]);

// arguments that are not immediates keep the ordinary call
const heapArgs = (function () {
  let h: any = function (s: any, o: any): any {
    return s + ":" + o.k;
  };
  function f(n: any): any {
    return h("s" + n, { k: n });
  }
  return f;
})();
console.log("heap args", heapArgs(3));

// a value that is not callable throws a TypeError
const notCallable = (function () {
  function f(n: any): any {
    const v: any = n;
    return v(1);
  }
  return f;
})();
try {
  notCallable(5);
} catch (e: any) {
  console.log("not callable", e instanceof TypeError);
}

// a Proxy callee runs its apply trap
const proxied = (function () {
  const target: any = function (n: any): any {
    return n + 1;
  };
  const p: any = new Proxy(target, {
    apply(t: any, _this: any, args: any): any {
      return "trap " + t(args[0]);
    },
  });
  function f(n: any): any {
    return p(n);
  }
  return f;
})();
console.log("proxy", proxied(1));

// a callee that reassigns the binding it was read from still runs
const rebind = (function () {
  let slot: any = function (n: any): any {
    slot = null;
    return "ran " + n;
  };
  function f(n: any): any {
    return slot(n);
  }
  return f;
})();
console.log("rebind", rebind(9));
