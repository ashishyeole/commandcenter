#!/usr/bin/env python3
"""Render recorded evidence; this does not execute or infer feature tests."""
import json
from html import escape
from pathlib import Path

root = Path(__file__).resolve().parents[1]
results = json.loads((root / 'docs/testing/results.json').read_text())
audit = json.loads((root / 'docs/testing/use-case-audit.json').read_text())


def page(title, introduction, headers, rows):
    head = ''.join(f'<th scope="col">{escape(h)}</th>' for h in headers)
    body = ''.join('<tr>' + ''.join(f'<td>{escape(str(c))}</td>' for c in row) + '</tr>' for row in rows)
    return f'''<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>{escape(title)}</title><style>body{{font:15px/1.65 system-ui;max-width:1250px;margin:36px auto;padding:0 24px;background:#f2f2f3;color:#202124}}h1{{font-size:28px}}p{{max-width:1000px}}table{{border-collapse:collapse;width:100%;background:white}}th,td{{text-align:left;padding:14px;border-bottom:1px solid #ddd;vertical-align:top}}th{{background:#252529;color:white;position:sticky;top:0}}td:first-child{{font-weight:600;white-space:nowrap}}a{{color:#194ca0}}.scroll{{overflow:auto}}@media(max-width:800px){{table{{min-width:800px}}}}</style></head><body><h1>{escape(title)}</h1>{introduction}<p><a href="STATUS-REPORT.html">95-requirement source/dependency report</a> · <a href="USE-CASE-AUDIT.html">Use-case audit</a> · <a href="TEST-REPORT.html">Build/test evidence</a></p><div class="scroll"><table><thead><tr>{head}</tr></thead><tbody>{body}</tbody></table></div></body></html>'''


intro = f'<p>Source {escape(audit["source_revision"])} · {escape(audit["generated_at"])}.</p><p><strong>Each row states what was exercised and what still needs an account, provider, endpoint or device.</strong> Public feeds and local organization require no command-center login. Optional private integrations cannot retrieve private content without provider authorization.</p><p>{escape(audit["method"])}</p>'
(root / 'docs/USE-CASE-AUDIT.html').write_text(page('Functional use-case audit', intro,
    ['ID', 'Use case', 'Observed status', 'Dependencies', 'Evidence and limits'],
    [[r['id'], r['use_case'], r['status'], r['dependencies'], r['evidence_and_limits']] for r in audit['cases']]))
intro = f'<p>Version {escape(results["version"])} · {escape(results["generated_at"])}.</p><p><strong>Public source tests, regression tests, installer checks and native UI checks are separate evidence.</strong> Full PRD/provider/device acceptance remains open; a passing build does not establish it.</p>'
(root / 'docs/TEST-REPORT.html').write_text(page('Local build and test report', intro,
    ['Status', 'Area', 'Evidence and limits'],
    [[r['status'], r['area'], r['evidence']] for r in results['results']]))
print(f'Rendered {len(audit["cases"])} use cases and {len(results["results"])} evidence rows.')
