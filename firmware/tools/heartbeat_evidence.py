"""Shared durable capture validation. No USB, process or network operations."""
import hashlib
import json
from pathlib import Path
import zlib
import heartbeat_capture as hc
import observation_domain as domain


def coverage_ok(headers):
    from observation_rules import coverage_ok as rule
    return rule(headers)


FULL_FILES={'identity.json','ready.json','pico.json','host.json','extra.json',
            'headers.json','quality.json','headers.jsonl','recvq.jsonl'}


def read(root,name):
    path=root/name
    hc.require(path.stat().st_size<=1048576,'JSON file size bound')
    return json.loads(path.read_text())


def identity(ready):
    return {k:ready[k]for k in ['pico_boot','host_boot','run','device','host_connection','session']}


def hashes(root,names):
    result={}
    for name in names:
        path=root/name
        hc.require(path.stat().st_size<=(524288 if name.endswith('.jsonl')else 1048576),'saved file size bound')
        result[name]=hashlib.sha256(path.read_bytes()).hexdigest()
    return result


def confirm_saved(root,ready):
    root=Path(root);hc.ap.persist(root/'ready.json',ready);hc.ap.persist(root/'identity.json',identity(ready))
    quality=read(root,'quality.json')
    manifest=dict(kind='heartbeat-v1',ready=ready,identity=identity(ready),quality=quality,
        pico_digest=int(read(root,'pico.json')['status']['digest'],16),host_digest=read(root,'host.json')['status']['digest'],
        hashes=hashes(root,FULL_FILES),released=False)
    hc.ap.persist(root/'saved.json',manifest)
    return validate_directory(root)


def partial_names(host_captured):
    return {'identity.json','partial.json','pico.json','extra.json','headers.json','quality.json',
            'headers.jsonl','recvq.jsonl','host.json'if host_captured else'host-not-armed.json'}


def confirm_partial(root,ident,host_captured):
    root=Path(root)
    hc.ap.persist(root/'partial.json',dict(identity=ident,host_captured=host_captured,never_complete=True))
    hc.ap.persist(root/'saved.json',dict(kind='partial-heartbeat-v1',identity=ident,host_captured=host_captured,
        quality=read(root,'quality.json'),hashes=hashes(root,partial_names(host_captured)),released=False))
    return validate_directory(root)


def host_digest(rows):
    digest=0
    for row in rows:
        canonical={k:row[k]for k in ['us','event','connection','message','session','frame','kind','a','b']}
        digest=zlib.crc32(json.dumps(canonical,separators=(',',':')).encode(),digest)
    return digest


def validate_directory(directory):
    root=Path(directory)
    try:
        saved=read(root,'saved.json');kind=saved.get('kind')
        hc.require(kind in ['heartbeat-v1','partial-heartbeat-v1'],'saved schema')
        partial=kind=='partial-heartbeat-v1';ident=read(root,'identity.json')
        hc.require(saved['identity']==ident,'saved identity changed')
        names=partial_names(saved['host_captured'])if partial else FULL_FILES
        hc.require(set(saved['hashes'])==names,'required saved hash set')
        hc.require(hashes(root,names)==saved['hashes'],'saved evidence SHA changed')
        headers=read(root,'headers.json');quality=read(root,'quality.json');p=read(root,'pico.json')
        hc.require(quality==saved['quality'],'saved quality changed')
        hc.require(p['status']['boot']==ident['pico_boot']and p['status']['run']==ident['run']and p['status']['state']=='3','Pico saved identity/state')
        hc.require(hc.ap.fnv(p['records'])==int(p['status']['digest'],16),'Pico saved digest')
        release=dict(pico=dict(boot=ident['pico_boot'],run=ident['run'],digest=int(p['status']['digest'],16)),host=None)
        captured=not partial or saved['host_captured']
        if captured:
            h=read(root,'host.json')
            hc.require(h['status']['boot']==ident['host_boot']and h['status']['run']==ident['run']and h['status']['phase']=='frozen','host saved identity/state')
            hc.require(host_digest(h['records'])==h['status']['digest'],'host saved digest')
            release['host']=dict(boot=ident['host_boot'],run=ident['run'],digest=h['status']['digest'])
        else:
            h=read(root,'host-not-armed.json');hc.require(h['boot']==ident['host_boot']and h['phase']=='idle','host partial state')
        hc.require(set(headers['SHA256'])=={'headers.jsonl','recvq.jsonl'},'raw observer manifest')
        hc.require(all(saved['hashes'][n]==sha for n,sha in headers['SHA256'].items()),'raw observer SHA')
        packets=[json.loads(line)for line in(root/'headers.jsonl').read_text().splitlines()]
        [json.loads(line)for line in(root/'recvq.jsonl').read_text().splitlines()]
        if partial:
            hc.require(read(root,'partial.json')==dict(identity=ident,host_captured=captured,never_complete=True),'partial declaration')
            hc.require(int(p['window']['trigger'],16)==0 and not p['records'],'partial capture already triggered')
            facts=domain.pending_assessment('partial preparation/no corresponding trigger')
        else:
            ready=read(root,'ready.json');extra=read(root,'extra.json')
            hc.require(saved['ready']==ready and identity(ready)==ident,'saved ready mismatch')
            hc.require(saved['pico_digest']==release['pico']['digest']and saved['host_digest']==release['host']['digest'],'saved digest declaration')
            facts=hc.validate(p,h,extra,ready,headers)
            integrity={'Pico identity','host identity','Pico digest','host digest','TX digest','CY digest','record counts','CY binding/schema'}
            hc.require(not integrity.intersection(facts.errors),'saved record integrity')
            workers=[e for e in headers['metadata_events']if e.get('event')=='worker_finished']
            if len(workers)!=1 or workers[0]['packets']!=len(packets)or not all(x.get('scope_valid')is True for x in packets):
                facts=domain.with_error(facts,'raw packet count/scope')
        return domain.confirm(facts,ident,domain._verified_evidence(ident),release=release,kind=kind)
    except(OSError,ValueError,TypeError,KeyError,OverflowError)as problem:
        return domain.incomplete(str(problem)[:240])
