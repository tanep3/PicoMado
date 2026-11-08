#!/usr/bin/env python3
from pathlib import Path
import argparse,sys,subprocess,os
F=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(F/'tools'));sys.path.insert(0,str(F/'tests'))
import prepare_cyw43_inner as overlay
from run_cyw43_spi_length_test import build_and_run
p=argparse.ArgumentParser();p.add_argument('--sdk',type=Path,required=True);p.add_argument('--out',type=Path,required=True);a=p.parse_args();a.out.mkdir(parents=True,exist_ok=True)
s=(a.sdk/'lib/cyw43-driver/src/cyw43_ll.c').read_text()
try:
 overlay.generate_credit(s+'\n')
except ValueError:
 pass
else:
 raise AssertionError('unreviewed SDK accepted')
plain=build_and_run(s,'original',a.out,False,inner=False)
probe=build_and_run(overlay.generate_credit(s),'credit',a.out,False,inner=False,credit=True)
assert [x for x in plain if x.startswith('wire ')]==[x for x in probe if x.startswith('wire ')]
unit=r'''
#include <assert.h>
#include "cyw43_credit_probe_impl.h"
int main(void){
 uint8_t buf[80]={0};uint32_t count,drop;pm_credit_row row;
 for(unsigned n=0;n<=64;n++){
  pm_credit_count=pm_credit_dropped=0;
  pm_credit_call c=pm_credit_begin(2,n,buf,12+n,0,1,1,100);
  assert(c.row->covered==n);pm_credit_ready(&c,buf,0,2,1,150);pm_credit_finish(&c,0,false);
  assert(pm_credit_get(0,&row,&count,&drop)&&!row.changed&&row.wait_us==50);
 }
 pm_credit_count=0;
 pm_credit_call shortbuf=pm_credit_begin(0,999,buf,8,0,1,1,0);
 assert(shortbuf.row->covered==0);pm_credit_ready(&shortbuf,buf,0,2,1,1);pm_credit_finish(&shortbuf,-5,false);
 pm_credit_count=0;
 pm_credit_call outer=pm_credit_begin(2,80,buf,sizeof buf,0,0,0,UINT32_MAX-2);
 pm_credit_call inner=pm_credit_begin(0,1,buf,sizeof buf,0,0,0,1);
 assert(outer.row!=inner.row);buf[12]=1;pm_credit_ready(&inner,buf,0,1,0,2);pm_credit_finish(&inner,0,false);
 pm_credit_ready(&outer,buf,0,2,1,3);pm_credit_finish(&outer,0,false);
 assert(outer.row->changed&&outer.row->wait_us==6&&inner.row->changed);
 for(unsigned i=2;i<PM_CREDIT_CAP+3;i++)(void)pm_credit_begin(2,0,buf,sizeof buf,0,0,0,0);
 assert(pm_credit_count==16&&pm_credit_dropped==3);
 pm_credit_dropped=UINT32_MAX;(void)pm_credit_begin(2,0,buf,sizeof buf,0,0,0,0);assert(pm_credit_dropped==UINT32_MAX);
 assert(!pm_credit_get(2,&row,&count,&drop));assert(!pm_credit_get(16,&row,&count,&drop));
}
'''
(a.out/'unit.c').write_text(unit)
cmd=['cc','-std=c11','-Wall','-Wextra','-Werror','-Wno-unused-function','-fsanitize=address,undefined','-fno-pie','-no-pie','-I'+str(F/'src'),str(a.out/'unit.c'),'-o',str(a.out/'unit')]
subprocess.run(cmd,check=True);subprocess.run([str(a.out/'unit')],check=True,env=dict(os.environ,ASAN_OPTIONS='detect_leaks=0'))
print('PASS credit probe actual SDK unchanged wire; changed/unchanged, DATA/control/async, timeout/nesting, short/zero/capacity/saturation ASan/UBSan')
