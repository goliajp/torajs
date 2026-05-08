import Editor, { loader } from '@monaco-editor/react'
import { useCallback, useEffect, useRef, useState } from 'react'
import { Link, useSearchParams } from 'react-router'

/**
 * T-22 (v0.6.0) — Playground.
 *
 * Phase 1 (this commit): Monaco editor + curated examples + URL-
 * encoded share-link. The Run button surfaces a clear "run locally
 * with `tr run` — server-side compile lands in v0.6+1" panel
 * instead of a half-implementation; per the project's
 * no-tech-debt rule the actual remote-compile substrate
 * (sandboxing + rate limiting + deploy) ships as a separate
 * sub-step (T-22.b).
 *
 * URL share: source is encoded into the `?src=...` query
 * (compressed via the browser's CompressionStream + base64url) so
 * a torajs.com/playground link can carry the program inline.
 *
 * Examples: `web/public/examples/*.ts` are committed alongside
 * the build and lazy-loaded into the editor on click. Adding a
 * new example is a single commit (drop a `.ts` file + add it to
 * the `EXAMPLES` array below).
 */

const EXAMPLES = [
  { path: 'hello.ts', label: 'hello, torajs' },
  { path: 'promise-chain.ts', label: 'Promise.then chain (12x faster)' },
  { path: 'closure.ts', label: 'Capturing closures + .then' },
  { path: 'fetch.ts', label: 'fetch (HTTP via libcurl)' },
  { path: 'fs-read.ts', label: 'fs/promises round-trip' },
] as const

const DEFAULT_SOURCE = `// Welcome to the torajs playground.
//
// • Pick an example on the left, or write TS here and copy the
//   share-link button to send it.
// • Run locally with \`tr run\`:
//     curl -fsSL https://install.torajs.com | bash
//     tr run yourfile.ts
// • Server-side run in the browser ships in v0.6+1 (T-22.b).

console.log('hello from torajs')
`

loader.config({
  paths: { vs: 'https://cdn.jsdelivr.net/npm/monaco-editor@0.55.1/min/vs' },
})

export function Playground() {
  const [source, setSource] = useState<string>(DEFAULT_SOURCE)
  const [params, setParams] = useSearchParams()
  const [shareCopied, setShareCopied] = useState(false)
  const [runOpen, setRunOpen] = useState(false)
  const initialLoadDone = useRef(false)

  /* On first mount, hydrate from `?src=...` if present. The decode
   * is async (CompressionStream API) so we use a one-shot effect
   * and gate further saves with `initialLoadDone`. */
  useEffect(() => {
    if (initialLoadDone.current) return
    initialLoadDone.current = true
    const encoded = params.get('src')
    if (encoded) {
      decodeShare(encoded)
        .then((s) => setSource(s))
        .catch(() => {
          /* malformed share — keep the default */
        })
    }
  }, [params])

  const onLoadExample = useCallback(async (path: string) => {
    const r = await fetch(`/examples/${path}`)
    if (r.ok) {
      const txt = await r.text()
      setSource(txt)
    }
  }, [])

  const onShare = useCallback(async () => {
    const encoded = await encodeShare(source)
    setParams({ src: encoded })
    const url = `${window.location.origin}/playground?src=${encoded}`
    try {
      await navigator.clipboard.writeText(url)
      setShareCopied(true)
      window.setTimeout(() => setShareCopied(false), 1800)
    } catch {
      /* clipboard blocked — the URL is in the address bar already */
    }
  }, [source, setParams])

  return (
    <main className="bg-ink text-bone min-h-screen">
      <Header />
      <section className="mx-auto max-w-[1400px] px-4 py-10 sm:px-6">
        <div className="flex flex-col gap-6 sm:flex-row sm:items-end sm:justify-between">
          <h1 className="wordmark-roman text-[40px] leading-[0.95] sm:text-[48px]">
            <span className="text-tiger">Playground</span>
          </h1>
          <div className="flex gap-3">
            <button
              onClick={onShare}
              className="text-bone hover:border-tiger hover:text-tiger border border-amber-700/50 bg-amber-950/20 px-4 py-2 font-mono text-[12px] tracking-[0.14em] uppercase transition-colors"
            >
              {shareCopied ? 'copied ✓' : 'copy share link'}
            </button>
            <button
              onClick={() => setRunOpen((v) => !v)}
              className="border-tiger bg-tiger/10 text-tiger hover:bg-tiger/20 border px-4 py-2 font-mono text-[12px] tracking-[0.14em] uppercase transition-colors"
            >
              ▶ run
            </button>
          </div>
        </div>

        <div className="mt-8 grid gap-6 sm:grid-cols-[220px_1fr]">
          <aside className="font-mono text-[12.5px]">
            <div className="text-bone-faint mb-3 text-[10.5px] tracking-[0.18em] uppercase">
              examples
            </div>
            <ul className="space-y-1.5">
              {EXAMPLES.map((ex) => (
                <li key={ex.path}>
                  <button
                    onClick={() => onLoadExample(ex.path)}
                    className="text-bone-dim hover:text-tiger w-full text-left transition-colors"
                  >
                    {ex.label}
                  </button>
                </li>
              ))}
            </ul>
            <p className="text-bone-faint mt-8 text-[11px] leading-[1.6]">
              Tip: any TS that <code className="text-bone">tr run</code> accepts works here. The
              editor doesn't typecheck against tr's substrate — copy the share link and run locally
              to see real diagnostics.
            </p>
          </aside>

          <div className="overflow-hidden border border-amber-900/30">
            <Editor
              height="540px"
              theme="vs-dark"
              defaultLanguage="typescript"
              value={source}
              onChange={(v) => setSource(v ?? '')}
              options={{
                fontSize: 13,
                fontFamily: "'JetBrains Mono', ui-monospace, SFMono-Regular, monospace",
                minimap: { enabled: false },
                scrollBeyondLastLine: false,
                tabSize: 2,
                wordWrap: 'on',
              }}
            />
          </div>
        </div>

        {runOpen && (
          <div className="border-tiger/40 bg-tiger/5 mt-6 border p-5 font-mono text-[12.5px]">
            <div className="text-tiger mb-3 text-[10.5px] tracking-[0.18em] uppercase">
              run locally
            </div>
            <p className="text-bone-dim leading-[1.7]">
              The browser-side wasm runner with sandboxed compile lands in{' '}
              <strong className="text-bone">v0.6+1</strong> (T-22.b). For now, copy the source and
              run with the local <code className="text-bone">tr</code> CLI:
            </p>
            <pre className="text-bone bg-ink/60 mt-4 overflow-x-auto p-4">
              {`# install (once)
curl -fsSL https://install.torajs.com | bash

# save your source as foo.ts, then:
tr run foo.ts

# or AOT to a tiny native binary:
tr build foo.ts -o foo
./foo

# wasm32-wasi target:
tr build foo.ts --target wasm32-wasi -o foo.wasm
wasmtime foo.wasm`}
            </pre>
            <button
              onClick={() => setRunOpen(false)}
              className="text-bone-dim hover:text-bone mt-4 text-[11px] tracking-[0.16em] uppercase"
            >
              dismiss
            </button>
          </div>
        )}
      </section>
    </main>
  )
}

function Header() {
  return (
    <header className="mx-auto flex max-w-[1400px] items-center justify-between px-4 pt-6 sm:px-6">
      <Link
        to="/"
        className="wordmark-roman text-bone text-[26px] tracking-tight"
        style={{ letterSpacing: '-0.05em' }}
      >
        <span className="text-tiger">tora</span>
        <span className="opacity-90">js</span>
      </Link>
      <nav className="text-bone-dim flex gap-5 font-mono text-[11.5px] tracking-[0.18em] uppercase">
        <Link to="/" className="hover:text-tiger-bright transition-colors">
          ← Home
        </Link>
        <Link to="/bench" className="hover:text-tiger-bright transition-colors">
          Bench
        </Link>
      </nav>
    </header>
  )
}

/* URL share-link codec.
 *
 * gzip → base64url. The CompressionStream API is in every modern
 * browser; we use it to keep share-links short (typical 80% size
 * reduction for human-readable TS). The encoded string is stuffed
 * into the `?src=` query — both decode and encode are async because
 * CompressionStream is stream-shaped. */
async function encodeShare(text: string): Promise<string> {
  const cs = new CompressionStream('gzip')
  const stream = new Blob([text]).stream().pipeThrough(cs)
  const buf = await new Response(stream).arrayBuffer()
  return base64urlEncode(new Uint8Array(buf))
}

async function decodeShare(encoded: string): Promise<string> {
  const bytes = base64urlDecode(encoded)
  const ds = new DecompressionStream('gzip')
  const stream = new Blob([bytes as BlobPart]).stream().pipeThrough(ds)
  return await new Response(stream).text()
}

function base64urlEncode(bytes: Uint8Array): string {
  let bin = ''
  for (let i = 0; i < bytes.length; i++) bin += String.fromCharCode(bytes[i])
  return btoa(bin).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '')
}

function base64urlDecode(s: string): Uint8Array {
  const pad = (4 - (s.length % 4)) % 4
  const std = s.replace(/-/g, '+').replace(/_/g, '/') + '='.repeat(pad)
  const bin = atob(std)
  const out = new Uint8Array(bin.length)
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i)
  return out
}
