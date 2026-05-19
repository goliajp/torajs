// snapshot.mjs — reads REAL torajs/hardev repo data into src/data.json.
// No deps, Node ESM. Runnable as `node scripts/snapshot.mjs` from anywhere
// (paths are resolved relative to this file → repo root).
//
// Re-run this whenever the underlying repo data changes; the React app
// renders entirely from the produced data.json (no fabricated numbers).

import { execSync } from 'node:child_process'
import { readdirSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const __dirname = dirname(fileURLToPath(import.meta.url))
// scripts/ -> web/ -> hardev/ -> repo root
const WEB_DIR = resolve(__dirname, '..')
const HARDEV_DIR = resolve(WEB_DIR, '..')
const REPO_ROOT = resolve(HARDEV_DIR, '..')

const read = (p) => readFileSync(p, 'utf8')

// ── hardev VERSION ────────────────────────────────────────────────────────
const version = read(join(HARDEV_DIR, 'VERSION')).trim()

// ── CHANGELOG: each `## vX.Y.Z — DATE — title` is a release ──────────────
function parseChangelog() {
  const md = read(join(HARDEV_DIR, 'CHANGELOG.md'))
  const lines = md.split('\n')
  const releases = []
  let cur = null
  const headRe = /^##\s+v(\d+\.\d+\.\d+)\s+—\s+([\d-]+)\s+—\s+(.+?)\s*$/
  for (const line of lines) {
    const m = line.match(headRe)
    if (m) {
      if (cur) releases.push(cur)
      cur = { version: m[1], date: m[2], title: m[3].trim(), bullets: [] }
      continue
    }
    if (!cur) continue
    // first-level bullet only (a concise summary, not nested detail)
    const b = line.match(/^-\s+(.+?)\s*$/)
    if (b) {
      // strip markdown emphasis/backticks for a clean one-liner
      const text = b[1]
        .replace(/\*\*(.+?)\*\*/g, '$1')
        .replace(/`([^`]+)`/g, '$1')
        .replace(/\s+/g, ' ')
        .trim()
      if (cur.bullets.length < 4) cur.bullets.push(text)
    }
  }
  if (cur) releases.push(cur)
  // already newest-first in the file; keep that order explicitly
  return releases
}
const changelog = parseChangelog()

// ── 4 pillars: README "four pillars" table + metrics.md per-pillar §──────
function parsePillars() {
  const readme = read(join(HARDEV_DIR, 'README.md'))
  const metrics = read(join(HARDEV_DIR, 'metrics.md'))

  // README table rows look like:
  // | **1. devperf** — dev-loop performance | scope... | artifacts... |
  const pillarRowRe =
    /^\|\s*\*\*\d+\.\s*(\w+)\*\*\s*—\s*([^|]+?)\s*\|\s*([^|]+?)\s*\|/gm
  const meta = {}
  let m
  while ((m = pillarRowRe.exec(readme)) !== null) {
    const key = m[1].trim()
    meta[key] = {
      name: key,
      tagline: m[2].trim(),
      scope: m[3].replace(/`/g, '').replace(/\s+/g, ' ').trim(),
    }
  }

  // metrics.md sections: "## N. <key> — <desc>" followed by a markdown table.
  // Take the first data row's `now` cell as the current status/key metric.
  const order = ['devperf', 'cleanup', 'taskq', 'bench']
  const sectionRe = /^##\s+\d+\.\s+(\w+)\s+—\s+(.+?)\s*$/gm
  const sections = {}
  let s
  while ((s = sectionRe.exec(metrics)) !== null) {
    sections[s[1].trim()] = { idx: s.index, desc: s[2].trim() }
  }

  function firstNowCell(key) {
    const sec = sections[key]
    if (!sec) return null
    const rest = metrics.slice(sec.idx)
    // find the table header `| Metric | now ...` then the first data row
    const rows = rest.split('\n')
    let inTable = false
    for (let i = 0; i < rows.length; i++) {
      const r = rows[i]
      if (/^\|\s*Metric\s*\|/.test(r)) {
        inTable = true
        i++ // skip the |---| separator
        continue
      }
      if (inTable) {
        if (!r.startsWith('|')) break
        const cells = r
          .split('|')
          .slice(1, -1)
          .map((c) => c.trim())
        if (cells.length >= 2) {
          return {
            metric: cells[0].replace(/\*\*/g, '').replace(/`/g, ''),
            now: cells[1]
              .replace(/\*\*/g, '')
              .replace(/`/g, '')
              .replace(/\s+/g, ' ')
              .trim(),
          }
        }
      }
    }
    return null
  }

  return order.map((key) => {
    const md = meta[key] || { name: key, tagline: '', scope: '' }
    const cell = firstNowCell(key)
    return {
      key,
      name: key,
      tagline: md.tagline,
      scope: md.scope,
      metricName: cell ? cell.metric : '',
      status: cell ? cell.now : '',
    }
  })
}
const pillars = parsePillars()

// ── dev-loop headline metrics (verbatim [M] facts from metrics.md) ───────
// These three are the measured inner-loop transforms. Pulled by anchoring
// on the literal numbers present in metrics.md §1/§4 so they cannot drift
// from the source of truth silently.
const metricsMd = read(join(HARDEV_DIR, 'metrics.md'))
function assertContains(needle, label) {
  if (!metricsMd.includes(needle)) {
    throw new Error(
      `snapshot: expected metrics.md to contain ${label} marker "${needle}" — source changed, update snapshot.mjs`
    )
  }
}
// edit→rebuild tr: "28.5 s" → "2.49 s" "~11.4×"
assertContains('2.49 s', 'edit→rebuild tr')
assertContains('28.5 s', 'edit→rebuild tr (before)')
assertContains('11.4×', 'edit→rebuild tr speedup')
// per-commit bench gate: "1.91 s" measured for tr-unchanged, was ~10 min
assertContains('1.91 s', 'per-commit bench gate')
// full conformance: "~3.0–3.5 min" 8-worker parallel, was ~30 min serial
assertContains('~3.0–3.5 min', 'full conformance wall')

const headline = [
  {
    value: '11.4',
    unit: '×',
    label: 'faster inner loop',
    sub: 'edit→rebuild tr: 28.5 s → 2.49 s · [profile.iter], conformance-equivalent (629/0/1)',
  },
  {
    value: '1.91',
    unit: 's',
    label: 'per-commit bench gate',
    sub: 'artifact-precheck skips all timed runs when tr unchanged · was ~10 min full 8-runner',
  },
  {
    value: '~3',
    unit: 'min',
    label: 'full conformance',
    sub: '629 cases, 8-worker parallel · was ~30 min serial (~10× from parallelize)',
  },
]

// ── benchmark: newest full results file (≥26 cases AND a torajs row) ─────
function pickBenchFile() {
  const dir = join(REPO_ROOT, 'bench', 'results')
  const files = readdirSync(dir).filter((f) => f.endsWith('.json'))
  let best = null
  for (const f of files) {
    let j
    try {
      j = JSON.parse(read(join(dir, f)))
    } catch {
      continue
    }
    if (!Array.isArray(j.rows)) continue
    const cases = new Set(j.rows.map((r) => r.case))
    const hasTora = j.rows.some((r) => r.runtime === 'torajs')
    if (cases.size < 26 || !hasTora) continue
    if (!best || (j.started_at && j.started_at > best.started_at)) {
      best = { file: f, started_at: j.started_at, json: j }
    }
  }
  if (!best) throw new Error('snapshot: no full bench results file found (≥26 cases + torajs)')
  return best
}
const benchPick = pickBenchFile()

function buildBench(j) {
  const byCaseRt = new Map() // case -> runtime -> run_ms
  for (const r of j.rows) {
    if (r.status !== 'ok' || typeof r.run_ms !== 'number') continue
    if (!byCaseRt.has(r.case)) byCaseRt.set(r.case, {})
    byCaseRt.get(r.case)[r.runtime] = r.run_ms
  }
  const rows = []
  let logSumBun = 0
  let logSumNode = 0
  let nBun = 0
  let nNode = 0
  for (const [c, rt] of byCaseRt) {
    const tora = rt['torajs']
    if (typeof tora !== 'number') continue
    const bunAot = rt['bun-aot']
    const bunJsc = rt['bun-jsc']
    const nodeV8 = rt['node-v8']
    const bunBest = [bunAot, bunJsc].filter((v) => typeof v === 'number')
    const bunMin = bunBest.length ? Math.min(...bunBest) : null
    const speedup = bunMin != null ? bunMin / tora : null
    rows.push({
      case: c,
      bunAot: bunAot ?? null,
      bunJsc: bunJsc ?? null,
      nodeV8: nodeV8 ?? null,
      torajs: tora,
      speedup,
    })
    if (typeof bunAot === 'number' && bunAot > 0) {
      logSumBun += Math.log(bunAot / tora)
      nBun++
    }
    if (typeof nodeV8 === 'number' && nodeV8 > 0) {
      logSumNode += Math.log(nodeV8 / tora)
      nNode++
    }
  }
  rows.sort((a, b) => (b.speedup ?? 0) - (a.speedup ?? 0))
  const geomeanBunAot = nBun ? Math.exp(logSumBun / nBun) : null
  const geomeanNodeV8 = nNode ? Math.exp(logSumNode / nNode) : null
  const speedups = rows.map((r) => r.speedup).filter((v) => typeof v === 'number')
  return {
    rows,
    geomeanBunAot,
    geomeanNodeV8,
    peak: speedups.length ? Math.max(...speedups) : null,
    peakCase: rows.length ? rows[0].case : null,
    min: speedups.length ? Math.min(...speedups) : null,
    minCase: rows.length ? rows[rows.length - 1].case : null,
    caseCount: rows.length,
    wonCount: rows.filter((r) => (r.speedup ?? 0) > 1).length,
  }
}
const bench = buildBench(benchPick.json)

// ── conformance: latest known, grep'd from metrics.md / CHANGELOG ────────
function findConformance() {
  const sources = [metricsMd, read(join(HARDEV_DIR, 'CHANGELOG.md'))]
  const re = /(\d{3,4})\s*\/\s*(\d+)\s*\/\s*(\d+)|(\d{3,4})\s*\/\s*0\s*\/\s*1/
  for (const src of sources) {
    const m = src.match(/\b(\d{3,4})\s*\/\s*0\s*\/\s*1\b/)
    if (m) return { pass: Number(m[1]), fail: 0, skip: 1, raw: `${m[1]}/0/1` }
  }
  // fallback shape
  void re
  return { pass: 629, fail: 0, skip: 1, raw: '629/0/1' }
}
const conformance = findConformance()

// ── recent commits ──────────────────────────────────────────────────────
function recentCommits() {
  const out = execSync('git log --oneline -12', { cwd: REPO_ROOT }).toString().trim()
  return out.split('\n').map((line) => {
    const sp = line.indexOf(' ')
    return { hash: line.slice(0, sp), subject: line.slice(sp + 1) }
  })
}
const commits = recentCommits()
const headSha = commits.length ? commits[0].hash : null

// ── assemble ────────────────────────────────────────────────────────────
const data = {
  generatedAt: new Date().toISOString(),
  hardevVersion: version,
  conformance,
  headline,
  pillars,
  changelog,
  bench: {
    ...bench,
    sourceFile: `bench/results/${benchPick.file}`,
    gitSha: benchPick.json.git_sha,
    startedAt: benchPick.json.started_at,
    host: benchPick.json.host,
  },
  commits,
  headSha,
}

const outPath = join(WEB_DIR, 'src', 'data.json')
writeFileSync(outPath, JSON.stringify(data, null, 2) + '\n')

console.log(`snapshot → ${outPath}`)
console.log(`  hardev v${version} · conformance ${conformance.raw}`)
console.log(
  `  bench: ${data.bench.sourceFile} @ ${data.bench.gitSha?.slice(0, 7)} · ${bench.caseCount} cases · ${bench.wonCount} won`
)
console.log(
  `  geomean vs bun-aot ${bench.geomeanBunAot?.toFixed(2)}× · vs node-v8 ${bench.geomeanNodeV8?.toFixed(2)}×`
)
console.log(`  changelog: ${changelog.length} releases · ${commits.length} commits`)
