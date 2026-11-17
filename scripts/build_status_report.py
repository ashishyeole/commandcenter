#!/usr/bin/env python3
"""Generate a self-contained HTML report from the manually assessed task data.
This is report generation, not feature verification or automated source assessment.
"""
from pathlib import Path
from collections import Counter
import json
from datetime import date
from html import escape

ROOT=Path(__file__).resolve().parents[1]
source=ROOT/'docs/status-report-data.json'
data=json.loads(source.read_text())
tasks=data['tasks']
assessed=date.fromisoformat(data['assessed_on'])
assessed_label=f'{assessed:%B} {assessed.day}, {assessed.year}'
def e(value): return escape(str(value),quote=True)
def pct(rows):
    rows=[t for t in rows if t.get('in_scope',True)]
    weight=sum(t['weight'] for t in rows)
    return round(sum(t['weight']*t['percent'] for t in rows)/weight) if weight else 0
def style(status):
    return {'Implemented in source':'implemented','Partially implemented':'partial','Not started':'planned','Blocked by environment':'blocked','Deferred / unverified':'deferred'}[status]
def bar(value): return f'<div class="mini-bar" role="img" aria-label="{value}% implementation estimate"><span style="width:{value}%"></span></div>'
active_tasks=[t for t in tasks if t.get('in_scope',True)]
counts=Counter(t['status'] for t in active_tasks)
complete=counts['Implemented in source']
partial=counts['Partially implemented']
remaining=len(active_tasks)-complete
overall=pct(tasks)
metrics=f'''<article class="metric"><div class="metric-label">Local scope · code completion</div><div class="value">{overall}<small>%</small></div>{bar(overall)}<p>Weighted in-scope code coverage; excluded work has zero weight.</p></article>
<article class="metric"><div class="metric-label">Source-implemented tasks</div><div class="value">{complete}<small> / {len(active_tasks)}</small></div><p>Narrow source scope complete; verification varies by task.</p></article>
<article class="metric"><div class="metric-label">Tasks with remaining scope</div><div class="value">{remaining}</div><p>{partial} partial · {remaining-partial} not implemented, blocked or deferred.</p></article>
<article class="metric"><div class="metric-label">Live public starters verified</div><div class="value">{data.get('online_readiness',{}).get('public_sources_passed',0)}<small> / {data.get('online_readiness',{}).get('public_sources_enabled',0)}</small></div><p>Native Mac and installed Windows reads; no app account. <a href="USE-CASE-AUDIT.html">Use-case acceptance and dependencies</a> are separate.</p></article>'''
phases=''
for phase,label in [('V1','Standalone product'),('V2','Optional device continuity'),('V3+','Personal customization retained')]:
    rows=[t for t in tasks if t['phase']==phase];value=pct(rows)
    phases+=f'<article class="phase-card"><div class="row"><h3>{e(phase)} · {label}</h3><strong>{value}%</strong></div><p>{len(rows)} tasks · estimated implementation</p>{bar(value)}</article>'
area_names=list(dict.fromkeys(t['area'] for t in tasks))
areas=''
for name in area_names:
    rows=[t for t in tasks if t['area']==name];value=pct(rows);n=sum(t['percent']==100 for t in rows)
    areas+=f'<button class="area" data-area="{e(name)}" aria-label="Filter tasks for {e(name)}, {value}% estimated"><div class="area-top"><h3>{e(name)}</h3><strong>{value}%</strong></div><p>{e(rows[0]["phase"])} · {len(rows)} tasks · {n} source-implemented</p>{bar(value)}</button>'
rows_html=''
for t in tasks:
    score=t['percent']; scope_label=str(score)+'%' if t.get('in_scope',True) else 'Excluded'; rows_html+=f'''<tr><td class="task-id">{t['id']}</td><td><div class="task-title">{e(t['task'])}</div><div class="task-area">{e(t['area'])} · PRD {e(t['prd'])}</div><div class="muted">Scope weight: {t['weight']}</div></td><td><span class="badge">{e(t['phase'])}</span></td><td class="progress-cell"><div class="progress-number">{scope_label}</div>{bar(score) if t.get("in_scope",True) else ""}<span class="badge {style(t['status'])}">{e(t['status'])}</span></td><td><div class="remaining">{e(t['remaining'])}</div><details class="row-detail"><summary>Evidence & notes</summary><p><strong>Implemented:</strong> {e(t['implemented'])}</p><p><strong>Notes:</strong> {e(t['notes'])} {e(t.get('scope_note',''))}</p><p><strong>Evidence:</strong> <a href="../{e(t['evidence'])}">{e(t['evidence'])}</a></p><p><strong>Dependencies:</strong> {e(t.get("dependency_type","Local functionality / optional source access"))}. {e("; ".join(t.get("external_dependencies",[])))}</p><p><strong>Owner actions:</strong> {e("; ".join(t.get("owner_actions",[])) or "None for local coding; optional provider setup is listed where applicable.")}</p><p><strong>Verification:</strong> {e(t.get('verification','Full feature acceptance not established; see the test report for selected checks.'))}</p></details></td></tr>'''
acceptance=''.join(f'<details><summary><span class="task-id">{a["id"]}</span><strong>{e(a["workflow"])}</strong><span class="badge deferred">{e(a["verification"])}</span></summary><p>{e(a["notes"])}</p></details>' for a in data['acceptance'])
blockers=''.join(f'<article class="next-card"><h3>{e(b["title"])}</h3><p>{e(b["detail"])}</p><small>{e(b["tasks"])}</small></article>' for b in data.get('blockers',[]))
replacements={'__ASSESSED_DATE__':assessed_label,'__ASSESSED_ISO__':data['assessed_on'],'__BLOCKERS__':blockers,'__VERSION__':e(data['version']),'__METRICS__':metrics,'__PHASES__':phases,'__METHOD__':e(data['method']),'__AREAS__':areas,'__AREA_OPTIONS__':''.join(f'<option>{e(n)}</option>' for n in area_names),'__STATUS_OPTIONS__':''.join(f'<option>{e(n)}</option>' for n in counts),'__COUNT__':str(len(tasks)),'__ROWS__':rows_html,'__ACCEPTANCE__':acceptance,'__DATA__':json.dumps(data,ensure_ascii=False).replace('<','\\u003c').replace('>','\\u003e').replace('&','\\u0026')}
html=(ROOT/'docs/status-report-template.html').read_text()
for token,value in replacements.items():html=html.replace(token,value)
out=ROOT/'docs/STATUS-REPORT.html';out.write_text(html)
print(f'Created {out}\n{len(tasks)} tasks | {len(area_names)} workstreams | Local-scope estimate {overall}%')
print('Phase estimates: '+', '.join(f'{p}: {pct([t for t in tasks if t["phase"]==p])}%' for p in ['V1','V2','V3+']))
