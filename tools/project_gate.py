#!/usr/bin/env python3
"""Supported development/build/ship gate. Never opens USB or changes a service."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import time
import struct
import tempfile
import importlib.util

ROOT=Path(__file__).resolve().parents[1]
CASE=json.loads((ROOT/'contracts/development-context.json').read_text())['active_change']


def sha(path):return hashlib.sha256(Path(path).read_bytes()).hexdigest()
def require(ok,message):
    if not ok:raise ValueError(message)
def write(path,value):
    path=Path(path);path.parent.mkdir(parents=True,exist_ok=True)
    body=json.dumps(value,indent=2,sort_keys=True)+'\n'
    with tempfile.NamedTemporaryFile('w',dir=path.parent,delete=False)as stream:
        stream.write(body);stream.flush();os.fsync(stream.fileno());tmp=stream.name
    os.replace(tmp,path)


def workflow_module(root=ROOT):
    spec=importlib.util.spec_from_file_location('workflow',root/'development/standard/v1/workflow.py')
    mod=importlib.util.module_from_spec(spec);spec.loader.exec_module(mod);return mod


def input_snapshot(root=ROOT):
    root=Path(root);case=json.loads((root/'contracts/development-context.json').read_text())['active_change']
    change_path='docs/changes/'+case+'.json';change=json.loads((root/change_path).read_text())
    data=subprocess.check_output(['git','ls-files','-z','--cached','--others','--exclude-standard'],cwd=root)
    names=sorted(set(p.decode()for p in data.split(b'\0')if p))
    values={p:sha(root/p)for p in names if (root/p).is_file()}
    mod=workflow_module(root)
    values[change_path]=hashlib.sha256(json.dumps(mod.intent(change),sort_keys=True).encode()).hexdigest()
    subject=hashlib.sha256(json.dumps(values,sort_keys=True).encode()).hexdigest()
    workflow=hashlib.sha256(json.dumps(change,sort_keys=True).encode()).hexdigest()
    evidence=hashlib.sha256(json.dumps([subject,workflow]).encode()).hexdigest()
    return dict(subject_digest=subject,workflow_digest=workflow,source_digest=evidence)


def inputs(root=ROOT):return input_snapshot(root)['source_digest']


def validate_check_proof(report,subject=None):
    if report.get('committed')is not True or report.get('schema')!=2 or report.get('change')!=CASE or report.get('stage')!='implementation'or report.get('complete')is not True:return False
    if subject is not None and report.get('subject_digest')!=subject:return False
    if subprocess.run(['git','merge-base','--is-ancestor',report.get('source_commit',''),'HEAD'],cwd=ROOT,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL).returncode:return False
    expected=hashlib.sha256(json.dumps([report['subject_digest'],report['workflow_digest']]).encode()).hexdigest()
    if report.get('source_digest')!=expected:return False
    if {c['name']for c in report['checks']}!={'structural','usb-domain','rust'}:return False
    return all(c['exit']==0 and sha(c['log'])==c['SHA256']for c in report['checks'])


def workflow_admission(action,root=ROOT):
    record=json.loads((root/'docs/changes'/f'{CASE}.json').read_text());snap=input_snapshot(root)
    def load(ref):
        path=Path(ref['path']);path=path if path.is_absolute()else root/path
        require(path.stat().st_size<=1048576 and sha(path)==ref['SHA256'],'workflow evidence missing/changed')
        return json.loads(path.read_text())
    def design(ref,case):
        commit=ref['commit']
        if subprocess.run(['git','merge-base','--is-ancestor',commit,'HEAD'],cwd=root,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL).returncode:return False
        if commit!=case['design_commit']:return False
        return bool(case['design_artifacts'])and all(subprocess.check_output(['git','show',commit+':'+name],cwd=root)==(root/name).read_bytes() for name in case['design_artifacts'])
    mod=workflow_module(root);errors=mod.admit(record,action,snap['subject_digest'],load,design,lambda p:validate_check_proof(p,snap['subject_digest']))
    require(not errors,'; '.join(errors));return snap


def static(root=ROOT):
    subprocess.run([sys.executable,str(root/'tools/generate_observation.py'),'--check'],check=True)
    spec=importlib.util.spec_from_file_location('standard_check',root/'development/standard/v1/check.py')
    mod=importlib.util.module_from_spec(spec);spec.loader.exec_module(mod)
    errors=mod.check(root,json.loads((root/'contracts/development-boundaries.json').read_text()))
    require(not errors,'\n'.join(errors))
    workflow_admission('develop',root)


def command(args,out,name,env=None):
    path=out/(name+'.log');start=time.monotonic()
    with path.open('w')as log:p=subprocess.run(args,cwd=ROOT,env=env,stdout=log,stderr=subprocess.STDOUT)
    require(p.returncode==0,f'{name} failed; see {path}')
    return dict(name=name,command=args,exit=0,seconds=round(time.monotonic()-start,3),log=str(path),SHA256=sha(path))


def rust_check(out,deps):
    if not deps:
        return command(['cargo','test','--offline','--locked','--all-features','--manifest-path','service/Cargo.toml','--lib','ack_probe::','--','--test-threads=1'],out,'rust',dict(os.environ,CARGO_TARGET_DIR=str(out/'cargo-target')))
    # Explicit local mode reuses read-only compiled dependencies; CI uses Cargo.
    import tomllib
    manifest=tomllib.loads((ROOT/'service/Cargo.toml').read_text());args=['rustc','--test','--edition=2021','--crate-name','picomado_core',str(ROOT/'service/src/lib.rs'),'-L','dependency='+str(deps),'-o',str(out/'rust-tests')]
    for f in manifest['features']:args+=['--cfg','feature="'+f+'"']
    for key in sorted(set(manifest['dependencies'])|set(manifest['dev-dependencies'])):
        name=key.replace('-','_');files=list(Path(deps).glob('lib'+name+'-*.rlib'))
        require(len(files)==1,'ambiguous borrowed dependency '+name);args+=['--extern',name+'='+str(files[0])]
    command(args,out,'rust-compile',dict(os.environ,CARGO_MANIFEST_DIR=str(ROOT/'service')))
    return command([str(out/'rust-tests'),'ack_probe::','--test-threads=1'],out,'rust')


def check(out,deps=None):
    out=Path(out).resolve();require(not(out/'check.json').exists(),'immutable check receipt exists; choose a fresh --out');out.mkdir(parents=True,exist_ok=True);before=inputs();static()
    env=dict(os.environ,PYTHONDONTWRITEBYTECODE='1')
    results=[command([sys.executable,'-m','unittest','discover','-s','tests','-p','test_*.py'],out,'structural',env),
             command(['bash','firmware/tests/run_usb_tx_tests.sh'],out,'usb-domain',env)]
    results.append(rust_check(out,deps))
    require(inputs()==before,'source changed during check')
    report=dict(schema=2,change=CASE,stage='implementation',**input_snapshot(),source_commit=subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),committed=not subprocess.check_output(['git','status','--porcelain'],cwd=ROOT,text=True).strip(),checks=results,complete=True,
        rust_mode='borrowed-read-only-dependencies'if deps else'cargo-offline-locked',physical_actions=0)
    write(out/'check.json',report);print(json.dumps(dict(complete=True,report=str(out/'check.json'),source_digest=before)))


def verify_check(path,action='stage'):
    path=Path(path);report=json.loads(path.read_text());snap=workflow_admission(action)
    require(validate_check_proof(report,snap['subject_digest']),'invalid/stale check receipt or changed logs')
    return report


def artifact(path,kind):
    data=Path(path).read_bytes()
    require(data,'empty artifact')
    if kind=='firmware':
        require(len(data)%512==0,'UF2 length')
        for i in range(len(data)//512):
            block=data[i*512:(i+1)*512];a,b,flags,address,n,index,total,family=struct.unpack_from('<8I',block)
            require(a==0x0a324655 and b==0x9e5d5157 and struct.unpack_from('<I',block,508)[0]==0x0ab16f30
                and flags&0x2000 and family==0xe48bff56 and index==i and total==len(data)//512
                and 0<n<=476 and 0x10000000<=address<address+n<=0x101ff000,'UF2 identity/range/settings sector')
    else:require(data[:4]==b'\x7fELF','server artifact must be ELF')


def lineage_module():
    spec=importlib.util.spec_from_file_location('build_lineage',ROOT/'tools/build_lineage.py')
    mod=importlib.util.module_from_spec(spec);spec.loader.exec_module(mod);return mod


def record(args):
    require(not Path(args.out).exists(),'candidate already exists; preserve it and choose a fresh output')
    report=verify_check(args.report);artifact(args.artifact,args.kind)
    build=lineage_module().verify(ROOT,args.build_receipt,args.artifact,sys.modules[__name__])
    require(build['kind']==args.kind and sha(args.report)==build['check_report_SHA256'],'unrelated build/check receipt')
    value=dict(schema=2,change=CASE,source_digest=report['source_digest'],subject_digest=report['subject_digest'],artifact_SHA256=sha(args.artifact),
        build_receipt=str(Path(args.build_receipt).resolve()),build_receipt_SHA256=sha(args.build_receipt),kind=args.kind,check_report=str(Path(args.report).resolve()),check_report_SHA256=sha(args.report))
    if args.kind=='firmware':
        require(args.device and args.board_id and args.config,'device binding required')
        board=Path(args.board_id).read_text().strip();require(len(board)==16 and all(c in '0123456789abcdefABCDEF'for c in board),'board ID')
        config=str(Path(args.config).resolve());board_file=str(Path(args.board_id).resolve())
        require(build['build']['device']==args.device and build['build']['mode']=='normal'
            and build['build']['configuration'].get(config)==sha(args.config)
            and build['build']['configuration'].get(board_file)==sha(args.board_id),'artifact configuration is not from this build')
        value.update(device=args.device,board_id=board.upper(),config_SHA256=sha(args.config))
    write(args.out,value);print(json.dumps(dict(prepared=True,approval_required=True,manifest=args.out)))


def preflight(args):
    value=json.loads(Path(args.candidate).read_text());report=verify_check(value['check_report'],'ship')
    require(value['subject_digest']==report['subject_digest']and sha(value['check_report'])==value['check_report_SHA256'],'candidate source/receipt changed')
    artifact(args.artifact,value['kind']);require(sha(args.artifact)==value['artifact_SHA256'],'artifact changed')
    require(sha(value['build_receipt'])==value['build_receipt_SHA256'],'build receipt changed')
    build=lineage_module().verify(ROOT,value['build_receipt'],args.artifact,sys.modules[__name__])
    require(build['kind']==value['kind'],'build kind mismatch')
    approval=json.loads(Path(args.review).read_text())
    require(approval.get('accepted')is True and approval.get('owner')=='parent'and approval.get('change')==CASE
        and approval.get('reviewer')and approval.get('subject_digest')==value['subject_digest']
        and approval.get('artifact_SHA256')==value['artifact_SHA256']
        and approval.get('candidate_SHA256')==sha(args.candidate)
        and approval.get('build_receipt_SHA256')==value['build_receipt_SHA256'],'independent parent acceptance missing/mismatched')
    if value['kind']=='firmware':
        require(args.device==value['device']and Path(args.board_id).read_text().strip().upper()==value['board_id']
            and sha(args.config)==value['config_SHA256'],'device/config binding changed')
    print(json.dumps(dict(verified=True,offline=True,physical_actions=0,kind=value['kind'])))


def main():
    p=argparse.ArgumentParser();sub=p.add_subparsers(dest='op',required=True)
    sub.add_parser('static')
    c=sub.add_parser('check');c.add_argument('--out',default=str(ROOT/'.gate'));c.add_argument('--rustc-deps',type=Path)
    c=sub.add_parser('verify-check');c.add_argument('--report',default=str(ROOT/'.gate/check.json'))
    c=sub.add_parser('record');c.add_argument('--build-receipt',required=True);c.add_argument('--report',default=str(ROOT/'.gate/check.json'));c.add_argument('--artifact',required=True);c.add_argument('--kind',choices=['firmware','server'],required=True);c.add_argument('--out',required=True)
    for flag in ['device','board-id','config']:c.add_argument('--'+flag)
    c=sub.add_parser('build');c.add_argument('--kind',choices=['firmware','server'],required=True);c.add_argument('--build-dir',required=True);c.add_argument('--report',default=str(ROOT/'.gate/check.json'));c.add_argument('--out');c.add_argument('--device');c.add_argument('--mode',choices=['normal','sample','unbound','unbound_panel'],default='normal');c.add_argument('--features',default='');c.add_argument('--no-default-features',action='store_true')
    c=sub.add_parser('preflight');c.add_argument('--candidate',required=True);c.add_argument('--review',required=True);c.add_argument('--artifact',required=True)
    for flag in ['device','board-id','config']:c.add_argument('--'+flag)
    a=p.parse_args()
    try:
        if a.op=='static':static();print('static domain/boundary gate PASS')
        elif a.op=='check':check(a.out,a.rustc_deps)
        elif a.op=='verify-check':verify_check(a.report);print('current check receipt PASS')
        elif a.op=='build':print(json.dumps(lineage_module().run(ROOT,a,sys.modules[__name__])))
        elif a.op=='record':record(a)
        elif a.op=='preflight':preflight(a)
    except(Exception,subprocess.CalledProcessError)as e:
        print('GATE REJECTED: '+str(e),file=sys.stderr);raise SystemExit(1)


if __name__=='__main__':main()
