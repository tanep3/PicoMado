from pathlib import Path
import subprocess,hashlib,sys,tempfile,os,argparse
F=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(F/"tools"))
import prepare_cyw43_inner as overlay
parser=argparse.ArgumentParser()
parser.add_argument('--sdk',type=Path,default=Path(os.environ.get('PICO_SDK_PATH',F.parent.parent/'pico-sdk')))
args=parser.parse_args()
sdk_source=args.sdk/'lib/cyw43-driver/src/cyw43_ll.c'
if not sdk_source.is_file():parser.error('SDK source unavailable; set PICO_SDK_PATH or --sdk')
src=sdk_source.read_text()
source=src
def function(signature):
 a=src.index(signature); b=src.index('{',a); n=1;i=b+1
 while n:
  n+=(src[i]=='{')-(src[i]=='}');i+=1
 return src[a:i]
prefix=r'''
#include <stdint.h>
#include <stdbool.h>
#include <assert.h>
#include <stdio.h>
#define USE_KSO 1
#define BACKPLANE_FUNCTION 1
#define SDIO_SLEEP_CSR 2
#define SBSDIO_SLPCSR_KEEP_SDIO_ON 1
#define SBSDIO_SLPCSR_DEVICE_ON 2
#define CYW43_WARN(...) (++warnings)
#define CYW43_VDEBUG(...) ((void)0)
#define CYW43_STAT_INC(x) ((void)0)
typedef struct {bool bus_is_up;} cyw43_int_t;
typedef cyw43_int_t cyw43_ll_t;
static unsigned reads,writes,delays,warnings,awake;
static int response;
static void cyw43_write_reg_u8(cyw43_int_t*s,int f,int r,uint8_t v){(void)s;(void)f;(void)r;(void)v;writes++;}
static int cyw43_read_reg_u8(cyw43_int_t*s,int f,int r){(void)s;(void)f;(void)r;reads++;return response;}
static void cyw43_delay_ms(unsigned n){delays+=n;}
static void cyw43_cb_ensure_awake(void*s){(void)s;awake++;}
'''
code=prefix
for sig in ['static void cyw43_kso_set(', 'static void cyw43_ll_bus_sleep_helper(', 'void cyw43_ll_bus_sleep(']:code+='\n'+function(sig)
code+=r'''
static void reset(int result){reads=writes=delays=warnings=awake=0;response=result;}
int main(void){
 cyw43_int_t state={false};reset(0);
 cyw43_ll_bus_sleep(&state,false);
 assert(state.bus_is_up && reads==64 && writes==66 && delays==64 && warnings==1);
 unsigned old=reads;cyw43_ll_bus_sleep(&state,false);assert(reads==old && awake==2);
 printf("FAIL fixture: 64 failed KSO reads, 66 writes, 64ms delay, warning, logical up; next wake skipped\n");
 reset(0);cyw43_ll_bus_sleep(&state,true);assert(!state.bus_is_up && reads==1);
 response=3;cyw43_ll_bus_sleep(&state,false);assert(state.bus_is_up && reads==2 && warnings==0);
 printf("RECOVERY fixture: subsequent sleep then successful wake recovers; not necessarily permanent\n");
 state.bus_is_up=false;reset(-1);cyw43_ll_bus_sleep(&state,false);assert(state.bus_is_up && reads==64 && warnings==1);
 state.bus_is_up=false;reset(255);cyw43_ll_bus_sleep(&state,false);assert(state.bus_is_up && reads==64 && warnings==1);
 state.bus_is_up=false;reset(3);cyw43_ll_bus_sleep(&state,false);assert(state.bus_is_up && reads==1 && writes==2 && delays==0 && warnings==0);
 printf("PASS negative/0xff failure and immediate valid wake controls\n");
}
'''
with tempfile.TemporaryDirectory(prefix='picomado-sleep-') as tmp:
 tmp=Path(tmp)
 def run(code,name):
  path=tmp/(name+'.c');path.write_text(code)
  subprocess.run(['cc','-std=c11','-Wall','-Wextra','-Werror','-I'+str(F/'src'),str(path),'-o',str(tmp/name)],check=True)
  return subprocess.check_output([str(tmp/name)],text=True)
 plain=run(code,'plain')
 src=overlay.generate_sleep(source)
 instrumented=prefix+'\n#include "cyw43_sleep_probe_impl.h"\nstatic uint32_t cyw43_hal_ticks_us(void){return delays*1000;}\n'
 for sig in ['static void cyw43_kso_set(', 'static void cyw43_ll_bus_sleep_helper(', 'void cyw43_ll_bus_sleep(']:instrumented+='\n'+function(sig)
 tail=code[code.index('static void reset('):]
 tail=tail.replace('assert(state.bus_is_up && reads==64 && writes==66 && delays==64 && warnings==1);','assert(state.bus_is_up && reads==64 && writes==66 && delays==64 && warnings==1); pm_sleep_status s;pm_sleep_get(&s);assert(s.wake==1 && s.failure[1]==1 && s.max_loops==64 && s.after && !s.before && s.last_failure_us==64000);')
 tail=tail[:tail.rfind('}')]+"pm_sleep_state.wake=UINT32_MAX;pm_sleep_transition(false,true,UINT32_MAX-5,4);assert(pm_sleep_state.wake==UINT32_MAX && pm_sleep_state.duration_us==10);\n"+tail[tail.rfind('}'):]

 assert run(instrumented+tail,'probe')==plain
 print(plain,end='')
 print('PASS exact SDK sleep probe preserves register/write/delay behavior; counters and wrap tested')
