#!/usr/bin/env python3
"""Validate frozen diagnostics, including schema, row counts and metric totals."""
import argparse,json,math
from pathlib import Path
EDGES=[1000,2000,4000,8000,16000,24000,32000,40000,50000,60000,70000,80000,
       90000,100000,120000,150000,180000,220000,260000,320000,400000,500000,
       650000,800000,1000000,1300000,1600000,2000000,3000000,5000000,10000000,None]
KINDS=['offer','replace','busy','order','context','reset','draw','frame','close','error','sample','invalid']
METRICS=['loop','loop_gap','sdk_wait','net_poll','rx_take','net_send','process','draw','frame_rx','flush','usb_write','rx_cb']

def quantile(bins,n,p):
 if not n:return None
 rank=max(1,math.ceil(n*p));seen=0
 for i,c in enumerate(bins):
  seen+=c
  if seen>=rank:return {'lower_inclusive_us':0 if i==0 else EDGES[i-1],'upper_exclusive_us':EDGES[i]}
 raise ValueError('histogram population mismatch')

def integers(values):
 result={k:int(v) for k,v in values.items()}
 if any(v<0 for v in result.values()):raise ValueError('negative unsigned field')
 return result

def require(values,keys,label):
 if not set(keys)<=values.keys():raise ValueError('missing '+label+' fields')

def parse(text):
 if text.lstrip().startswith('{'):
  text=''.join(json.loads(line).get('text','') for line in text.splitlines() if line.strip())
 reports={}
 for line in text.splitlines():
  at=line.find('diag ')
  if at<0:continue
  parts=line[at:].split()
  if len(parts)<2:raise ValueError('truncated diagnostic line')
  kind=parts[1];values=dict(x.split('=',1) for x in parts[2:] if '=' in x)
  if 'epoch' not in values:raise ValueError('missing epoch')
  epoch=int(values.pop('epoch'))
  r=reports.setdefault(epoch,{'hist':{},'counts':{},'timeline':{},'_rows':0,'_metrics':{}})
  if kind not in ['end','metric']:r['_rows']+=1
  if kind in ['begin','end','hooks','trigger']:
   if kind in r:raise ValueError('duplicate '+kind)
   r[kind]=integers(values)
  elif kind=='hist':
   require(values,['h','n','sum','max','ge500','ge1000'],'histogram')
   h=int(values.pop('h'))
   if h not in range(3):raise ValueError('bad histogram index')
   m=r['hist'].setdefault(h,{'bins':[None]*32})
   if 'n' in m:raise ValueError('duplicate histogram header')
   m.update(integers(values))
  elif kind=='bins':
   require(values,['h','b','v'],'bins');h=int(values['h']);b=int(values['b']);v=list(map(int,values['v'].split(',')))
   if h not in range(3) or b not in [0,8,16,24] or len(v)!=8 or min(v)<0:raise ValueError('bad histogram range')
   m=r['hist'].setdefault(h,{'bins':[None]*32})
   if any(x is not None for x in m['bins'][b:b+8]):raise ValueError('duplicate bins')
   m['bins'][b:b+8]=v
  elif kind=='count':
   require(values,['k','n'],'counter');k=int(values['k']);n=int(values['n'])
   if k not in range(len(KINDS)) or k in r['counts'] or n<0:raise ValueError('bad/duplicate counter')
   r['counts'][k]=n
  elif kind in ['time','buf','draw']:
   require(values,['i'],'sample');i=int(values.pop('i'))
   if not 0<=i<24:raise ValueError('bad sample index')
   m=r['timeline'].setdefault(i,{})
   if kind in m:raise ValueError('duplicate timeline part')
   m[kind]=integers(values)
  elif kind=='metric':
   require(values,['usb','metric','total_us'],'metric')
   key=values['usb']+':'+values['metric']
   if values['usb'] not in ['on','off'] or values['metric'] not in METRICS or key in r['_metrics']:raise ValueError('bad/duplicate metric')
   r['_metrics'][key]=integers({'total_us':values['total_us']})['total_us']
  else:raise ValueError('unknown diagnostic line '+kind)
 if not reports:raise ValueError('no diagnostic report')
 output=[]
 for epoch,r in sorted(reports.items()):
  for name,keys in [('begin',['start_us','end_us','ram','overflow','samples','overwritten']),
                    ('end',['rows','export_us']),('hooks',['calls','total_us','max_us']),('trigger',['us','k','value'])]:
   if name not in r:raise ValueError('missing '+name+' row')
   require(r[name],keys,name)
  schema=r['trigger'].get('schema',1)
  if schema not in [1,2]:raise ValueError('unsupported schema')
  if r['_rows']!=r['end']['rows']:raise ValueError('diagnostic row count mismatch')
  if len(r['counts'])!=len(KINDS) or set(r['hist'])!={0,1,2}:raise ValueError('missing counters/histograms')
  if not 0<=r['begin']['samples']<=24:raise ValueError('bad sample count')
  if set(r['timeline'])!=set(range(r['begin']['samples'])):raise ValueError('missing sample index')
  for t in r['timeline'].values():
   if set(t)!=({'time','buf','draw'} if schema==2 else {'time','buf'}):raise ValueError('incomplete sample')
   require(t['time'],['us','session','frame','seq'],'assembly identity')
   require(t['buf'],['rx','app','credit','wnd','nxt','used','flags'],'buffer')
   if schema==2:
    require(t['draw'],['session','frame','seq','tcp'],'display identity')
    flags=t['buf']['flags'];t['validity']={'assembly':bool(flags&4),'display':bool(flags&8),'tcp':bool(flags&16)}
    if not flags&4 and any(t['time'][k] for k in ['session','frame','seq']):raise ValueError('invalid assembly identity is nonzero')
    if not flags&4 and t['buf']['used']:raise ValueError('invalid assembly has byte count')
    if not flags&8 and any(t['draw'][k] for k in ['session','frame','seq']):raise ValueError('invalid display identity is nonzero')
  for i,h in r['hist'].items():
   if 'n' not in h or any(x is None for x in h['bins']):raise ValueError('missing histogram bins')
   if not r['begin']['overflow'] and sum(h['bins'])!=h['n']:raise ValueError('histogram count mismatch')
   h['name']=['video_draw_gap','completed_frame_assembly','video_spi_elapsed'][i]
   h['percentile_bounds']=None if r['begin']['overflow'] else {str(p):quantile(h['bins'],h['n'],p) for p in [.5,.95,.99]}
  keys={bank+':'+m for bank in ['off','on'] for m in METRICS};missing=sorted(keys-r['_metrics'].keys())
  if r['_metrics'] and missing:raise ValueError('partial metric totals: '+','.join(missing))
  r['metric_totals']={'available':not missing,'missing':missing,'values':r.pop('_metrics')}
  r['schema']=schema;r['identity_semantics']='separate_assembly_display_tcp' if schema==2 else 'legacy_ambiguous'
  r['validated_rows']=r.pop('_rows');r['epoch']=epoch;r['counter_names']={KINDS[k]:v for k,v in r['counts'].items()}
  output.append(r)
 return output
if __name__=='__main__':
 p=argparse.ArgumentParser();p.add_argument('input',type=Path);p.add_argument('--output',type=Path);a=p.parse_args()
 try:out=json.dumps(parse(a.input.read_text()),ensure_ascii=False,indent=2)+'\n'
 except (ValueError,KeyError,TypeError) as e:p.error(str(e))
 if a.output:a.output.write_text(out)
 else:print(out,end='')
