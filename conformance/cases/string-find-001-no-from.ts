// P3.1-g.3 — exercise __torajs_str_{starts_with,ends_with,index_of,includes}.
console.log("hello".startsWith("hel"));      // true
console.log("hello".startsWith("ell"));      // false
console.log("hello".startsWith(""));         // true
console.log("hello".endsWith("llo"));        // true
console.log("hello".endsWith("hel"));        // false
console.log("hello".endsWith(""));           // true
console.log("hello".indexOf("ell"));         // 1
console.log("hello".indexOf("z"));           // -1
console.log("hello".indexOf(""));            // 0
console.log("hello".includes("ell"));        // true
console.log("hello".includes("z"));          // false
console.log("hello".includes(""));           // true
