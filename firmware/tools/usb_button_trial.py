#!/usr/bin/env python3
"""One finite detached SIM job plus existing exact-socket observer; plan by default.
No flashing, service restart, SSH, selection changes, automatic rearm or retry.
"""
import argparse,hashlib,importlib.util,json,os,select,socket,struct,subprocess,sys,time
from pathlib import Path
import ack_probe_v2 as ap

def fields(line):return dict(x.split('=',1) for x in line.split()[2:] if '=' in x)
def h(value):return format(value,'x')
def require(ok,message):
    if not ok:raise ValueError(message)

class Serial(ap.Serial):
    on_error=None
    def __exit__(self,kind,value,traceback):
        if kind is not None and self.on_error is not None:
            try:self.on_error()
            except BaseException:pass # finite firmware leases still apply; original error propagates
        return super().__exit__(kind,value,traceback)
    """One parser for AP2/SIM/TX replies; preserves base exclusive/DTR lifecycle."""
    def __enter__(self):
        super().__enter__()
        try:
            # Recover only an incomplete preexisting line; no mutator is retried.
            require(os.write(self.fd,b'\n')==1,'cold newline not sent')
            deadline=time.monotonic()+.5
            while time.monotonic()<deadline:
                ready,_,_=select.select([self.fd],[],[],.025)
                if ready:
                    data=os.read(self.fd,4096)
                    if not data:raise EOFError('cold USB disconnected')
                    self.bytes+=len(data)
                    require(self.bytes<524288,'cold USB byte cap')
            self.pending=b'';return self
        except BaseException:self.__exit__(None,None,None);raise
    def request(self,command,end):
        payload=(command+'\n').encode('ascii');deadline=time.monotonic()+4;lines=[]
        require(len(payload)<=96,'command line bound')
        while time.monotonic()<deadline:
            ready,write,_=select.select([self.fd],[self.fd] if payload else [],[],.025)
            if write:
                try:payload=payload[os.write(self.fd,payload):]
                except BlockingIOError:pass
            if ready:
                try:data=os.read(self.fd,4096)
                except BlockingIOError:continue
                if not data:raise EOFError('USB disconnected; no retry')
                self.pending+=data;self.bytes+=len(data)
                require(self.bytes<=524288 and len(self.pending)<=8192,'USB byte/line bound')
            while b'\n' in self.pending:
                line,self.pending=self.pending.split(b'\n',1);line=line.decode('ascii','replace').strip()
                if any(line.startswith(x+' error=')for x in ['usb','ap2','sim','tx1','cy1']):raise ValueError(line)
                if line.startswith(('ap2 ','sim ','tx1 ','cy1 ')):lines.append(line)
                if line.startswith(end):return lines
        raise TimeoutError('USB command deadline; accepted state may be uncertain; no retry')

def one(usb,command,prefix):
    rows=usb.request(command,prefix);return fields(next(x for x in rows if x.startswith(prefix)))

def acknowledge(usb,boot,cid,job,verb,args,expected):
    row=one(usb,f'SIM {verb} {boot} {h(cid)}'+(' '+h(job) if job is not None else '')+(' '+args if args else ''),'sim ack ')
    require(row.get('b')==boot and int(row['cid'],16)==cid and row.get('result')==expected,'SIM acknowledgement mismatch')
    if job is not None:require(int(row['job'],16)==job,'SIM job mismatch')
    return row

def release_prior(host,usb,device,saved_dir,out):
    """After BOOTSEL, only H RAM can remain. Never discard an unknown frozen run."""
    hs=host({'op':'status'});ps=one(usb,'AP2 STATUS','ap2 status ')
    require(ps['state']=='0','Pico must be IDLE before new trial')
    if hs['phase']=='idle':return hs
    require(saved_dir is not None and hs['phase']=='frozen','host retained; explicit --release-saved required')
    saved=json.loads((saved_dir/'saved.json').read_text())
    require(saved['device']==device and saved['pico_boot']!=ps['boot'],'prior target/boot mismatch; no release')
    for name in ['pico','host']:
        require(hashlib.sha256((saved_dir/(name+'.json')).read_bytes()).hexdigest()==saved[name+'_sha256'],'prior saved SHA mismatch')
    require(all(hs[k]==saved[v]for k,v in [('boot','host_boot'),('run','run'),('digest','host_digest')]),'prior frozen host identity/digest mismatch')
    ap.read_host(host,saved['host_boot'],saved['run'])
    ap.persist(out/'prior-preserved.json',{'saved_dir':str(saved_dir),'saved':saved,'new_pico_boot':ps['boot'],'release_scope':'host RAM only; old files retained; old Pico reset by user BOOTSEL'})
    got=host({'op':'release','boot':hs['boot'],'run':hs['run'],'digest':hs['digest']})
    require(got['phase']=='idle','prior host release unconfirmed');return got

def refresh_target(args,host,observer):
    a=host({'op':'status'});require(a['target']==args.device and a['phase']=='armed' and a['connection'] and a['session'],'host not ready')
    process=Path('/proc')/str(args.host_pid)
    require(process.stat().st_uid==os.getuid(),'server process owner mismatch')
    require(int(a['boot'].rsplit('-',1)[1],16)==args.host_pid,'server boot PID mismatch')
    neighbors=json.loads(subprocess.check_output(['ip','-j','neigh','show','dev',args.interface],text=True,timeout=2))
    matches=[x for x in neighbors if x.get('lladdr','').lower()==args.known_mac.lower()]
    require(len(matches)==1 and 'REACHABLE' in matches[0]['state'],'known MAC must be uniquely REACHABLE; no ping/config/retry')
    peer=matches[0]['dst'];route=json.loads(subprocess.check_output(['ip','-j','route','get',peer],text=True,timeout=2))
    require(len(route)==1 and route[0]['dev']==args.interface and route[0].get('prefsrc')==args.local_ip,'route mismatch')
    owned={}
    for p in(process/'fd').iterdir():
        try:
            link=os.readlink(p)
            if link.startswith('socket:['):owned[link[8:-1]]=int(p.name)
        except OSError:pass
    unix=[x.split()for x in(process/'net/unix').read_text().splitlines()[1:]]
    require(any(len(x)>=8 and x[7]==str(args.socket) and x[6]in owned for x in unix),'control socket not owned by selected server PID')
    unpack=lambda v:(socket.inet_ntoa(struct.pack('<I',int(v.split(':')[0],16))),int(v.split(':')[1],16))
    rows=[]
    for line in(process/'net/tcp').read_text().splitlines()[1:]:
        p=line.split()
        if p[3]!='01' or p[9]not in owned:continue
        local,remote=unpack(p[1]),unpack(p[2])
        if local==(args.local_ip,args.port) and remote[0]==peer:rows.append({'interface':args.interface,'local_ip':local[0],'local_port':local[1],'peer_ip':remote[0],'peer_port':remote[1],'inode':int(p[9]),'pid':args.host_pid,'fd':owned[p[9]],'process_start_ticks':int((process/'stat').read_text().rsplit(')',1)[1].split()[19])})
    require(len(rows)==1,'ambiguous server-owned peer socket');target=rows[0];diag=observer.Diag()
    try:
        observed=diag.query(target);require(observed['inode']==target['inode'] and observed['state']==1,'owned socket state changed');target['cookie']=observed['cookie']
        again=host({'op':'status'});require(all(a[k]==again[k]for k in ['boot','run','phase','connection','session','target']),'host binding changed')
        require(diag.query(target,target['cookie'])['inode']==target['inode'],'socket cookie changed')
    finally:diag.close()
    target.update(metadata_monotonic_ns=time.monotonic_ns(),clock=observer.stamp(),known_device_mac=args.known_mac,binding={k:a[k]for k in ['boot','run','phase','connection','session','target']})
    observer.validate_target(target);return target

def tx_digest(rows):
    value=2166136261
    for row in rows:
        for key,n in [('t',8),('s',4),('e',4),('a',4),('dt',4),('ip',2),('len',2),('rc',1),('nr',1),('f',1),('ref',1)]:
            for b in (row[key]&((1<<(8*n))-1)).to_bytes(n,'little'):value=((value^b)*16777619)&0xffffffff
    return value

def collect_extra(usb,pico,job,inner_dir=None):
    pb=pico['status']['boot'];run=pico['status']['run'];binding=one(usb,f'SIM BIND {pb}','sim bind ');require(binding['b']==binding['rb']==pb and binding['r']==run and int(binding['j'],16)==job,'SIM capture binding mismatch');syn=one(usb,'AP2 SYNTH','ap2 synth ');sim=one(usb,f'SIM STATUS {pb}','sim status ')
    require(syn['boot']==pb and syn['run']==run and sim['b']==pb and int(sim['j'],16)==job,'synthetic identity changed')
    require(sim['phase']in ['4','5'],'SIM job not terminal')
    steps=[one(usb,f'SIM GET {pb} {h(job)} {h(i)}','sim step ')for i in range(int(sim['n']))]
    tx=one(usb,'TX1 STATUS','tx1 status ');require(tx['b']==pb and tx['r']==run and tx['active']=='0','TX capture not frozen/matching')
    tx_status_generation=tx['gen'];count=int(tx['n'],16);require(0<=count<=32,'TX count bound');rows=[]
    for i in range(count):
        r=one(usb,f'TX1 READ {pb} {run} {h(i)}','tx1 row ');require(int(r['i'],16)==i,'TX row order')
        rows.append({k:int(v,10 if k=='rc' else 16)for k,v in r.items()})
    require(tx_digest(rows)==int(tx['h'],16),'TX frozen digest mismatch')
    final=one(usb,'TX1 STATUS','tx1 status ');require(final==tx,'TX changed while frozen')
    events=[r for r in pico['records']if r['event']==24]
    synthetic_ok=int(syn['n'],16)==len(events)==sum(int(s['d'])for s in steps) and len({r['c']for r in events})==len(events) and all((r['a']|(r['b']<<32))==job and r['c']<len(steps) and (r['d']&255)==int(steps[r['c']]['logical']) for r in events)
    if events:synthetic_ok=synthetic_ok and int(syn['job'],16)==job and int(syn['step'],16)==events[-1]['c']
    quality={'synthetic_origin_validated':synthetic_ok,'operation_dispatched_and_normally_released':sim['d']==sim['a']==sim['r'] and sim['a']!='0' and sim['c']=='0','accepted_mask':sim['a'],'dispatched_mask':sim['d'],'released_mask':sim['r'],'cancelled_mask':sim['c'],'dispatch_is_render_success':False,'physical_counter_unchanged':pico['window']['begin']==pico['window']['end']==pico['quality']['buttons'],'AP2_no_overwrite':pico['usable_without_overwrite'],'USB_no_observation_overlap':not pico['observation_usb_overlap_detected'],'TX_no_drop':int(tx['over'],16)==0,'TX_no_malformed':int(tx['bad'],16)==0,'TX_no_generation_change':tx['changed']=='0','TX_wrapper_attached':tx['ok']=='1','linkoutput_success_proves_RF':False}
    inner={}
    if inner_dir is not None:
        import cyw43_inner_collect
        inner=cyw43_inner_collect.collect(usb,pb,run,inner_dir)
        quality['SDK_no_diagnostic_loss']=inner['no_diagnostic_loss']
        joined=[]
        for tx_row in rows:
            found=[parts[0]['id'] for parts in inner['rows'] if (parts[1]['s'],parts[1]['e'],parts[1]['a'],parts[1]['ip'],parts[1]['len'])==(tx_row['s'],tx_row['e'],tx_row['a'],tx_row['ip'],tx_row['len']) and tx_row['t']<=parts[0]['t']<=parts[0]['end']<=tx_row['t']+tx_row['dt']]
            joined.append({'TX_index':tx_row['i'],'SDK_tickets':found})
        inner['TX_join']=joined
        quality['SDK_all_TX_rows_joined']=bool(rows) and all(len(x['SDK_tickets'])==1 for x in joined) and int(inner['status']['gen'],16)==int(tx_status_generation,16)

    return {'SDK_inner':inner,'binding':binding,'synthetic':syn,'simulator':sim,'steps':steps,'tx_status':tx,'tx_rows':rows,'quality':quality}

def observe_and_commit(session,usb,boot,cid,job,out):
    """START before COMMIT. Caller closes DTR immediately on return/error."""
    prepared=session.until('prepared',5);ap.persist(out/'observer-prepared.json',prepared)
    session.start();started=session.until('started',5);ap.persist(out/'observer-started.json',started)
    # If this ACK is lost, the finite job may be accepted. Do not retry/rearm.
    ack=acknowledge(usb,boot,cid,job,'COMMIT','','accepted');return ack

def finish_observer(session,out,seconds=65):
    deadline=time.monotonic()+seconds
    with(out/'headers.jsonl').open('xb')as packets,(out/'recvq.jsonl').open('xb')as queues:
        seen=0
        while True:
            session.pump(.025)
            for event in session.events[seen:]:
                dest=packets if event.get('event')=='packet'else queues if event.get('event')=='queue'else None
                if dest:dest.write(json.dumps(event,separators=(',',':')).encode()+b'\n')
            seen=len(session.events)
            if session.process.poll()is not None and session.eof:break
            if time.monotonic()>=deadline:raise TimeoutError('controller observation deadline')
        for f in [packets,queues]:f.flush();os.fsync(f.fileno())
    result=session.finish(3);worker=next((x for x in result['events']if x.get('event')=='worker_finished'),None);reaped=next((x for x in result['events']if x.get('event')=='guard_reaped'),None)
    scope_valid=all(x.get('scope_valid',True)for x in result['events'])
    result['metadata_events']=[x for x in result.pop('events')if x.get('event')not in ['packet','queue']]
    result['quality_complete']=bool(worker and reaped and worker.get('sockets_closed') and reaped.get('child_exit')==0 and scope_valid and worker['reason']=='time_limit' and worker['packet_stats'].get('kernel_drops')==0 and worker['max_queue_sample_gap_ns']<=200000000 and result['exit']==0)
    result['output_SHA256']={p.name:hashlib.sha256(p.read_bytes()).hexdigest()for p in [out/'headers.jsonl',out/'recvq.jsonl']};ap.persist(out/'observer-result.json',result);return result

def cleanup(session,usb,boot,cid,out):
    result={'scope':'cancel remaining virtual state; stop/reap exact observer; preserve captures; no release/rearm'}
    if usb is not None and boot:
        try:result['sim_stop']=acknowledge(usb,boot,cid,None,'STOP','','ok')
        except BaseException as e:result['sim_stop_uncertain']=str(e)
    if session is not None:
        process=getattr(session,'process',None)
        result['observer_controller_pid']=getattr(process,'pid',None)
        try:
            session.stop();session.close_control();finished=session.finish(3)
            events=finished.pop('events',[])
            result['observer']=finished|{'metadata_events':[x for x in events if x.get('event')not in ['packet','queue']]}
            # Persist any partial capture separately, bounded and without replacing outputs.
            for kind,name,cap in [('packet','cleanup-headers.jsonl',8388608),('queue','cleanup-recvq.jsonl',1048576)]:
                count=0
                with(out/name).open('xb')as f:
                    for event in events:
                        if event.get('event')!=kind:continue
                        data=json.dumps(event,separators=(',',':')).encode()+b'\n'
                        if count+len(data)>cap:result['partial_output_cap']=True;break
                        f.write(data);count+=len(data)
                    f.flush();os.fsync(f.fileno())
                result[name+'_SHA256']=hashlib.sha256((out/name).read_bytes()).hexdigest()
        except BaseException as e:
            result['observer_cleanup_uncertain']=str(e)
            result['observer_last_metadata']=[x for x in getattr(session,'events',[])if x.get('event')not in ['packet','queue']]

    ap.persist(out/'cleanup.json',result);return result

def run(args):
    require(0<=args.key<4 and 80<=args.hold_ms<=5000 and 0<=args.delay_ms<=14000 and args.delay_ms+args.hold_ms+70<=15000,'bounded operation required')
    plan={'plan_only':not args.execute,'device':args.device,'serial':args.serial,'host_socket':str(args.socket),'host_pid':args.host_pid,'operations':1,'key':args.key,'hold_ms':args.hold_ms,'delay_ms':args.delay_ms,'observer_seconds':60,'Pico_seconds':20,'job_seconds_max':15,'gate_guard_ms':500,'required_CY1':bool(getattr(args,'require_inner',False)),'flash':False,'service_restart':False,'SSH':False,'automatic_retry_or_rearm':False}
    if not args.execute:print(json.dumps(plan));return
    os.umask(0o077);args.out.mkdir(mode=0o700,parents=True,exist_ok=False);ap.persist(args.out/'plan.json',plan)
    sys.path.insert(0,str(args.observer_base));import observer_client as client
    host=lambda q:ap.host_request(args.socket,q);session=None;usb=None;boot=None;committed=False;cid=time.time_ns();job=cid;identity=None
    try:
        with Serial(args.serial)as usb:
            ident=one(usb,'AP2 ID','ap2 identity ');require(ident['device']==args.device,'Pico device mismatch');boot=ident['boot']
            def emergency_stop():
                result={}
                try:result['ack']=acknowledge(usb,boot,cid+1,None,'STOP','','ok')
                except BaseException as e:result['uncertain']=str(e)
                if identity is not None:
                    try:result['P_freeze']=one(usb,f'AP2 FREEZE {boot} {identity["run"]}','ap2 status ')
                    except BaseException as e:result['P_freeze_uncertain']=str(e)
                ap.persist(args.out/'error-sim-stop.json',result)
            usb.on_error=emergency_stop
            caps=one(usb,'SIM CAPS','sim caps ');require(caps['b']==boot and int(caps['keys'])>args.key and caps['state']=='3','SIM target/capability/state mismatch')
            if getattr(args,'require_inner',False):
                import cyw43_inner_collect
                cyw43_inner_collect.capabilities(usb)
            hs=release_prior(host,usb,args.device,args.release_saved,args.out);require(hs['target']==args.device and hs['connection'],'host target/disconnected')
            run_id=h(max(time.time_ns(),int(ident['last'],16)+1,int(hs['last_run'],16)+1));require(int(run_id,16)<2**64,'run counter exhausted')
            identity={'device':args.device,'run':run_id,'pico_boot':boot,'host_boot':hs['boot'],'host_connection':hs['connection'],'host_session':hs['session'],'job':h(job)};ap.persist(args.out/'identity.json',identity)
            armed=host({'op':'arm','boot':hs['boot'],'run':run_id});require(armed['phase']=='armed' and armed['run']==run_id,'host arm mismatch')
            ap.require_status(one(usb,f'AP2 ARM {boot} {run_id}','ap2 status '),boot,run_id,4)
            live=one(usb,'AP2 LIVE','ap2 live ');ap.require_prepared(live,boot,hs['session'])
            if getattr(args,'require_inner',False):ap.persist(args.out/'cyw43-prepared.json',cyw43_inner_collect.prepared(usb,boot,run_id,live))
            acknowledge(usb,boot,cid,job,'BEGIN',f'1 {caps["s"]} 3','staging');cid+=1
            binding=one(usb,f'SIM BIND {boot}','sim bind ');require(binding['b']==binding['rb']==boot and binding['r']==run_id and binding['s']==caps['s'] and int(binding['j'],16)==job,'SIM preparation binding mismatch')
            acknowledge(usb,boot,cid,job,'STEP',f'0 {h(args.delay_ms)} {h(args.key)} {h(args.hold_ms)}','staged');cid+=1
            target=refresh_target(args,host,client.observer);ap.persist(args.out/'target.json',target)
            require(target['binding']['run']==run_id and target['binding']['connection']==hs['connection'] and target['binding']['session']==hs['session'],'prepared binding changed')
            # Last endpoint checks precede the observer START, then COMMIT and immediate close.
            ap.require_prepared(one(usb,'AP2 LIVE','ap2 live '),boot,hs['session'])
            session=client.Session(target,seconds=60,prepare_seconds=30)
            ack=observe_and_commit(session,usb,boot,cid,job,args.out);committed=True
        usb=None
        ap.persist(args.out/'committed.json',{'ack':ack,'USB_DTR_closed':True,'USB_cable_removed':False,'job':h(job)})
        observed=finish_observer(session,args.out);session.close_control();session=None
        # Fixed upper bound has elapsed; no manual operation cue or repeated loop.
        host({'op':'freeze','boot':identity['host_boot'],'run':identity['run']});ap.wait_for_window(time.monotonic())
        with Serial(args.serial)as usb:
            ap.require_status(one(usb,f'AP2 FREEZE {boot} {identity["run"]}','ap2 status '),boot,identity['run'],3)
            pico=ap.read_pico(usb,boot,identity['run']);ap.persist(args.out/'pico.json',pico);extra=collect_extra(usb,pico,job,args.out if getattr(args,'require_inner',False)else None)
        usb=None;server=ap.read_host(host,identity['host_boot'],identity['run'])
        psha=ap.persist(args.out/'pico.json',pico);hsha=ap.persist(args.out/'host.json',server);esha=ap.persist(args.out/'extra.json',extra)
        ap.persist(args.out/'saved.json',identity|({'cyw43_sha256':hashlib.sha256((args.out/'cyw43.json').read_bytes()).hexdigest()}if getattr(args,'require_inner',False)else{})|{'pico_sha256':psha,'host_sha256':hsha,'extra_sha256':esha,'pico_digest':int(pico['status']['digest'],16),'host_digest':server['status']['digest'],'released':False,'quality':extra['quality']|{'observer_complete':observed['quality_complete'],'host_no_loss':server['usable_without_overwrite']}})
        print('SAVED: one finite virtual operation, frozen AP2/SIM/TX and host/header evidence retained; see quality flags.')
    except BaseException as error:
        # with Serial has already dropped DTR. COMMIT may have executed even if ACK was lost.
        usb=None;result=cleanup(session,None,boot,cid+1,args.out)
        if identity is not None:
            try:result['H_freeze']=host({'op':'freeze','boot':identity['host_boot'],'run':identity['run']})
            except BaseException as e:result['H_freeze_uncertain']=str(e)
        ap.persist(args.out/'failed.json',{'error':str(error),'commit_ACK_received':committed,'COMMIT_outcome_may_be_uncertain':True,'identity':identity,'cleanup':result,'automatic_retry':False});raise

def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--execute',action='store_true')
    for name in ['serial','device','interface','known-mac','local-ip']:p.add_argument('--'+name,required=True)
    for name in ['socket','observer-base','out']:p.add_argument('--'+name,type=Path,required=True)
    p.add_argument('--host-pid',type=int,required=True);p.add_argument('--port',type=int,default=16153);p.add_argument('--release-saved',type=Path)
    p.add_argument('--require-inner',action='store_true');p.add_argument('--key',type=int,required=True);p.add_argument('--hold-ms',type=int,default=100);p.add_argument('--delay-ms',type=int,default=1000);run(p.parse_args())
if __name__=='__main__':main()
