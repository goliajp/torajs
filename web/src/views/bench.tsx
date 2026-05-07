import { useEffect, useMemo, useState } from 'react'
import { Link } from 'react-router'

/**
 * T-23 (v0.6.0) — bench scoreboard auto-render.
 *
 * Mounts at `/bench`. Fetches `/bench-latest.json` (the most recent
 * file from `bench/results/*.json`, copied into `web/public/` at
 * build time) and renders the cross-runtime row table sorted by
 * case name. Per-runtime tooltip with stddev + artifact size on
 * hover. The data shape mirrors `bench/harness/src/output.rs`.
 *
 * The Bench section on the landing page (`Landing` → `Bench`) is a
 * curated "win" excerpt with hand-picked headline numbers; this
 * page is the full table for anyone who wants to verify, sort, or
 * compare across runtimes case-by-case.
 */

type Row = {
  case: string
  runtime: string
  runtime_version: string | null
  status: 'ok' | 'fail' | 'skip'
  compile_ms: number | null
  run_ms: number | null
  run_stddev_ms: number | null
  artifact_bytes: number | null
  stdout_match: boolean
  error: string | null
}

type Result = {
  started_at: string
  host: string
  git_sha: string
  rows: Row[]
}

const RUNTIME_ORDER = [
  'torajs',
  'torajs-run',
  'bun-aot',
  'bun-jsc',
  'node-v8',
  'rust',
  'go',
  'python',
] as const

export function Bench() {
  const [data, setData] = useState<Result | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    fetch('/bench-latest.json')
      .then((r) => {
        if (!r.ok) throw new Error(`bench-latest.json: ${r.status}`)
        return r.json()
      })
      .then((j: Result) => setData(j))
      .catch((e: Error) => setError(e.message))
  }, [])

  const grouped = useMemo(() => groupByCase(data?.rows ?? []), [data])

  return (
    <main className="bg-ink text-bone min-h-screen">
      <Header />
      <section className="mx-auto max-w-[1080px] px-6 py-16">
        <h1 className="wordmark-roman text-[44px] leading-[0.95] sm:text-[56px]">
          <span className="text-tiger">Bench</span> scoreboard
        </h1>
        {data && (
          <p className="text-bone-dim mt-4 font-mono text-[12px] tracking-[0.04em]">
            run {data.started_at} · host {data.host} · git{' '}
            <code className="text-bone">{data.git_sha.slice(0, 7)}</code>
          </p>
        )}
        {error && (
          <p className="mt-6 font-mono text-[13px] text-red-400">
            failed to load bench data: {error}
          </p>
        )}

        {!data && !error && <p className="text-bone-dim mt-6 font-mono text-[13px]">loading…</p>}

        {data && (
          <div className="mt-10 overflow-x-auto">
            <table className="min-w-full font-mono text-[12.5px]">
              <thead>
                <tr className="border-b border-amber-900/40 text-left text-[10.5px] tracking-[0.18em] text-amber-300/70 uppercase">
                  <th className="py-2 pr-4">case</th>
                  {RUNTIME_ORDER.map((rt) => (
                    <th key={rt} className="py-2 pr-4">
                      {rt}
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {Object.keys(grouped)
                  .sort()
                  .map((caseName) => (
                    <tr
                      key={caseName}
                      className="border-b border-amber-900/15 hover:bg-amber-900/10"
                    >
                      <td className="text-bone py-2 pr-4">{caseName}</td>
                      {RUNTIME_ORDER.map((rt) => {
                        const row = grouped[caseName].find((r) => r.runtime === rt)
                        return (
                          <td key={rt} className="py-2 pr-4">
                            <Cell
                              row={row}
                              torajsRow={grouped[caseName].find((r) => r.runtime === 'torajs')}
                            />
                          </td>
                        )
                      })}
                    </tr>
                  ))}
              </tbody>
            </table>
          </div>
        )}

        {data && (
          <p className="text-bone-dim mt-10 max-w-[680px] text-[14px] leading-[1.6]">
            Times are wall-clock (hyperfine, 5 runs / 2 warmup unless overridden per case). torajs
            row is the AOT binary; torajs-run is the dev-loop interpreter (cache hit + native exec).
            Cells lighter than torajs's entry mean torajs is slower; darker mean torajs is faster.
            Skipped cells = the language has no source file for that case.
          </p>
        )}
      </section>
    </main>
  )
}

function Cell({ row, torajsRow }: { row?: Row; torajsRow?: Row }) {
  if (!row || row.status === 'skip') {
    return <span className="text-bone-dim">—</span>
  }
  if (row.status === 'fail') {
    return <span className="text-red-400">fail</span>
  }
  const ms = row.run_ms
  if (ms === null) return <span className="text-bone-dim">—</span>
  const tr = torajsRow?.run_ms ?? null
  const ratio = tr !== null && row.runtime !== 'torajs' ? ms / tr : null
  const tone =
    row.runtime === 'torajs'
      ? 'text-tiger'
      : ratio !== null && ratio >= 2
        ? 'text-amber-300'
        : ratio !== null && ratio >= 1.05
          ? 'text-bone'
          : 'text-bone-dim'
  return (
    <span
      className={tone}
      title={row.run_stddev_ms ? `± ${row.run_stddev_ms.toFixed(2)} ms` : undefined}
    >
      {ms.toFixed(2)}
    </span>
  )
}

function Header() {
  return (
    <header className="mx-auto flex max-w-[1080px] items-center justify-between px-6 pt-6">
      <Link
        to="/"
        className="wordmark-roman text-bone text-[26px] tracking-tight"
        style={{ letterSpacing: '-0.05em' }}
      >
        <span className="text-tiger">tora</span>
        <span className="opacity-90">js</span>
      </Link>
      <nav className="text-bone-dim font-mono text-[11.5px] tracking-[0.18em] uppercase">
        <Link to="/" className="hover:text-tiger-bright transition-colors">
          ← Home
        </Link>
      </nav>
    </header>
  )
}

function groupByCase(rows: Row[]): Record<string, Row[]> {
  const out: Record<string, Row[]> = {}
  for (const r of rows) {
    if (!out[r.case]) out[r.case] = []
    out[r.case].push(r)
  }
  return out
}
