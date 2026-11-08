#!/usr/bin/env python3
"""Serial actual main cache controller tests, BT compiled in/out, no hardware."""
import argparse,json,os,subprocess
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument('--out',type=Path,required=True);a=p.parse_args();a.out=a.out.resolve();a.out.mkdir(parents=True,exist_ok=True)
f=Path(__file__).resolve().parents[1];results=[]
for bt in (0,1):
 for name,san in [('native',[]),('sanitized',['-fsanitize=address,undefined','-fno-omit-frame-pointer'])]:
  tag=f'app-bt{bt}-{name}';binary=a.out/tag
  cmd=['cc','-std=gnu11','-O1','-g','-Wall','-Wextra','-Werror','-ffunction-sections','-fdata-sections','-Wl,--gc-sections',*san,
   '-DPM_USE_SAMPLE_CONFIG=1','-include','profiles/pico_w_eink_250x122_2.h','-Idevices/E-ink_1','-Iprofiles','-Isrc',
   '-DPM_NORMAL_BUILD=1',f'-DPM_BLUETOOTH_AUDIO={bt}','-DPM_SERVER_LINK_HINT=1','-DPM_SERVER_LINK_CACHE=1',
   '-DPM_EXPECTED_BOARD_ID="0000000000000001"','-DPM_BUILD_DEVICE="E-ink_1"','-Itests/app_stubs',
   'tests/server_link_cache_app_test.c','src/core.c','src/video.c','src/trace.c','src/rx_stats.c','src/server_link_hint.c','src/server_link_cache.c','-o',str(binary)]
  for label,args in [('compile-'+tag,cmd),(tag,[str(binary)])]:
   r=subprocess.run(args,cwd=f,capture_output=True,text=True,env=dict(os.environ,ASAN_OPTIONS='detect_leaks=0'),timeout=120)
   (a.out/(label+'.log')).write_text(r.stdout+r.stderr);results.append(dict(label=label,command=args,exit=r.returncode))
   (a.out/'results.json').write_text(json.dumps(results,indent=2)+'\n')
   if r.returncode:raise SystemExit(label+'\n'+r.stdout+r.stderr)
   if label==tag:
    hello=next(l[6:] for l in r.stdout.splitlines() if l.startswith('HELLO '));j=json.loads(hello)
    assert j['data']['remembered_server_link']['server_ipv4']=='192.168.0.120'
    print(tag, r.stdout.splitlines()[-1],flush=True)
print('PASS',len(results),'actual main cache steps')
