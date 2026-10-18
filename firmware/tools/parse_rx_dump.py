#!/usr/bin/env python3
"""Parse a saved RAM-statistics USB dump, rejecting missing/duplicate parts.

This reads a file only. It never opens USB, sends DTR, or contacts the service.
Each complete epoch must contain the 24 metric rows plus stats end, rx begin,
exactly 22 parts per declared record and 3 per event, and rx end.
"""
import argparse,json,re
from pathlib import Path
RECORD_PARTS={'id','time','gap','gap0','gap1','progress','peaks','peak_time','calls','early','consume','take','end_buf','end_tcp','wnd','alloc_b0','alloc_b1','alloc_e0','alloc_e1','alloc_used','failure','seg'}
EVENT_PARTS={'id','time','alloc'}
METRICS={'loop','loop_gap','sdk_wait','net_poll','rx_take','net_send','process','draw','frame_rx','flush','usb_write','rx_cb'}
FIELDS={
 'id':'frame session conn reason usb','time':'start_us end_us','gap':'from_us to_us',
 'gap0':'rx credit anchor tail','gap1':'rx credit','progress':'bytes at_us last_cb_us',
 'peaks':'rx app credit pool heap','peak_time':'rx_us credit_us',
 'calls':'callbacks bytes free_calls freed','early':'calls bytes','consume':'calls credit bytes last_us',
 'take':'calls messages','end_buf':'rx app credit','end_tcp':'valid wnd ann nxt ooseq',
 'wnd':'min ann_min ooseq_seen','alloc_b0':'flags bits pool_err heap_err','alloc_b1':'tcp_mem drop chk err',
 'alloc_e0':'flags bits pool_err heap_err','alloc_e1':'tcp_mem drop chk err',
 'alloc_used':'pool0 heap0 pool1 heap1','failure':'first_us mask error','seg':'err0 err1 used0 used1 peak'}
EVENT_FIELDS={'id':'frame conn reason error','time':'at_us rx app credit','alloc':'flags pool_err heap_err'}
def parse(text):
    epochs={}
    for raw in text.splitlines():
        # The collector may prepend host timestamps; the firmware payload is fixed.
        m=re.search(r'\b(stats|rx|rxevent) (.*)$',raw)
        if not m:continue
        kind,body=m.groups();f=dict(re.findall(r'(\w+)=(-?\d+|[a-z_][a-z_0-9]*)',body))
        if 'epoch' not in f:continue
        epoch=int(f.pop('epoch'));e=epochs.setdefault(epoch,{'stats':{},'records':{},'events':{},'errors':[]})
        def once(key,value):
            if key in e:e['errors'].append('duplicate '+key)
            e[key]=value
        if kind=='stats' and body.startswith('begin '):once('stats_begin',f)
        elif kind=='stats' and body.startswith('end '):once('stats_end',f)
        elif kind=='stats':
            key=(f.get('usb'),f.get('metric'))
            if key in e['stats']:e['errors'].append('duplicate metric '+str(key))
            e['stats'][key]=f
        elif kind=='rx' and body.startswith('begin '):once('rx_begin',f)
        elif kind=='rx' and body.startswith('end '):once('rx_end',f)
        else:
            slot=f.pop('slot',None);part=f.pop('part',None)
            if slot is None or part is None:e['errors'].append('missing slot/part');continue
            if not slot.isdigit():e['errors'].append('invalid slot');continue
            dest=e['events' if kind=='rxevent' else 'records'].setdefault(int(slot),{})
            if part in dest:e['errors'].append('duplicate '+kind+' slot='+slot+' part='+part)
            dest[part]={k:int(v) if re.fullmatch(r'-?\d+',v) else v for k,v in f.items()}
    results=[]
    for epoch,e in sorted(epochs.items()):
        for key in ['stats_begin','stats_end','rx_begin','rx_end']:
            if key not in e:e['errors'].append('missing '+key)
        expected={(bank,name) for bank in ['off','on'] for name in METRICS}
        if set(e['stats'])!=expected:e['errors'].append('missing/unknown metrics')
        for metric in e['stats'].values():
            if set(metric)!={'usb','metric','calls','max_us'} or any(not metric.get(k,'').isdigit() for k in ['calls','max_us']):e['errors'].append('truncated metric')
        header=e.get('rx_begin',{})
        if header.get('schema')!='2' or header.get('parts')!='22' or header.get('event_parts')!='3':e['errors'].append('unsupported schema/part count')
        for name,parts in [('records',RECORD_PARTS),('events',EVENT_PARTS)]:
            if len(e[name])!=int(header.get(name,-1)):e['errors'].append('declared '+name+' count mismatch')
            for slot,row in e[name].items():
                if set(row)!=parts:e['errors'].append(name+' slot='+str(slot)+' missing/unknown parts')
                if not 0<=slot<4:e['errors'].append(name+' slot outside fixed bounds')
                schema=EVENT_FIELDS if name=='events' else FIELDS
                for part,values in row.items():
                    if part in schema and (set(values)!=set(schema[part].split()) or any(not isinstance(v,int) for v in values.values())):e['errors'].append(name+' slot='+str(slot)+' truncated/invalid '+part)
        results.append({'epoch':epoch,'complete':not e['errors'],'errors':e['errors'],'stats_begin':e.get('stats_begin'),'stats_end':e.get('stats_end'),'rx_begin':header,'metrics':list(e['stats'].values()),'records':e['records'],'events':e['events']})
    return results
def main():
    cli=argparse.ArgumentParser(description=__doc__);cli.add_argument('input',type=Path);cli.add_argument('--output',type=Path);args=cli.parse_args()
    result=parse(args.input.read_text(errors='replace'));text=json.dumps(result,indent=2)+'\n'
    if args.output:args.output.write_text(text)
    else:print(text,end='')
    raise SystemExit(not result or not all(e['complete'] for e in result))
if __name__=='__main__':main()
