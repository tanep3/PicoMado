import sys
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'tools'))
import event_stream as ev
import event_causality as causal

window=ev.Window(1);facts=[];start=end=None
for host_sequence,line in enumerate(Path(sys.argv[1]).read_bytes().splitlines(keepends=True),1):
    fact=ev.decode(line)
    if fact:
        facts.append(fact);window.accept(fact,host_sequence)
    else:
        status=ev.parse_status(line);assert status
        if status['on']:
            start=status;window.begin(status)
            class Storage:
                def status(self):return dict(durable_prefix=100,queue_lost=0,write_lost=0,error=None)
            owner=object.__new__(ev.Collector);owner.window=window;owner.link_epoch=1;owner.stop_requested=False;owner.journal=Storage()
            assert owner.status()['causal_ready'],owner.status()
            assert window.last==2 and start['sequence']==1
        else:end=status
assert [f['sequence'] for f in facts]==[1,2,3]
assert window.finish(end)['causal_complete']
assert causal.interval_source(facts,start,end,[])[1]
print('PASS real IRQ-boundary bytes reach causal_ready and causal_complete, including post-begin Network fact before ON reply')
