#!/usr/bin/env python3
"""Explicit ACK probe control. Plan only unless --execute; no fixed device defaults."""
import argparse,fcntl,hashlib,json,os,select,socket,struct,termios,time,tempfile
from pathlib import Path
RECORD=struct.Struct('<QQQIIIIIIHHB3x')
WINDOW_SECONDS=20.0
SETTLE_SECONDS=WINDOW_SECONDS+0.1

def pico_fields(line):
    return dict(v.split('=',1) for v in line.split()[2:] if '=' in v)

def fnv(rows):
    h=2166136261
    for r in rows:
        packed=RECORD.pack(*(r[k] for k in ['us','session','frame','connection','message','a','b','c','d','event','flags','kind']))
        for b in packed:h=((h^b)*16777619)&0xffffffff
    return h

def require_status(s,boot,run,state):
    if s.get('boot')!=boot or s.get('run')!=run or s.get('state')!=str(state):raise ValueError('Pico boot/run/state mismatch; no cue')

def require_prepared(live,boot,session):
    required={'boot','session','active','state','trigger','buttons','begin'}
    if not required<=live.keys():raise ValueError('Pico prepared confirmation missing; no cue')
    if live['boot']!=boot or live['active']!='1' or int(live['session'],16)!=session or live['state']!='4' or int(live['trigger'],16)!=0 or int(live['buttons'],16)!=int(live['begin'],16):
        raise ValueError('Pico preparation input/state changed; no cue')

def wait_for_window(stopped_at,clock=time.monotonic,sleeper=time.sleep):
    # Inputs must already have stopped. Even an unknown first-trigger time is
    # then <= stopped_at, so the fixed20s observation ends before USB opens.
    deadline=stopped_at+SETTLE_SECONDS
    while True:
        remaining=deadline-clock()
        if remaining<=0:return
        sleeper(min(remaining,0.25))

def read_pico(transport,boot,run):
    status=pico_fields(transport.request('AP2 STATUS','ap2 status ')[0]);require_status(status,boot,run,3)
    window=pico_fields(transport.request('AP2 WINDOW','ap2 window ')[0])
    if window['boot']!=boot or window['run']!=run:raise ValueError('Pico window identity mismatch')
    quality=pico_fields(transport.request('AP2 QUALITY','ap2 quality ')[0])
    if quality['boot']!=boot or quality['run']!=run:raise ValueError('Pico quality identity mismatch')
    n=int(status['n']);digest=int(status['digest'],16)
    if not 0<=n<=96:raise ValueError('Pico count out of bounds')
    rows=[]
    for start in range(0,n or 1,8):
        lines=transport.request(f'AP2 READ {boot} {run} {start:x} 8','ap2 end ')
        begin=next((pico_fields(s) for s in lines if s.startswith('ap2 page ')),None)
        if not begin or begin['boot']!=boot or begin['run']!=run or int(begin['from'],16)!=start or int(begin['total'],16)!=n or int(begin['digest'],16)!=digest:raise ValueError('Pico page identity mismatch')
        count=min(8,n-start);parts={}
        for s in lines:
            if s.startswith('ap2 i='):
                v=dict(x.split('=',1) for x in s.split()[1:]);i=int(v.pop('i'),16)
                part='id' if 't' in v else 'data'
                if (i,part) in parts:raise ValueError('duplicate Pico row part')
                parts[(i,part)]={k:int(x,16) for k,x in v.items()}
        end=pico_fields(lines[-1])
        if int(end['from'],16)!=start or int(end['count'],16)!=count or int(end['digest'],16)!=digest or len(parts)!=2*count:raise ValueError('incomplete Pico page')
        for i in range(start,start+count):
            a=parts[(i,'id')];b=parts[(i,'data')]
            rows.append(dict(zip(['us','event','kind','connection','message','session','frame'],[a[k] for k in ['t','e','k','g','m','s','f']]))|b)
    if fnv(rows)!=digest:raise ValueError('Pico frozen digest mismatch')
    live=pico_fields(transport.request('AP2 LIVE','ap2 live ')[0])
    if live['boot']!=boot:raise ValueError('Pico boot changed during read')
    late=int(live['buttons'],16)!=int(window['end'],16)
    preparation_input=int(quality['buttons'],16)!=int(window['begin'],16)
    overlap=int(quality['overlap'],16)!=0
    return {'status':status,'window':window,'quality':quality,'prepared_input_detected':preparation_input,'observation_usb_overlap_detected':overlap,'records':rows,'complete':True,'usable_without_overwrite':int(status['over'])==0,'post_freeze_buttons_detected':late,'usable_for_last_reported_button':not late and not preparation_input and not overlap and int(status['over'])==0 and int(window['trigger'],16)>0 and any(r['event']==1 for r in rows)}

def host_request(path,request):
    with socket.socket(socket.AF_UNIX,socket.SOCK_STREAM) as s:
        s.settimeout(2);s.connect(str(path));s.sendall(json.dumps(request,separators=(',',':')).encode()+b'\n');out=b''
        while not out.endswith(b'\n'):
            data=s.recv(4096)
            if not data:raise ValueError('incomplete host control reply')
            out+=data
            if len(out)>32768:raise ValueError('host control reply too large')
    value=json.loads(out)
    if 'error' in value:raise ValueError('host '+value['error'])
    return value

def read_host(request,boot,run):
    status=request({'op':'status'})
    if status['boot']!=boot or status['run']!=run or status['phase']!='frozen':raise ValueError('host boot/run/state mismatch')
    n=status['count'];rows=[]
    if not 0<=n<=512:raise ValueError('host count out of bounds')
    for start in range(0,n or 1,16):
        page=request({'op':'read','boot':boot,'run':run,'start':start,'limit':16});s=page['status']
        if s['boot']!=boot or s['run']!=run or s['digest']!=status['digest'] or s['count']!=n or page['start']!=start or page['count']!=min(16,n-start) or len(page['records'])!=page['count']:raise ValueError('host page identity/count mismatch')
        rows+=page['records']
    import zlib
    digest=0
    for r in rows:
        canonical={k:r[k] for k in ['us','event','connection','message','session','frame','kind','a','b']}
        digest=zlib.crc32(json.dumps(canonical,separators=(',',':')).encode(),digest)
    if digest!=status['digest']:raise ValueError('host frozen digest mismatch')
    stable={k:v for k,v in status.items() if k not in ['connection','session']}
    return {'status':stable,'records':rows,'complete':True,'usable_without_overwrite':status['overwritten']==0 and status['contended']==0,'input_trigger_present':status.get('trigger_us',0)>0}

class Serial:
    def __init__(self,path):self.path=path;self.fd=None;self.original=None;self.pending=b'';self.bytes=0;self.dtr_set=False
    def __enter__(self):
        try:
            self.fd=os.open(self.path,os.O_RDWR|os.O_NOCTTY|os.O_NONBLOCK);fcntl.ioctl(self.fd,termios.TIOCEXCL)
            self.original=termios.tcgetattr(self.fd)
            if self.original[5]==termios.B1200:raise ValueError('1200 baud refused')
            import copy
            raw=copy.deepcopy(self.original);raw[0]&=~(termios.ICRNL|termios.INLCR|termios.IGNCR|termios.IXON|termios.ISTRIP);raw[1]&=~termios.OPOST;raw[3]&=~(termios.ECHO|termios.ICANON|termios.ISIG|termios.IEXTEN);raw[6][termios.VMIN]=raw[6][termios.VTIME]=0
            termios.tcsetattr(self.fd,termios.TCSANOW,raw);fcntl.ioctl(self.fd,termios.TIOCMBIS,struct.pack('I',termios.TIOCM_DTR));self.dtr_set=True;return self
        except BaseException:self.__exit__(None,None,None);raise
    def __exit__(self,*_):
        if self.fd is not None:
            try:
                if self.original is not None and self.dtr_set:termios.tcsetattr(self.fd,termios.TCSANOW,self.original)
            finally:
                try:
                    if self.dtr_set:fcntl.ioctl(self.fd,termios.TIOCMBIC,struct.pack('I',termios.TIOCM_DTR))
                finally:os.close(self.fd);self.fd=None
    def request(self,command,end):
        payload=(command+'\n').encode();deadline=time.monotonic()+4;lines=[]
        while time.monotonic()<deadline:
            ready,write,_=select.select([self.fd],[self.fd] if payload else [],[],.05)
            if write:
                try:n=os.write(self.fd,payload);payload=payload[n:]
                except BlockingIOError:pass
            if ready:
                try:data=os.read(self.fd,4096)
                except BlockingIOError:continue
                if not data:raise EOFError("USB disconnected; capture retained, no retry")
                self.pending+=data;self.bytes+=len(data)
                if self.bytes>524288:raise ValueError('USB byte cap')
            while b'\n' in self.pending:
                line,self.pending=self.pending.split(b'\n',1);line=line.decode('ascii','replace').strip()
                if line.startswith('ap2 error='):raise ValueError(line)
                if line.startswith('ap2 '):lines.append(line)
                if line.startswith(end):return lines
        raise TimeoutError('control acknowledgement deadline; no automatic retry')

def _write_staging(fd,data):
    remaining=memoryview(data)
    while remaining:
        written=os.write(fd,remaining)
        if written<=0:raise OSError('staging write made no progress')
        remaining=remaining[written:]

def persist(path,value):
    data=json.dumps(value,ensure_ascii=False,indent=2).encode()+b'\n'
    if len(data)>1048576:raise ValueError('output file cap')
    directory=os.open(path.parent,os.O_RDONLY|os.O_DIRECTORY)
    staged=None;fd=None
    try:
        if path.exists():
            if path.read_bytes()!=data:raise ValueError('retained file differs; refusing overwrite')
            os.fsync(directory)
        else:
            fd,name=tempfile.mkstemp(prefix='.'+path.name+'.',suffix='.tmp',dir=path.parent);staged=Path(name)
            _write_staging(fd,data);os.fsync(fd);os.close(fd);fd=None
            try:os.link(staged,path)  # Atomic publish, never replace a retained file.
            except FileExistsError:
                if path.read_bytes()!=data:raise ValueError('retained file differs; refusing overwrite')
            os.fsync(directory)
    finally:
        if fd is not None:os.close(fd)
        if staged is not None:
            try:staged.unlink()
            except FileNotFoundError:pass
        os.close(directory)
    return hashlib.sha256(data).hexdigest()

def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('op',choices=['prepare','collect','release']);p.add_argument('--execute',action='store_true');p.add_argument('--serial',required=True);p.add_argument('--socket',required=True,type=Path);p.add_argument('--device',required=True);p.add_argument('--dir',required=True,type=Path);a=p.parse_args()
    if not a.execute:print(json.dumps({'plan_only':True,'operation':a.op,'serial':a.serial,'socket':str(a.socket),'device':a.device,'window_seconds':20,'firmware_write':False,'service_restart':False}));return
    os.umask(0o077);root=a.dir;host=lambda q:host_request(a.socket,q)
    if a.op=='prepare':
        root.mkdir(mode=0o700,exist_ok=True);hs=host({'op':'status'})
        prior=json.loads((root/'identity.json').read_text()) if (root/'identity.json').exists() else None
        if (root/'released.json').exists():raise ValueError('use a new directory for a new run')
        if hs['target']!=a.device or hs['phase'] not in ['idle','armed'] or not hs['connection']:raise ValueError('host not idle/armed and bound to target; no cue')
        with Serial(a.serial) as usb:
            identity=pico_fields(usb.request('AP2 ID','ap2 identity ')[0]);ps=pico_fields(usb.request('AP2 STATUS','ap2 status ')[0])
            if identity['device']!=a.device or identity['boot']!=ps['boot'] or ps['state'] not in ['0','4']:raise ValueError('Pico target/boot/state mismatch; no cue')
            if prior:
                manifest=prior;run=prior['run']
                if prior['device']!=a.device or prior['pico_boot']!=ps['boot'] or prior['host_boot']!=hs['boot'] or prior['host_connection']!=hs['connection']:raise ValueError('preparation identity changed; no cue')
                if (ps['state']=='4' and ps['run']!=run) or (hs['phase']=='armed' and hs['run']!=run):raise ValueError('another run retained; no cue')
            else:
                if ps['state']!='0' or hs['phase']!='idle':raise ValueError('another run retained; no cue')
                run=format(max(time.time_ns(),int(identity['last'],16)+1,int(hs['last_run'],16)+1),'x')
                if int(run,16)>=2**64:raise ValueError('run counter exhausted')
                manifest={'run':run,'pico_boot':ps['boot'],'host_boot':hs['boot'],'device':a.device,'host_connection':hs['connection'],'phase':'preparing'};persist(root/'identity.json',manifest)
            armed=host({'op':'arm','boot':hs['boot'],'run':run})
            if armed['phase']!='armed' or armed['run']!=run:raise ValueError('host ARM unconfirmed; no cue')
            got=pico_fields(usb.request(f"AP2 ARM {ps['boot']} {run}",'ap2 status ')[0]);require_status(got,ps['boot'],run,4)
            got=pico_fields(usb.request('AP2 STATUS','ap2 status ')[0]);require_status(got,ps['boot'],run,4)
            final=host({'op':'status'})
            if final['phase']!='armed' or final['connection']!=hs['connection'] or final['boot']!=hs['boot'] or final['run']!=run:raise ValueError('host readiness changed; no cue')
            # Last endpoint check. USB still gates Pico triggers throughout it.
            live=pico_fields(usb.request('AP2 LIVE','ap2 live ')[0])
            require_prepared(live,ps['boot'],hs['session'])
        # Closing USB opens the unstarted Pico gate; no more endpoint work here.
        persist(root/'ready.json',manifest|{'phase':'ready','USB_closed':True,'pico_gate':'arms_on_USB_disconnect','confirmed_buttons':live['buttons'],'window_starts':'first physical button after USB gate closes; host starts on first decoded input; clocks separate'})
        print('READY: USB closed. First physical button starts a fixed20s window. Stop on delay or by20s; then collect.')
    else:
        identity=json.loads((root/'identity.json').read_text());run=identity['run'];pb=identity['pico_boot'];hb=identity['host_boot']
        if identity['device']!=a.device:raise ValueError('target identity mismatch')
        if a.op=='collect':
            stopped_at=time.monotonic()
            host({'op':'freeze','boot':hb,'run':run})
            wait_for_window(stopped_at)
            with Serial(a.serial) as usb:
                before=pico_fields(usb.request('AP2 STATUS','ap2 status ')[0])
                if before.get('state')=='2':raise ValueError('Pico still recording after settle; no clean USB observation claim')
                frozen=pico_fields(usb.request(f'AP2 FREEZE {pb} {run}','ap2 status ')[0]);require_status(frozen,pb,run,3);pico=read_pico(usb,pb,run)
            server=read_host(host,hb,run);psha=persist(root/'pico.json',pico);hsha=persist(root/'host.json',server)
            persist(root/'saved.json',identity|{'pico_sha256':psha,'host_sha256':hsha,'pico_digest':int(pico['status']['digest'],16),'host_digest':server['status']['digest'],'USB_closed':True,'released':False,'quality':{'pico_last_button_usable':pico['usable_for_last_reported_button'],'observation_USB_overlap_detected':pico['observation_usb_overlap_detected'],'prepared_input_detected':pico['prepared_input_detected'],'host_input_trigger_present':server['input_trigger_present'],'host_no_observation_loss':server['usable_without_overwrite']}});print('SAVED: both frozen captures validated and fsynced; retained until explicit release. Quality flags are in saved.json.')
        else:
            saved=json.loads((root/'saved.json').read_text())
            if any(saved[k]!=identity[k] for k in ['run','pico_boot','host_boot','device']):raise ValueError('saved capture identity mismatch; no release')
            for name in ['pico','host']:
                if hashlib.sha256((root/(name+'.json')).read_bytes()).hexdigest()!=saved[name+'_sha256']:raise ValueError('saved file changed; no release')
            with Serial(a.serial) as usb:
                status=pico_fields(usb.request(f"AP2 RELEASE {pb} {run} {saved['pico_digest']:x}",'ap2 status ')[0])
                if status['boot']!=pb or status['state']!='0':raise ValueError('Pico release unconfirmed')
            reply=host({'op':'release','boot':hb,'run':run,'digest':saved['host_digest']})
            if reply['phase']!='idle':raise ValueError('host release unconfirmed')
            persist(root/'released.json',{'run':run,'USB_closed':True,'released':True});print('RELEASED: a new prepare can arm without restart/reflash.')
if __name__=='__main__':main()
