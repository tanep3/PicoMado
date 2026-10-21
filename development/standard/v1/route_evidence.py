"""IO adapter for scoped application evidence; never installs or applies a route.

Receipts come from the controlled local producer, not from an untrusted attestor.
Project input/entry policy and check validation are injected by its root adapter.
"""
import hashlib
import json
from pathlib import Path
import subprocess
import time


def require(ok,message):
    if not ok:raise ValueError('route evidence: '+message)


def sha(path):return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def canonical(value,directory=False):
    p=Path(value)
    require(p.is_absolute() and p==p.resolve(strict=True),'noncanonical path')
    require(p.is_dir() if directory else p.is_file(),'missing root/file')
    return p


def git(root,*args):
    return subprocess.check_output(['git','-C',str(root),*args],text=True).strip()


def scope(source,active,subject,entries,snapshot):
    source=canonical(str(source),True);active=canonical(str(active),True)
    require(all(git(root,'rev-parse','--show-toplevel')==str(root)for root in [source,active]),'root must be a complete worktree')
    require(snapshot(source)['subject_digest']==subject==snapshot(active)['subject_digest'],'active/source subject mismatch')
    hashes={name:sha(canonical(str(active/name)))for name in entries}
    require(all(hashes[name]==sha(canonical(str(source/name)))for name in entries),'entry differs from reviewed source')
    return dict(source_root=str(source),active_root=str(active),subject_digest=subject,
        source_commit=git(source,'rev-parse','HEAD'),active_commit=git(active,'rev-parse','HEAD'),entries=hashes)


def verify_scope(value,source,subject,entries,snapshot):
    require(value['source_root']==str(source),'wrong reviewed source root')
    current=scope(source,canonical(value['active_root'],True),subject,entries,snapshot)
    for key in ['source_root','active_root','subject_digest','entries']:
        require(value[key]==current[key],'changed scope '+key)
    for key,root in [('source_commit',source),('active_commit',Path(value['active_root']))]:
        require(isinstance(value[key],str)and len(value[key])in [40,64]and all(c in'0123456789abcdef'for c in value[key]),'commit identity')
        require(subprocess.run(['git','-C',str(root),'merge-base','--is-ancestor',value[key],'HEAD'],
            stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL).returncode==0,'unrelated '+key)
        require(all(hashlib.sha256(subprocess.check_output(['git','show',value[key]+':'+name],cwd=root)).hexdigest()==value['entries'][name]
            for name in entries),'entry not from declared '+key)


def verify_probe(ref,need_refusal,source,subject,entries,snapshot,load,check):
    probe=load(ref);require(probe['schema']==1 and probe['kind']=='route-probe','probe schema')
    value=probe['scope'];verify_scope(value,source,subject,entries,snapshot)
    python=canonical(probe['interpreter']['path']);require(sha(python)==probe['interpreter']['SHA256'],'interpreter changed')
    executions=probe['executions'];require(set(executions)<=set(['current-check','stale-check'])and'current-check'in executions,'probe operations')
    good=None
    for op in ['current-check','stale-check']if need_refusal else['current-check']:
        receipt=load(executions[op]);require(receipt['schema']==1 and receipt['kind']=='route-execution','execution schema')
        require(receipt['scope']==value and receipt['operation']==op,'execution scope/operation mismatch')
        inp=receipt['input'];input_path=canonical(inp['path']);body=load(inp)
        if op=='current-check':
            require(check(body,subject),'positive input is not a current check');good=body
        else:
            expected=dict(good,subject_digest='0'*64)
            require(body==expected,'negative input must invalidate only the accepted subject')
        expected_command=[str(python),'-I','-B',str(Path(value['active_root'])/'tools/project_gate.py'),'verify-check','--report',str(input_path)]
        require(receipt['command']==expected_command and receipt['cwd']==value['active_root'],'wrong executed route')
        require(type(receipt['start_ns'])is int and type(receipt['end_ns'])is int and 0<receipt['start_ns']<receipt['end_ns'],'execution time interval')
        log=load(receipt['log'])
        require(log=={k:receipt[k]for k in ['operation','command','cwd','start_ns','end_ns','exit','stdout','stderr']},'saved result log differs')
        if op=='current-check':
            require(receipt['exit']==0 and receipt['stdout']=='current check receipt PASS\n'and receipt['stderr']=='','current check did not pass')
        else:
            require(receipt['exit']==1 and receipt['stdout']==''and receipt['stderr']=='GATE REJECTED: invalid/stale check receipt or changed logs\n','stale subject was not refused by check validation')
    return probe


def verify(stage,proof,record,source,subject,entries,snapshot,load,check):
    require(proof.get('schema')==1,'application/effectiveness schema')
    probe=verify_probe(proof['probe'],stage=='effectiveness_confirmation',source,subject,entries,snapshot,load,check)
    require(all(proof[k]==probe['scope'][k]for k in ['source_root','active_root']),'declaration/probe root mismatch')
    if stage=='effectiveness_confirmation':
        ref=proof['application']
        require(ref in record['stages']['operational_application']['evidence'],'not the declared application proof')
        application=load(ref)
        require(application['change']==record['id'] and application['stage']=='operational_application'
            and application['subject_digest']==subject and application['owner']=='parent' and application['applied']is True,'invalid linked application')
        require(application['probe']==proof['probe'] and all(application[k]==proof[k]for k in ['source_root','active_root']),'effectiveness belongs to another application')
    return True


def produce(source,active,subject,entries,snapshot,check_report,out,python,load,check,write):
    """Read-only route execution. The caller handles prerequisite stage admission."""
    source=canonical(str(source),True);active=canonical(str(active),True);python=canonical(str(python))
    value=scope(source,active,subject,entries,snapshot)
    require(not git(source,'status','--porcelain')and not git(active,'status','--porcelain'),'probe requires committed clean roots')
    report_path=canonical(str(Path(check_report).resolve(strict=True)))
    report=load(dict(path=str(report_path),SHA256=sha(report_path)))
    require(check(report,subject),'probe needs current check receipt')
    out=Path(out).resolve();require(not out.exists(),'preserve existing probe output; choose fresh directory');out.mkdir(parents=True)
    def ref(path):return dict(path=str(path),SHA256=sha(path))
    probe=dict(schema=1,kind='route-probe',scope=value,interpreter=dict(path=str(python),SHA256=sha(python)),executions={})
    for op in ['current-check','stale-check']:
        inp=out/(op+'-input.json');write(inp,report if op=='current-check'else dict(report,subject_digest='0'*64))
        command=[str(python),'-I','-B',str(active/'tools/project_gate.py'),'verify-check','--report',str(inp)]
        start=time.monotonic_ns()
        process=subprocess.run(command,cwd=active,capture_output=True,text=True,timeout=60)
        end=time.monotonic_ns()
        log=dict(operation=op,command=command,cwd=str(active),start_ns=start,end_ns=end,exit=process.returncode,stdout=process.stdout,stderr=process.stderr)
        log_path=out/(op+'.log.json');write(log_path,log)
        receipt=dict(schema=1,kind='route-execution',scope=value,input=ref(inp),log=ref(log_path),**log)
        receipt_path=out/(op+'-execution.json');write(receipt_path,receipt);probe['executions'][op]=ref(receipt_path)
    require(scope(source,active,subject,entries,snapshot)==value,'scope changed during probe')
    require(not git(source,'status','--porcelain')and not git(active,'status','--porcelain'),'roots changed during probe')
    probe_path=out/'probe.json';write(probe_path,probe)
    verify_probe(ref(probe_path),True,source,subject,entries,snapshot,load,check)
    return dict(probe=ref(probe_path),application_approved=False,physical_actions=0)
