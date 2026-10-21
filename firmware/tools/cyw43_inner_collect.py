"""Stable, bounded CY1 collector. All numeric fields hex except rxrc/brc."""
import struct
from pathlib import Path
import ack_probe_v2 as ap
SCHEMA={'v':'1','rows':'f','crc':'100','off':'e','depth':'4','row':'90','src':'cyw43-inner-v1:7f4c9375'}
def require(ok,why):
 if not ok:raise ValueError(why)
def one(usb,command,prefix):
 lines=usb.request(command,prefix);return dict(x.split('=',1)for x in next(v for v in lines if v.startswith(prefix)).split()[2:]if'='in x)
def capabilities(usb):
 value=one(usb,'CY1 CAPS','cy1 caps ');require(value==SCHEMA,'CY1 required schema/source/capacity mismatch');return value
def prepared(usb,boot,run,live):
 value=one(usb,'CY1 STATUS','cy1 status ')
 require(value['b']==boot and value['r']==run and value['gen']==live['gen'] and value['active']=='0'and all(int(value[k],16)==0 for k in ['n','over','inc','changed']),'CY1 prepared binding/empty window mismatch')
 return value

def digest(rows):
 h=2166136261
 def word(v,n):
  nonlocal h
  for b in(int(v)&((1<<(8*n))-1)).to_bytes(n,'little'):h=((h^b)*16777619)&0xffffffff
 for parts in rows:
  a,b,c,d,e,f=parts
  for k in ['t','bus','end']:word(a[k],8)
  for key in [b,c]:
   for k,n in [('s',4),('e',4),('a',4),('ip',2),('len',2),('et',2),('f',1),('v',1)]:word(key[k],n)
  for k in ['c0','cr','cb','b0','bb']:word(d[k],4)
  word(e['wait'],4);word(e['polls'],4)
  for k in ['calls','req','errs']:word(f[k],4)
  word(a['id'],4);word(f['brc'],4);word(a['flags'],2);word(d['cov'],2);word(d['domain'],2)
  for k in ['d','c','a','bad']:word(e[k],2)
  word(e['rxrc'],2);word(e['n'],2)
  for k in ['f0','f1','c0','c1','s0','s1']:word(f[k],1)
  word(a['dep'],1);word(a['nest'],1)
 return h

def classify(parts,loss=False):
 a,b,c,d,e,f=parts;flags=a['flags']
 if loss or not flags&16 or flags&(256|1024|8192):return'incomplete'
 if flags&512:return'credit_timeout'
 if f['brc'] or not flags&8:return'bus_error_or_no_bus_write'
 if not flags&2:return'no_credit_stall_region_changed'if flags&32 else'no_credit_stall'
 if flags&32:return'stall_RX_observed_region_changed'if f['calls']else'stall_region_changed_without_observed_RX'
 return'stall_without_observed_region_change'

def collect(usb,boot,run,out=None):
 caps=capabilities(usb);status=one(usb,'CY1 STATUS','cy1 status ')
 require(status['b']==boot and status['r']==run and status['active']=='0','CY1 capture binding/state mismatch')
 quality=one(usb,'CY1 QUALITY','cy1 quality ');n=int(status['n'],16);require(0<=n<=15,'CY1 count bound');rows=[]
 if out is not None:
  root=Path(out)/'cyw43-parts';root.mkdir(exist_ok=False);ap.persist(root/'identity.json',{'caps':caps,'status':status,'quality':quality})
 for i in range(n):
  parts=[]
  for p in range(6):
   row=one(usb,f'CY1 READ {boot} {run} {i:x} {p:x}','cy1 row ')
   value={k:int(v,10 if k in ['rxrc','brc']else 16)for k,v in row.items()}
   require(value['i']==i and value['p']==p,'CY1 part identity/order mismatch')
   if out is not None:ap.persist(root/f'{i:x}-{p:x}.json',value)
   parts.append(value)
  require(parts[0]['flags']&16,'CY1 incomplete record');require(parts[3]['off']==14 and 0<parts[3]['cov']<=256 and parts[3]['cov']<=parts[3]['domain'],'CY1 CRC domain bound');rows.append(parts)
 require(digest(rows)==int(status['h'],16),'CY1 digest mismatch')
 require(one(usb,'CY1 STATUS','cy1 status ')==status and one(usb,'CY1 QUALITY','cy1 quality ')==quality,'CY1 frozen capture changed while collecting')
 loss=any(int(status[k],16)for k in ['over','inc','changed'])or any(int(quality[k],16)for k in ['sat','depth','reject'])
 result={'caps':caps,'status':status,'quality':quality,'rows':rows,'classifications':[classify(r,loss)for r in rows],'no_diagnostic_loss':not loss,'domain':'BDC+Ethernet startingoffset14; boundedprefix256; valid SDPCM/padding and tail alignment excluded','equal_CRC_proves_full_identity':False,'RF_departure_proven':False}
 if out is not None:ap.persist(Path(out)/'cyw43.json',result)
 return result
