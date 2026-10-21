#!/usr/bin/env python3
from pathlib import Path
import argparse,subprocess,tempfile,sys,json,os,hashlib
F=Path(__file__).resolve().parents[1];sys.path.insert(0,str(F/'tools'));import prepare_cyw43_inner as patch
p=argparse.ArgumentParser();p.add_argument('--sdk',type=Path,required=True);p.add_argument('--out',type=Path,required=True);a=p.parse_args();a.out.mkdir(parents=True,exist_ok=True)
source=a.sdk/'lib/cyw43-driver/src/cyw43_ll.c';original=source.read_text();modified=patch.generate(original)
def function(text,signature,last=False):
 at=text.rindex(signature)if last else text.index(signature);return text[at:text.index('\n}\n',at)+3]
rows=[]
for variant,text in [('OFF',original),('ON',modified)]:
 d=a.out/variant;d.mkdir(exist_ok=True)
 chunks=[function(text,'static int sdpcm_process_rx_packet('),function(text,'static int cyw43_ll_sdpcm_poll_device(',True),function(text,'static int cyw43_sdpcm_send_common('),function(text,'int cyw43_ll_send_ethernet(')]
 # The pinned SDK introduced a file-scope polarity constant used by the extracted SPI poll.
 start=text.index('// Configure the active level of the host interrupt pin.')
 end=text.index('\n#if CYW43_USE_STATS',start)
 chunks.insert(0,text[start:end])
 chunks.insert(0,'#define CYW43_PIN_WL_HOST_WAKE 1')
 (d/'generated_sdk_functions.inc').write_text('\n\n'.join(chunks))
 for sanitized in [False,True]:
  tag=variant+('-asan-ubsan'if sanitized else'-native');binary=a.out/tag
  args=['cc','-std=gnu11','-Wall','-Wextra','-Werror','-DPM_CYW43_INNER_PROBE='+('1'if variant=='ON'else'0'),'-I'+str(d),'-I'+str(F/'src'),'-I'+str(F/'tests/app_stubs'),str(F/'tests/cyw43_driver_path_test.c'),'-o',str(binary)]
  if sanitized:args+=['-g','-fsanitize=address,undefined','-fno-omit-frame-pointer','-fno-pie','-no-pie']
  c=subprocess.run(args,capture_output=True,text=True,timeout=30);(a.out/(tag+'-compile.log')).write_text(c.stdout+c.stderr)
  if c.returncode:raise RuntimeError(tag+' compile: '+c.stderr[-3000:])
  r=subprocess.run([str(binary)],capture_output=True,text=True,timeout=20,env=dict(os.environ,ASAN_OPTIONS='detect_leaks=0'));(a.out/(tag+'.log')).write_text(r.stdout+r.stderr)
  if r.returncode:raise RuntimeError(tag+' run: '+(r.stdout+r.stderr)[-3000:])
  rows.append({'tag':tag,'command':args,'exit':r.returncode,'wire':[x for x in r.stdout.splitlines()if x.startswith('wire ')]})
assert len(rows[0]['wire'])==18 and all(x['wire']==rows[0]['wire']for x in rows)
result={'all_passed':True,'wire_scenarios':18,'ON_OFF_exact_bus_trace_equal':True,'SDK_original_SHA256':hashlib.sha256(original.encode()).hexdigest(),'patched_SHA256':hashlib.sha256(modified.encode()).hexdigest(),'actual_functions':['cyw43_ll_send_ethernet','cyw43_sdpcm_send_common','cyw43_ll_sdpcm_poll_device SPI','sdpcm_process_rx_packet'],'mocked':'bus register/read/write,clock,async callback; nested callbacks deliberately injected; valid framed RX reaches real parser; not RF test','steps':rows};(a.out/'result.json').write_text(json.dumps(result,indent=2)+'\n');print('PASS exact SDK functions:18 scenarios, native/ASan/UBSan, ON/OFF identical bus trace and error/pbuf semantics')
