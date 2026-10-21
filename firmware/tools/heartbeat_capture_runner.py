#!/usr/bin/env python3
"""One finite ARM_HB capture through the existing exclusive owner. Plan by default.

Optional one-step detached SIM; no firmware writes, service operations, SSH, network changes or automatic retry.
The heartbeat implementation is imported unchanged; runner evidence is separate.
"""
import argparse
import errno
import fcntl
import hashlib
import importlib
import json
import os
from pathlib import Path
import select
import struct
import sys
import termios
import time
import event_stream as ev
import heartbeat_capture as hc
import observation_domain as domain
from observation_serialization import value as output_value, report_value


class DTRFailure(RuntimeError):
    pass


class Owner:
    """Same SerialPort/Commands as the persistent collector; exactly one open."""
    def __init__(self,path,logpath,*,port_factory=ev.SerialPort,ioctl=fcntl.ioctl,
                 quiet_seconds=.15,request_seconds=5):
        self.port=None;self.closed=False;self.observer=None;self.ioctl=ioctl
        self.io_allowed=True;self.dtr_state=True;self.reads=self.writes=self.bytes=0
        self.request_seconds=request_seconds;self.mutations=set();self.last_status=None
        self.sim_enabled=False
        self.log=Path(logpath).open('x',buffering=1);os.chmod(logpath,0o600)
        self.lines=ev.Lines();self.mux=ev.Commands(audit=self.audit)
        try:
            resolved=os.path.realpath(path)
            for process in Path('/proc').iterdir():
                if not process.name.isdigit():continue
                try:
                    for fd in (process/'fd').iterdir():
                        try:
                            if os.readlink(fd)==resolved:raise ValueError('another serial owner exists')
                        except OSError:pass
                except OSError:pass
            self.port=port_factory(path);self.audit(kind='owner_opened',fd=self.fd)
            start=last=time.monotonic()
            while time.monotonic()-last<quiet_seconds or self.lines.partial():
                if time.monotonic()-start>10:raise TimeoutError('startup prelude deadline')
                if self.pump(.02):last=time.monotonic()
        except BaseException:
            self.close();raise

    @property
    def fd(self):return self.port.fd if self.port is not None else None

    @property
    def trustworthy(self):
        return self.port is not None and self.io_allowed and not(self.mux.uncertain or self.mux.dirty)

    def audit(self,**row):
        if self.log.closed:return
        if self.log.tell()>2097152:raise ValueError('owner audit byte bound')
        row.update(host_monotonic_ns=time.monotonic_ns(),realtime_ns=time.time_ns())
        self.log.write(json.dumps(row,separators=(',',':'))+'\n')

    def _write(self,data):
        if not self.io_allowed:raise RuntimeError('USB write inside observation forbidden')
        self.writes+=1;return self.port.write(data)

    def pump(self,timeout=.02):
        if self.observer is not None:self.observer.pump(0)
        if not self.io_allowed:return 0
        self.mux.tick(self._write,time.monotonic())
        if not select.select([self.port],[],[],timeout)[0]:return 0
        self.reads+=1;data=self.port.read()
        if not data:self.mux.disconnect();raise EOFError('serial disconnected; no reopen')
        self.bytes+=len(data)
        if self.bytes>524288:raise ValueError('USB byte bound')
        for line in self.lines.feed(data):
            self.audit(kind='USB_line',line=line.decode('ascii',errors='replace').rstrip())
            active=self.mux.active
            if active and (line.startswith(b'usb error=')or line.startswith(active['prefix']+b'error=')):
                self.mux._fail(active,'device rejected command');self.mux.active=None
            else:self.mux.receive(line)
        return len(data)

    def request(self,command,end):
        if not self.trustworthy:raise RuntimeError('serial state uncertain; command not sent')
        words=command.split()
        if not words or words[0]not in ['AP2','CY1','TX1','EV1','SIM']or(words[0]=='EV1'and words[1:]!=['STATUS'])\
           or(words[0]=='SIM'and(not self.sim_enabled or words[1]not in ['CAPS','BEGIN','STEP','COMMIT','STATUS','BIND','GET','STOP'])):
            raise ValueError('runner only permits scoped probe/SIM controls and EV1 STATUS')
        if words[0]=='AP2'and words[1]in ['ARM_HB','FREEZE','RELEASE']:
            key=tuple(words[:4])
            if key in self.mutations:raise RuntimeError('mutator is never automatically retried')
            self.mutations.add(key)
        if words[0]=='SIM'and words[1]in ['BEGIN','STEP','COMMIT','STOP']:
            key=('SIM',words[2],words[3])
            if key in self.mutations:raise RuntimeError('SIM mutator is never automatically retried')
            self.mutations.add(key)
        self.audit(kind='request',command=command)
        future=self.mux.submit(command,reply_prefix=words[0].lower()+' ',end_prefix=end)
        deadline=time.monotonic()+self.request_seconds
        try:
            while not future.done()and time.monotonic()<deadline:self.pump(.01)
            if not future.done():self.mux.disconnect();raise TimeoutError('command outcome uncertain; no retry')
            value=future.result();self.audit(kind='response',command=command,result=value)
            return value['replies']
        except BaseException:
            if not future.done():self.mux.disconnect()
            raise

    def set_dtr(self,enabled):
        if self.port is None:raise DTRFailure('closed owner')
        fd=self.fd;before=os.fstat(fd);error=None;actual=None
        try:
            prior=bool(struct.unpack('I',self.ioctl(fd,termios.TIOCMGET,struct.pack('I',0)))[0]&termios.TIOCM_DTR)
            self.audit(kind='DTR_before',enabled=prior,requested=enabled,fd=fd)
            try:self.ioctl(fd,termios.TIOCMBIS if enabled else termios.TIOCMBIC,struct.pack('I',termios.TIOCM_DTR))
            except OSError as problem:error=problem
            actual=bool(struct.unpack('I',self.ioctl(fd,termios.TIOCMGET,struct.pack('I',0)))[0]&termios.TIOCM_DTR)
            after=os.fstat(fd)
            if self.fd!=fd or(before.st_dev,before.st_ino,before.st_rdev)!=(after.st_dev,after.st_ino,after.st_rdev):
                actual=None;raise DTRFailure('FD identity changed')
        except BaseException as problem:error=problem
        self.dtr_state=actual;self.io_allowed=actual is True
        self.audit(kind='DTR_result',requested=enabled,actual=actual,error=str(error)if error else None,fd=fd)
        if error or actual is not enabled:raise DTRFailure('DTR transition failed; observed='+str(actual))

    def close(self):
        if self.closed:return
        old_fd=self.fd
        try:
            if self.port is not None:
                try:self.mux.disconnect()
                finally:self.port.close()
        finally:
            self.port=None;self.closed=old_fd is None
            if old_fd is not None:
                try:os.fstat(old_fd)
                except OSError as problem:self.closed=problem.errno==errno.EBADF
            try:self.audit(kind='owner_closed');self.log.flush();os.fsync(self.log.fileno())
            finally:self.log.close()


class HostGuard:
    """A fresh status resolves uncertain RPCs; no blind repeat of a mutator."""
    def __init__(self,call):self.call=call;self.sent=set()
    def __call__(self,query):
        op=query['op']
        if op in ['freeze','release']:
            status=self.call({'op':'status'})
            hc.require(status['boot']==query['boot'],'host boot changed')
            if op=='freeze'and status['phase']=='frozen'and status['run']==query['run']:return status
            key=(op,query['boot'],query['run'])
            if key in self.sent:
                if op=='release'and status['phase']=='idle'and status.get('last_run')==query['run']:return status
                raise RuntimeError('host mutator outcome uncertain; not retried')
            hc.require(status['run']==query['run']and status['phase']!='idle','host run changed')
            self.sent.add(key)
        return self.call(query)


class Runner:
    def __init__(self,owner,host,resolve_target,observer_factory,out,*,clock=time.monotonic,wait=time.sleep,clock_ns=None,sim=None):
        self.owner=owner;self.host=HostGuard(host);self.resolve_target=resolve_target
        self.observer_factory=observer_factory;self.root=Path(out);self.clock=clock;self.wait=wait
        self.persist=lambda path,data:hc.ap.persist(path,output_value(data));self.errors=[];self.session=None;self.target=None;self.ready=None
        self.ns=clock_ns or(time.monotonic_ns if clock is time.monotonic else lambda:int(clock()*1e9))
        self.coverage=dict(clock='CLOCK_MONOTONIC',wait_budget_ns=16000000000)
        self.identity=None;self.headers=None;self.safe_deadline=None;self.high_attempted=False
        self.interval=None;self.pico=None;self.server=None;self.saved=False
        self.observer_finished=True
        hc.require(sim is None or (type(sim) is dict and set(sim)=={'key','delay_ms','hold_ms'}
            and all(type(v)is int for v in sim.values()) and sim['key']==1
            and 0<=sim['delay_ms']<=14000 and 80<=sim['hold_ms']<=300
            and sim['delay_ms']+sim['hold_ms']+70<15000), 'bounded one-step SIM plan')
        self.sim=sim;self.sim_job=None;self.sim_cid=None;self.sim_sent=False
        if sim is not None:self.owner.sim_enabled=True

    def sim_ack(self,verb,args,expected):
        self.sim_cid+=1
        got=hc.one(self.owner,f'SIM {verb} {self.ready["pico_boot"]} {self.sim_cid:x} {self.sim_job:x}'+
                   (' '+args if args else ''),'sim ack ')
        hc.require(got['b']==self.ready['pico_boot'] and int(got['cid'],16)==self.sim_cid
                   and int(got['job'],16)==self.sim_job and got['result']==expected,
                   'SIM '+verb+' acknowledgement')
        self.sim_sent=True
        return got

    def stage_sim(self):
        caps=hc.one(self.owner,'SIM CAPS','sim caps ')
        hc.require(caps['b']==self.ready['pico_boot'] and int(caps['s'],16)==self.ready['session']
                   and caps['state']=='3' and int(caps['keys'])==4 and int(caps['steps'])>=1
                   and int(caps['job_ms'],16)==15000 and int(caps['gate_ms'],16)==500,
                   'SIM capability/session/state')
        self.sim_job=self.ready['sim_job'];self.sim_cid=self.sim_job
        self.sim_ack('BEGIN',f'1 {self.ready["session"]:x} {int(caps["state"]):x}','staging')
        self.sim_ack('STEP',f'0 {self.sim["delay_ms"]:x} {self.sim["key"]:x} {self.sim["hold_ms"]:x}','staged')

    def stop_sim_if_active(self):
        if self.sim_job is None or not self.owner.trustworthy:return
        status=hc.one(self.owner,f'SIM STATUS {self.ready["pico_boot"]}','sim status ')
        if int(status['j'],16)==self.sim_job and status['phase'] in ['1','2','3']:
            self.sim_cid+=1
            got=hc.one(self.owner,f'SIM STOP {self.ready["pico_boot"]} {self.sim_cid:x}','sim ack ')
            hc.require(got['b']==self.ready['pico_boot']and int(got['cid'],16)==self.sim_cid
                       and got['result']=='ok','SIM STOP acknowledgement')

    def error(self,phase,problem):
        self.errors.append(dict(phase=phase,type=type(problem).__name__,message=str(problem)[:240]))

    def wait_window(self):
        while self.safe_deadline is not None and self.ns()<self.safe_deadline:
            if self.session is not None:self.session.pump(.02)
            self.wait(max(0,min(.02,(self.safe_deadline-self.ns())/1e9)))

    def stop_observer(self):
        events=[];result=dict(exit=None,stderr='',events=[])
        if self.session is not None:
            self.coverage['stop_request_before_ns']=self.ns()
            self.session.stop()
            try:result=self.session.finish(4);events=result.pop('events')
            except BaseException as problem:
                self.error('observer_stop',problem);events=list(getattr(self.session,'events',[]))
            self.owner.observer=None
            self.coverage['finished_after_ns']=self.ns()
            self.coverage['worker_end_bracket_monotonic_ns']=[self.coverage['stop_request_before_ns'],self.coverage['finished_after_ns']]
        for name,kind in [('headers.jsonl','packet'),('recvq.jsonl','queue')]:
            with(self.root/name).open('xb')as stream:
                for event in events:
                    if event.get('event')==kind:stream.write(json.dumps(event,separators=(',',':')).encode()+b'\n')
                stream.flush();os.fsync(stream.fileno())
        worker=next((e for e in events if e.get('event')=='worker_finished'),None)
        reaped=next((e for e in events if e.get('event')=='guard_reaped'),None)
        self.observer_finished=self.session is None or bool(worker and reaped and result['exit']is not None and worker.get('sockets_closed'))
        summary=result|dict(metadata_events=[e for e in events if e.get('event')not in ['packet','queue']],
            complete_interval=bool(worker and reaped and result['exit']==0 and worker['sockets_closed']
                and worker['packet_stats']['kernel_drops']==0 and worker.get('max_queue_sample_gap_ns',10**12)<=200000000
                and not reaped.get('forced',False)),payload_copied_or_persisted=False,
            SHA256={n:hashlib.sha256((self.root/n).read_bytes()).hexdigest()for n in ['headers.jsonl','recvq.jsonl']})
        if self.ready is not None and self.target is not None:
            summary=hc.header_evidence(summary,[e for e in events if e.get('event')=='packet'],self.target,self.ready)
        else:summary.update(scope_valid=False)
        summary['controller']=self.coverage|dict(errors=[dict(e)for e in self.errors])
        self.headers=summary;self.persist(self.root/'observer-result.json',summary)

    def freeze_scoped(self):
        if self.identity is None:return
        identity=self.identity
        try:
            hs=self.host({'op':'status'})
            if hs['boot']==identity['host_boot']and hs['run']==identity['run']and hs['phase']!='idle':
                self.host(dict(op='freeze',boot=identity['host_boot'],run=identity['run']))
        except BaseException as problem:self.error('host_freeze',problem)
        if self.owner.trustworthy:
            ps=hc.one(self.owner,'AP2 STATUS','ap2 status ')
            if ps['boot']==identity['pico_boot']and ps['run']==identity['run']and ps['state']!='3':
                hc.one(self.owner,f'AP2 FREEZE {identity["pico_boot"]} {identity["run"]}','ap2 status ')

    def gather(self):
        if self.identity is None:return False
        i=self.identity;self.freeze_scoped()
        hc.require(self.owner.trustworthy,'Pico stream/DTR uncertain; RAM retained')
        ps=hc.one(self.owner,'AP2 STATUS','ap2 status ')
        hc.ap.require_status(ps,i['pico_boot'],i['run'],3)
        self.pico=hc.ap.read_pico(self.owner,i['pico_boot'],i['run'])
        self.persist(self.root/'pico.json',self.pico)
        hs=self.host({'op':'status'})
        if hs['boot']==i['host_boot']and hs['run']==i['run']and hs['phase']=='frozen':
            self.server=hc.ap.read_host(self.host,i['host_boot'],i['run']);self.persist(self.root/'host.json',self.server)
        else:
            hc.require(hs['boot']==i['host_boot']and hs['phase']=='idle','host capture identity changed')
            self.persist(self.root/'host-not-armed.json',hs)
        trigger=hc.one(self.owner,'AP2 TRIGGER','ap2 trigger ')
        txs=hc.one(self.owner,'TX1 STATUS','tx1 status ');cys=hc.one(self.owner,'CY1 STATUS','cy1 status ')
        extra=dict(trigger=trigger,tx_status=txs,tx_rows=[],SDK_inner=None,
                   EV1=hc.one(self.owner,'EV1 STATUS','ev1 status '))
        if self.sim is not None:
            sim_status=hc.one(self.owner,f'SIM STATUS {i["pico_boot"]}','sim status ')
            sim=dict(status=sim_status,
                     bind=hc.one(self.owner,f'SIM BIND {i["pico_boot"]}','sim bind '),
                     synth=hc.one(self.owner,'AP2 SYNTH','ap2 synth '))
            if self.sim_job is not None and int(sim_status['j'],16)==self.sim_job and sim_status['n']=='1':
                sim['step']=hc.one(self.owner,f'SIM GET {i["pico_boot"]} {self.sim_job:x} 0','sim step ')
            extra['SIM']=sim
        own_tx=txs['b']==i['pico_boot']and txs['r']==i['run']
        own_cy=cys['b']==i['pico_boot']and cys['r']==i['run']
        if own_tx and own_cy:
            hc.require(txs['active']=='0'and 0<=int(txs['n'],16)<=32,'TX frozen bound')
            for index in range(int(txs['n'],16)):
                row=hc.one(self.owner,f'TX1 READ {i["pico_boot"]} {i["run"]} {index:x}','tx1 row ')
                value={k:int(v,10 if k=='rc'else 16)for k,v in row.items()}
                hc.require(value['i']==index,'TX index');extra['tx_rows'].append(value)
            hc.require(hc.txlib.tx_digest(extra['tx_rows'])==int(txs['h'],16),'TX digest')
            hc.require(hc.one(self.owner,'TX1 STATUS','tx1 status ')==txs,'TX changed while frozen')
            extra['SDK_inner']=hc.cy.collect(self.owner,i['pico_boot'],i['run'],self.root)
        else:
            hc.require(int(self.pico['window']['trigger'],16)==0 and not self.pico['records'],
                       'current TX/CY unavailable after trigger; retain RAM')
            extra['prior_TX_CY_excluded']=dict(TX=txs,CY=cys,reason='run never triggered; prior run is not current evidence')
        self.persist(self.root/'extra.json',extra)
        self.persist(self.root/'headers.json',self.headers)
        if self.ready and self.server and extra['SDK_inner']is not None:
            quality=hc.validate(self.pico,self.server,extra,self.ready,self.headers)
        else:quality=domain.pending_assessment('partial preparation/no corresponding trigger')
        self.persist(self.root/'quality.json',quality.record())
        if self.ready and self.server and extra['SDK_inner']is not None:
            self.quality=hc.confirm_saved(self.root,self.ready)
        else:self.quality=hc.confirm_partial(self.root,self.identity,self.server is not None)
        hc.require(self.quality['evidence_confirmed'],'common evidence validator rejected saved capture')
        return True

    def release_saved(self):
        if not self.saved or self.pico is None:return
        hc.require(self.observer_finished,'observer exit unconfirmed; RAM retained')
        confirmed=hc.validate_directory(self.root)
        hc.require(confirmed['evidence_confirmed'],'common evidence validator rejected release')
        release=confirmed['release']
        i=self.identity
        files=[p for p in self.root.rglob('*')if p.is_file()and p.name not in ['owner.private.jsonl','pre-release.json','result.json']]
        self.owner.log.flush();os.fsync(self.owner.log.fileno())
        audit=self.root/'owner.private.jsonl'
        if audit.exists():
            with(self.root/'pre-release-owner.private.jsonl').open('xb')as dest:
                dest.write(audit.read_bytes());dest.flush();os.fsync(dest.fileno())
            files.append(self.root/'pre-release-owner.private.jsonl')
        hashes={str(p.relative_to(self.root)):hashlib.sha256(p.read_bytes()).hexdigest()for p in files}
        self.persist(self.root/'pre-release.json',dict(identity=i,SHA256=hashes,errors=self.errors,
                    capture_quality=self.quality,observation_USB_io=self.observation_io))
        hc.require(all(hashlib.sha256((self.root/n).read_bytes()).hexdigest()==h for n,h in hashes.items()),'saved evidence changed')
        ps=hc.one(self.owner,f'AP2 RELEASE {release["pico"]["boot"]} {release["pico"]["run"]} {release["pico"]["digest"]:x}','ap2 status ')
        hc.require(ps['state']=='0','Pico release unconfirmed')
        if release['host']is not None:
            hs=self.host(dict(op='release',**release['host']))
            hc.require(hs['phase']=='idle','host release unconfirmed')
        self.persist(self.root/'released.json',dict(pico=ps,host=self.host({'op':'status'})))

    def run(self,device,run_id):
        self.quality=domain.incomplete('not captured');self.observation_io=dict(reads=0,writes=0)
        counts=None
        try:
            self.ready=hc.prepare(self.owner,self.host,device,run_id,self.root,
                observation_kind='natural_heartbeat_then_sim'if self.sim else'natural_heartbeat',sim=self.sim)
            self.identity=json.loads((self.root/'identity.json').read_text())
            self.target=self.resolve_target(self.ready);self.persist(self.root/'target.private.json',self.target)
            self.session=self.observer_factory(self.target);self.owner.observer=self.session;self.observer_finished=False
            self.persist(self.root/'observer-prepared.json',self.session.until('prepared',5))
            self.coverage['start_request_before_ns']=self.ns()
            self.session.start();started=self.session.until('started',5)
            self.coverage['start_confirm_after_ns']=self.ns()
            start_clock=started.get('clock',{})
            hc.require(started.get('event')=='started'and isinstance(start_clock,dict)
                and all(type(start_clock.get(k))is int for k in ['mono_before_ns','mono_after_ns'])
                and self.coverage['start_request_before_ns']<=start_clock['mono_before_ns']
                <=start_clock['mono_after_ns']<=self.coverage['start_confirm_after_ns'],'observer START unconfirmed')
            self.persist(self.root/'observer-started.json',started)
            if self.sim is not None:self.stage_sim()
            status=self.host({'op':'status'})
            hc.require(status['phase']=='armed'and status['boot']==self.ready['host_boot']
                and status['run']==run_id and status['last_message']==self.ready['baseline']
                and status['connection']==self.ready['host_connection'],'origin changed before DTR gate')
            if self.sim is not None:self.sim_ack('COMMIT','','accepted')
            self.coverage['DTR_low_before_ns']=self.ns()
            self.safe_deadline=self.coverage['DTR_low_before_ns']+36100000000
            self.coverage['required_end_ns']=self.safe_deadline
            counts=(self.owner.reads,self.owner.writes)
            self.owner.set_dtr(False);self.coverage['DTR_low_after_ns']=self.ns();self.wait_window()
        except BaseException as problem:self.error('capture',problem)
        finally:
            if self.identity is None and(self.root/'identity.json').exists():
                self.identity=json.loads((self.root/'identity.json').read_text())
            if counts:self.observation_io=dict(reads=self.owner.reads-counts[0],writes=self.owner.writes-counts[1])
            if self.owner.dtr_state is False:
                try:self.wait_window();self.high_attempted=True;self.owner.set_dtr(True)
                except BaseException as problem:self.error('DTR_restore',problem)
            elif self.owner.dtr_state is None:
                try:self.wait_window()
                except BaseException as problem:self.error('unknown_DTR_wait',problem)
            try:self.stop_observer()
            except BaseException as problem:self.error('observer_save',problem)
            try:self.stop_sim_if_active()
            except BaseException as problem:self.error('SIM_stop',problem)
            try:
                self.saved=self.gather()
                if self.saved and self.headers is not None:self.release_saved()
            except BaseException as problem:self.error('freeze_save_release',problem)
            pico_idle=host_idle=False;ps=hs=None
            try:
                if self.owner.trustworthy:
                    ps=hc.one(self.owner,'AP2 STATUS','ap2 status ')
                    pico_idle=bool(self.identity and ps['boot']==self.identity['pico_boot']and ps['state']=='0')
                hs=self.host({'op':'status'});host_idle=bool(hs['phase']=='idle'and(not self.identity or hs['boot']==self.identity['host_boot']))
            except BaseException as problem:self.error('final_status',problem)
            try:self.owner.close()
            except BaseException as problem:self.error('owner_close',problem)
            result=domain.run_outcome(self.quality,errors=self.errors,pico_idle=pico_idle,host_idle=host_idle,owner_closed=self.owner.closed,
                Pico_status=ps,host_status=hs,observation_USB_io=self.observation_io,
                automatic_rearm=False,SIM_sent=self.sim_sent,raw_evidence_saved=self.headers is not None)
            try:self.persist(self.root/'result.json',result)
            except BaseException as problem:self.error('result_save',problem);result=domain.operation_failed(result,self.errors)
        return result


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--config',type=Path,required=True);parser.add_argument('--dir',type=Path,required=True)
    parser.add_argument('--execute',action='store_true');args=parser.parse_args()
    cfg=json.loads(args.config.read_text())
    sim=cfg.get('sim')
    if not args.execute:
        print(json.dumps(dict(plan_only=True,device=cfg['device'],expected_Flash_ID=cfg['expected_flash_id'],
              single_owner=True,SIM=sim,window_seconds=36.1,firmware_write=False,service_operation=False)));return
    hc.require(cfg['expected_flash_id']=='E661640843239D21'and cfg['expected_flash_id']in cfg['serial_by_id'],
               'expected bound LCD serial identity')
    hc.require(type(cfg.get('PID'))is int and cfg['PID']>0,'refresh server PID after approved rollout before execution')
    hc.require(hashlib.sha256(Path(f'/proc/{cfg["PID"]}/exe').read_bytes()).hexdigest()==cfg['server_sha'],'server binary changed')
    directory=Path(cfg['observer']);sys.path.insert(0,str(directory))
    for name,digest in cfg['observer_SHA256'].items():
        hc.require(name in ['header_observer.py','observer_client.py']and hashlib.sha256((directory/name).read_bytes()).hexdigest()==digest,'observer source changed')
    hc.require(set(cfg['observer_SHA256'])=={'header_observer.py','observer_client.py'},'observer source manifest')
    observer=importlib.import_module('header_observer');client=importlib.import_module('observer_client')
    os.umask(0o077);args.dir.mkdir(mode=0o700,parents=True,exist_ok=False)
    host=lambda q:hc.ap.host_request(Path(cfg['host_socket']),q)
    from types import SimpleNamespace
    a=SimpleNamespace(device=cfg['device'],host_pid=cfg['PID'],interface=cfg['interface'],known_mac=cfg['known_mac'],
                      local_ip=cfg['local_ip'],port=cfg['port'],socket=Path(cfg['host_socket']))
    try:
        preflight=hc.txlib.refresh_target(a,host,observer,expected_phase='idle',
            neighbor_evidence=args.dir/'neighbor-before-arm.json')
        hc.ap.persist(args.dir/'target-before-arm.private.json',preflight)
    except BaseException as problem:
        hc.ap.persist(args.dir/'target-preflight-error.json',dict(
            type=type(problem).__name__,message=str(problem)[:240],USB_opened=False,ARM_sent=False))
        raise
    def resolve(ready):
        target=hc.txlib.refresh_target(a,host,observer,
            neighbor_evidence=args.dir/'neighbor-after-arm.json')
        hc.txlib.require_same_target(preflight,target)
        return target
    owner=Owner(cfg['serial_by_id'],args.dir/'owner.private.jsonl')
    runner=Runner(owner,host,resolve,lambda target:client.Session(target,seconds=40,prepare_seconds=20),args.dir,sim=sim)
    result=runner.run(cfg['device'],format(time.time_ns(),'x'))
    print(json.dumps({k:report_value(result)[k]for k in ['complete','pico_idle','host_idle','owner_closed','errors']},ensure_ascii=False))


if __name__=='__main__':main()
