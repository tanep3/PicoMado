#!/usr/bin/env python3
"""Read-only host collector. Always retain partial evidence and final status."""
import argparse,json,os,subprocess,time
from pathlib import Path

def process(pid):
 root=Path('/proc')/str(pid);stat=(root/'stat').read_text().rsplit(')',1)[1].split()
 result={'utime':int(stat[11]),'stime':int(stat[12]),'starttime':int(stat[19]),
         'threads':int(stat[17]),'rss':int(stat[21])*os.sysconf('SC_PAGE_SIZE')}
 for name in ['io','schedstat']:
  try:result[name]=(root/name).read_text()
  except OSError as e:result[name]={'error':str(e)}
 return result

def partial_text(value):
 return value.decode(errors='replace') if isinstance(value,bytes) else value or ''

def collect(pid,seconds,directory):
 directory.mkdir(parents=True,exist_ok=False)
 start=time.time();mono=time.monotonic();count=0;errors=[];finished=False
 journal_exit=None;journal_error=''
 try:
  with (directory/'host.jsonl').open('w') as out:
   initial=process(pid)
   while time.monotonic()-mono<seconds:
    tick=time.monotonic();row={'unix_us':time.time_ns()//1000,'mono_ns':time.monotonic_ns(),
        'server':process(pid),'collector':process(os.getpid())}
    if row['server']['starttime']!=initial['starttime']:raise RuntimeError('PID incarnation changed')
    if count%2==0:
     command=['ss','-tinm','sport = :16153']
     try:
      r=subprocess.run(command,capture_output=True,text=True,timeout=5)
      row['ss']=r.stdout;row['ss_exit']=r.returncode
      if r.returncode:errors.append({'command':command,'kind':'exit','exit':r.returncode,'stderr':r.stderr})
     except subprocess.TimeoutExpired as e:
      issue={'command':command,'kind':'timeout','timeout_s':e.timeout,
             'stdout':partial_text(e.stdout),'stderr':partial_text(e.stderr)}
      row['ss_error']=issue;row['ss']=issue['stdout'];errors.append(issue)
     except OSError as e:
      issue={'command':command,'kind':'os_error','error':str(e)}
      row['ss_error']=issue;errors.append(issue)
    out.write(json.dumps(row)+'\n');out.flush();count+=1
    time.sleep(max(0,1-(time.monotonic()-tick)))
   finished=True
 except (OSError,RuntimeError,KeyboardInterrupt) as e:
  errors.append({'phase':'sampling','kind':type(e).__name__,'error':str(e)})
 finally:
  end=time.time();measurement_duration=time.monotonic()-mono
  command=['journalctl','--user','-u','picomado.service','--since','@'+str(start),
           '--until','@'+str(end),'-o','json','--no-pager']
  try:
   with (directory/'journal.jsonl').open('w') as out:
    try:
     r=subprocess.run(command,stdout=out,stderr=subprocess.PIPE,text=True,timeout=30)
     journal_exit=r.returncode;journal_error=r.stderr
     if r.returncode:errors.append({'command':command,'kind':'exit','exit':r.returncode,'stderr':r.stderr})
    except subprocess.TimeoutExpired as e:
     journal_error=partial_text(e.stderr)
     if e.stdout and out.tell()==0:out.write(partial_text(e.stdout))
     errors.append({'command':command,'kind':'timeout','timeout_s':e.timeout,'stderr':journal_error})
  except OSError as e:
   errors.append({'command':command,'kind':'os_error','error':str(e)})
  result={'pid':pid,'start_unix':start,'end_unix':end,'duration':measurement_duration,
          'samples':count,'sampling_finished':finished,'errors':errors,
          'journal_exit':journal_exit,'journal_error':journal_error,
          'complete':finished and not errors and journal_exit==0}
  temporary=directory/'result.json.tmp';temporary.write_text(json.dumps(result,indent=2)+'\n')
  temporary.replace(directory/'result.json')
 return result

def main():
 p=argparse.ArgumentParser();p.add_argument('--plan',action='store_true');p.add_argument('--pid',type=int)
 p.add_argument('--seconds',type=int,default=180);p.add_argument('--out',type=Path);a=p.parse_args()
 if a.plan:
  print(json.dumps({'process_fields':['utime','stime','starttime','threads','rss','io','schedstat'],
    'process_interval_s':1,'ss_interval_s':2,'collector_self_metrics':True,
    'journal':'existing user picomado.service records read once after window',
    'usb':'none; separately obtain bounded frozen snapshot after window',
    'failure':'timeouts retain partial evidence and complete=false',
    'mutations':'output files only; no API writes, service commands, SSH or capture'},ensure_ascii=False,indent=2));return
 if not a.pid or not a.out or not 1<=a.seconds<=3600:p.error('explicit --pid --out and seconds 1..3600 required')
 collect(a.pid,a.seconds,a.out)
if __name__=='__main__':main()
