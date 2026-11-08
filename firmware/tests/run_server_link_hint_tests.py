#!/usr/bin/env python3
"""Serial offline server-link parser, network owner and production app checks."""
import argparse,json,os,subprocess
from pathlib import Path
p=argparse.ArgumentParser(description=__doc__);p.add_argument('--out',type=Path,required=True)
a=p.parse_args();a.out=a.out.resolve();a.out.mkdir(parents=True,exist_ok=True)
f=Path(__file__).resolve().parents[1];results=[]
def run(label,cmd,fail=False):
 r=subprocess.run(list(map(str,cmd)),cwd=f,capture_output=True,text=True,timeout=90,env=dict(os.environ,ASAN_OPTIONS='detect_leaks=0'))
 (a.out/(label+'.log')).write_text(r.stdout+r.stderr);results.append(dict(label=label,command=list(map(str,cmd)),exit=r.returncode,expected_failure=fail))
 (a.out/'results.json').write_text(json.dumps(results,indent=2)+'\n')
 if (r.returncode!=0)!=fail:raise SystemExit(label+'\n'+r.stdout+r.stderr)
 if not label.startswith('compile'):print(label,r.stdout.strip(),flush=True)
for variant,san in [('native',[]),('sanitized',['-fsanitize=address,undefined','-fno-omit-frame-pointer'])]:
 cc=['cc','-std=gnu11','-O1','-g','-Wall','-Wextra','-Werror']+san
 profile=['-DPM_USE_SAMPLE_CONFIG=1','-include','profiles/pico_w_eink_250x122_2.h','-Idevices/E-ink_1','-Iprofiles','-Isrc']
 net=profile+['-Itests/net_stubs','-Itests/app_stubs','-I.']
 feature=['-DPM_SERVER_LINK_HINT=1','-DETHARP_SUPPORT_STATIC_ENTRIES=1']
 cases=[('parser',['-Isrc','tests/server_link_hint_test.c','src/server_link_hint.c'],[]),
        ('owner',net+feature+['tests/server_link_owner_test.c','src/ws_accept.c','src/server_link_hint.c'],[]),
        ('ordinary-net',net+['tests/net_nagle_test.c','src/ws_accept.c'],[a.out/(variant+'.flags')]),
        ('ordinary-cleanup',net+['tests/net_terminal_cleanup_test.c','src/ws_accept.c'],[])]
 for mode in [0,1]:
  cases.append(('app-'+str(mode),profile+['-DPM_NORMAL_BUILD=1','-DPM_BLUETOOTH_AUDIO=0',f'-DPM_SERVER_LINK_HINT={mode}','-DPM_EXPECTED_BOARD_ID="0000000000000001"','-DPM_BUILD_DEVICE="E-ink_1"','-Itests/app_stubs','tests/server_link_app_test.c','src/core.c','src/video.c','src/trace.c','src/rx_stats.c','src/server_link_hint.c'],[]))
 for label,flags,args in cases:
  name=label+'-'+variant;binary=a.out/name
  run('compile-'+name,cc+flags+['-o',binary]);run(name,[binary]+args)
cmake=a.out/'options.cmake';cmake.write_text('include("'+str(f/'cmake/server_link_hint.cmake')+'")\n')
run('cmake-default',['cmake','-P',cmake]);run('cmake-enabled',['cmake','-DPM_SERVER_LINK_HINT=ON','-P',cmake])
run('cmake-conflict-rejected',['cmake','-DPM_SERVER_LINK_HINT=ON','-DPM_HOME_STATIC_ARP_DIAGNOSTIC=ON','-P',cmake],True)
run('c-invalid-option',['cc','-std=gnu11','-DPM_SERVER_LINK_HINT=2','-Isrc','-fsyntax-only','src/server_link_hint.c'],True)
(a.out/'summary.json').write_text(json.dumps(dict(passed=True,steps=len(results),scope='Actual parser/pure rules/production net owner/main; deterministic adapters only, no network or device operations'),indent=2)+'\n')
print('PASS server link hint',len(results),'steps')
