import importlib.util
from pathlib import Path
import sys
spec=importlib.util.spec_from_file_location('ev1',Path(__file__).resolve().parents[1]/'tools/event_stream.py')
ev=importlib.util.module_from_spec(spec);spec.loader.exec_module(ev)
rows=[];stage=None
for line in Path(sys.argv[1]).read_bytes().splitlines(keepends=True):
    if line.startswith(b'STAGE '):stage=line.decode().strip().split()[1];continue
    row=ev.decode(line)
    if row:
        assert not row['dropped'];row['stage']=stage;rows.append(row)
def matches(component,event,phase,frame):
    return [r for r in rows if (r['component'],r['event'],r['phase'],r['frame'])==(component,event,phase,frame)]
for frame in (101,202):
    for event in (12,13):
        pending=matches(2,event,3,frame)
        assert len(pending)==1 and pending[0]['b']==2,(frame,event,pending)
        received=matches(2,event,1,frame)
        assert len(received)==1 and pending[0]['operation']==received[0]['operation']
rejected=matches(2,12,5,303)
assert len(rejected)==1 and rejected[0]['b']==0,rejected
assert not matches(5,1,2,303) and not matches(5,1,4,303)
assert len(matches(4,1,7,101))==1 and not matches(4,1,7,202)
latest_begin=matches(5,1,2,202);latest_complete=matches(5,1,4,202)
assert len(latest_begin)==len(latest_complete)==1
assert latest_begin[0]['stage']=='old_done' and latest_complete[0]['stage']=='new_done'
assert latest_complete[0]['a']==1
assert all(r['stage']=='old_done' for r in matches(5,1,4,101))
assert [r['sequence'] for r in rows]==sorted(r['sequence'] for r in rows)
print('PASS decoded real EV1: pending/rejected result and old/new frame identities stay separate')

# A repeated in-flight token has separate API operations, one physical update.
for event in (12,13):
    received=matches(2,event,1,900);pending=matches(2,event,3,900)
    assert len(received)==len(pending)==2,(event,received,pending)
    assert len({r['operation'] for r in received})==2
    assert sorted(r['operation'] for r in pending)==sorted(r['operation'] for r in received)
    assert all(r['b']==2 for r in pending)
begin=matches(5,1,2,900);done=matches(5,1,4,900)
assert len(begin)==len(done)==1,(begin,done)
assert begin[0]['stage']=='replay_original_ready'
assert done[0]['stage']=='replay_original_done' and done[0]['a']==1
assert not matches(4,1,7,900)
print('PASS decoded EV1: two replay API operations coalesce into one physical frame900 completion')
