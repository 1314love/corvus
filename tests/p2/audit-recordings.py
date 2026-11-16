#!/usr/bin/env python3
"""Audit recorded MCP steps for leaks and schema hygiene (QA-only)."""
import sqlite3, json, re, collections, sys

db = sys.argv[1]
token_path = sys.argv[2]
tok = open(token_path, 'rb').read().strip().decode()
con = sqlite3.connect(db)
con.row_factory = sqlite3.Row
rows = [dict(r) for r in con.execute('select * from steps order by id')]
blob = json.dumps(rows, ensure_ascii=False)

out = {'steps': len(rows), 'tools': dict(collections.Counter(r['tool'] for r in rows))}
checks = {}
checks['no_private_provider_refs'] = 'private-node' not in blob
checks['no_png_base64'] = 'iVBORw0KGgo' not in blob
checks['no_token'] = tok not in blob
checks['no_banned_wording'] = not re.search(r'\b(provider|extension|cdp|plugin|worker|websocket|ws)\b', blob, re.I)
checks['no_ws_url'] = 'ws://' not in blob

shots = [r for r in rows if r['tool'] == 'page_screenshot']
shot_ok = True
shot_details = []
for r in shots:
    res = json.loads(r['result_json']) if r['result_json'] else {}
    detail = {
        'id': r['id'], 'ok': r['ok'], 'keys': sorted(res.keys()),
        'has_data': 'data' in res, 'image_keys': sorted((res.get('image') or {}).keys()),
        'inline': res.get('inline'), 'path': res.get('path'),
        'sha256': res.get('sha256'), 'bytes': res.get('bytes'),
    }
    shot_details.append(detail)
    if r['ok']:
        shot_ok = shot_ok and isinstance(res.get('path'), str) and re.search(r'[\\/]shot-[0-9a-f]{32}\.(png|jpg)$', res['path']) is not None
        shot_ok = shot_ok and isinstance(res.get('sha256'), str) and re.fullmatch(r'[0-9a-f]{64}', res['sha256']) is not None
        shot_ok = shot_ok and isinstance(res.get('bytes'), int) and res['bytes'] > 0
        shot_ok = shot_ok and not detail['has_data']
        image = res.get('image')
        if res.get('inline'):
            shot_ok = shot_ok and isinstance(image, dict) and sorted(image.keys()) == ['mime_type']
        else:
            shot_ok = shot_ok and image is None
checks['screenshots_finalized'] = shot_ok
out['screenshots'] = shot_details

snaps = [r for r in rows if r['tool'] == 'page_snapshot']
refs = []
refs_ok = True
for r in snaps:
    res = json.loads(r['result_json']) if r['result_json'] else {}
    row_refs = [n.get('ref') for n in res.get('nodes', [])]
    refs.append(row_refs)
    refs_ok = refs_ok and all(isinstance(x, str) and re.fullmatch(r'r-[0-9a-f]{8}', x) is not None for x in row_refs)
checks['snapshot_refs_opaque'] = refs_ok
out['snapshot_refs'] = refs

out['error_steps'] = [{'tool': r['tool'], 'error_code': r['error_code'], 'result': json.loads(r['result_json']) if r['result_json'] else None} for r in rows if r['error_code']]
checks['error_results_null'] = all(x['result'] is None for x in out['error_steps'])
checks['tools_are_known'] = set(out['tools']) <= {'browser_status','tab_list','tab_open','tab_close','page_navigate','page_read','page_snapshot','element_click','element_type','page_screenshot'}
out['checks'] = checks
out['passed'] = sum(1 for v in checks.values() if v)
out['total'] = len(checks)
print(json.dumps(out, ensure_ascii=False, indent=2))
