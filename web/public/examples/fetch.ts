// HTTP fetch via libcurl (native target). Sync MVP — every Promise
// is settled by the time control reaches its await.
//
// Wasm target (`tr build --target wasm32-wasi`) routes through the
// browser fetch API instead; substrate ships in v0.6+1.

let resp = await fetch('https://httpbin.org/get')
console.log('status:', resp.status)
let body = await resp.text()
console.log('bytes:', body.length)
