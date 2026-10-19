import json,re,sys
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'tools'))
from parse_stream_diag import parse
text=Path(sys.argv[1]).read_text();data=parse(text);assert len(data)==1
r=data[0];h=r['hist'][0]
assert h['n']==10003 and h['ge500']==4 and h['ge1000']==3
assert h['percentile_bounds']['0.5']=={'lower_inclusive_us':80000,'upper_exclusive_us':90000}
chunks='\n'.join(json.dumps({'t':i,'text':text[i:i+7]}) for i in range(0,len(text),7))
assert parse(chunks)==data
bad=[text.replace('diag end','removed end'),text.replace('diag bins epoch=1 h=0 b=0','removed bins epoch=1 h=0 b=0'),text+next(l+'\n' for l in text.splitlines() if l.startswith('diag bins'))]
for item in bad:
 try:parse(item)
 except ValueError:pass
 else:raise AssertionError('accepted incomplete or duplicate report')
overflow=parse(text.replace('overflow=0','overflow=1'))
assert all(h['percentile_bounds'] is None for h in overflow[0]['hist'].values())
print('unified parser: all intervals, percentile bounds, USB fragments, missing/duplicate rows and overflow PASS')

# R6: the independent-review missing-trigger and empty-input reproductions.
for item in ['',text.replace('diag trigger','removed trigger'),re.sub(r'rows=(\d+)',lambda m:'rows='+str(int(m.group(1))+1),text)]:
 try:parse(item)
 except ValueError:pass
 else:raise AssertionError('accepted missing trigger/empty input/wrong row count')
assert not data[0]['metric_totals']['available'] and len(data[0]['metric_totals']['missing'])==24
metrics=['loop','loop_gap','sdk_wait','net_poll','rx_take','net_send','process','draw','frame_rx','flush','usb_write','rx_cb']
metric_rows=[f'diag metric epoch=1 usb={b} metric={m} total_us={i+1}\n' for b in ['off','on'] for i,m in enumerate(metrics)]
full=parse(''.join(metric_rows)+text);assert full[0]['metric_totals']['available']
assert full[0]['metric_totals']['values']['off:loop']==1 and full[0]['metric_totals']['values']['on:rx_cb']==12
for rows in [metric_rows[:-1],metric_rows+[metric_rows[0]]]:
 try:parse(''.join(rows)+text)
 except ValueError:pass
 else:raise AssertionError('accepted partial/duplicate metric totals')
assert data[0]['validated_rows']==data[0]['end']['rows'] and data[0]['schema']==2
print('R6 PASS: empty/trigger/row-count validation, all 24 totals parsed, absent/partial/duplicate totals handled')
