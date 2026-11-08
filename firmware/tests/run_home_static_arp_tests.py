#!/usr/bin/env python3
"""Small serial offline static-ARP fixtures; no sockets, USB or SDK changes."""
import argparse,hashlib,json,os,subprocess
from pathlib import Path
p=argparse.ArgumentParser(description=__doc__)
p.add_argument('--sdk',type=Path,required=True);p.add_argument('--out',type=Path,required=True)
a=p.parse_args();a.out=a.out.resolve();a.out.mkdir(parents=True,exist_ok=True)
f=Path(__file__).resolve().parents[1];sdk=a.sdk.resolve()/'lib/lwip/src'
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
hashes={str(sdk/x):sha(sdk/x) for x in ['core/ipv4/etharp.c','include/lwip/etharp.h','include/lwip/opt.h']}
results=[]
def run(label,cmd,fail=False):
 r=subprocess.run(list(map(str,cmd)),cwd=f,capture_output=True,text=True,timeout=90,env=dict(os.environ,ASAN_OPTIONS='detect_leaks=0'))
 (a.out/(label+'.log')).write_text(r.stdout+r.stderr)
 results.append(dict(label=label,command=list(map(str,cmd)),exit=r.returncode,expected_failure=fail))
 (a.out/'results.json').write_text(json.dumps(results,indent=2)+'\n')
 if (r.returncode!=0)!=fail:raise SystemExit(label+'\n'+r.stdout+r.stderr)
 if not label.startswith('compile'):print(label, r.stdout.strip())
for variant,san in [('native',[]),('sanitized',['-fsanitize=address,undefined','-fno-omit-frame-pointer'])]:
 cc=['cc','-std=gnu11','-O1','-g','-Wall','-Wextra','-Werror']+san
 profile=['-DPM_USE_SAMPLE_CONFIG=1','-include','profiles/pico_w_eink_250x122_2.h','-Idevices/E-ink_1','-Iprofiles','-Isrc']
 net=profile+['-Itests/net_stubs','-Itests/app_stubs','-I.']
 diag=['-DPM_HOME_STATIC_ARP_DIAGNOSTIC=1','-DETHARP_SUPPORT_STATIC_ENTRIES=1']
 cases=[('net',net+diag+['tests/home_static_arp_net_test.c','src/ws_accept.c'],[]),
        ('net-off',net+['tests/net_nagle_test.c','src/ws_accept.c'],[a.out/(variant+'.flags')]),
        ('cleanup',net+['tests/net_terminal_cleanup_test.c','src/ws_accept.c'],[]),
        ('sdk',['-DETHARP_SUPPORT_STATIC_ENTRIES=1','-ffunction-sections','-fdata-sections','-Itests/nagle_sdk_stubs','-Itests/app_stubs','-I.','-I'+str(sdk),' -I'+str(sdk/'include')],[])]
 cases[-1]=('sdk',[x.strip() for x in cases[-1][1]]+['tests/home_static_arp_sdk_test.c',sdk/'core/ipv4/ip4_addr.c',sdk/'core/def.c',sdk/'core/stats.c','-Wl,--gc-sections'],[])
 for mode in [0,1]:
  cases.append(('app-'+str(mode),profile+['-DPM_NORMAL_BUILD=1','-DPM_BLUETOOTH_AUDIO=0',f'-DPM_HOME_STATIC_ARP_DIAGNOSTIC={mode}','-DPM_EXPECTED_BOARD_ID="0000000000000001"','-DPM_BUILD_DEVICE="E-ink_1"','-Itests/app_stubs','tests/home_static_arp_app_test.c','src/core.c','src/video.c','src/trace.c','src/rx_stats.c'],[]))
 for label,flags,args in cases:
  name=label+'-'+variant;binary=a.out/name
  run('compile-'+name,cc+flags+['-o',binary]);run(name,[binary]+args)
cmake=a.out/'options.cmake'
cmake.write_text('include("'+str(f/'cmake/home_static_arp.cmake')+'")\n')
run('cmake-default',['cmake','-DPM_DEVICE=E-ink_0','-P',cmake])
run('cmake-eink1',['cmake','-DPM_DEVICE=E-ink_1','-DPM_HOME_STATIC_ARP_DIAGNOSTIC=ON','-P',cmake])
run('cmake-other-rejected',['cmake','-DPM_DEVICE=E-ink_0','-DPM_HOME_STATIC_ARP_DIAGNOSTIC=ON','-P',cmake],True)
assert all(sha(Path(k))==v for k,v in hashes.items())
(a.out/'summary.json').write_text(json.dumps(dict(passed=True,steps=len(results),SDK_unchanged=True,SDK_hashes=hashes,scope='production net/main plus installed ARP; deterministic IO only; no RF or hardware acceptance'),indent=2)+'\n')
print('PASS all static-ARP checks',len(results))
