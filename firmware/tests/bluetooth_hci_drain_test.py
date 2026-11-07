"""Compile the actual SDK drain function against fake IO. No radio/hardware IO."""
from pathlib import Path
import os, subprocess, tempfile
root=Path(__file__).resolve().parents[1]
sdk=Path(os.environ.get('PICO_SDK_PATH',root.parent.parent/'pico-sdk'))
source=sdk/'src/rp2_common/pico_cyw43_driver/btstack_hci_transport_cyw43.c'
s=source.read_text()
marker='static void hci_transport_cyw43_process(void) {'
assert s.count(marker)==1
start=s.index(marker); opening=s.index('{',start); depth=1; end=opening+1
while depth:
    assert end<len(s)
    depth+=(s[end]=='{')-(s[end]=='}');end+=1
body=s[start:end]
assert 'PICO_BTSTACK_CYW43_MAX_HCI_PROCESS_LOOP_COUNT' in body
prefix=r'''
#include <assert.h>
#include <stdint.h>
#include <stdbool.h>
#define CYW43_THREAD_LOCK_CHECK
#define CYW43_PACKET_HEADER_SIZE 4
#define HCI_INCOMING_PACKET_BUFFER_SIZE 32
#define BT_DEBUG(...) ((void)0)
static uint8_t cyw43_receive_buffer[36];
static unsigned reads,dispatches,remaining;
static bool endless,error;
static int cyw43_bluetooth_hci_read(uint8_t *p,unsigned cap,uint32_t *len){
    assert(cap==36);assert(++reads<1000);
    if(error){*len=99;return -1;}
    *len=(remaining||endless)?5:0;
    if(remaining)remaining--;
    p[3]=2;return 0;
}
static void hci_transport_cyw43_packet_handler(uint8_t type,uint8_t *p,uint32_t size){
    assert(type==2&&p==cyw43_receive_buffer+4&&size==1);dispatches++;
}
'''
suffix=r'''
int main(void){
    remaining=40;hci_transport_cyw43_process();
#ifdef PICO_BTSTACK_CYW43_MAX_HCI_PROCESS_LOOP_COUNT
    assert(reads==PICO_BTSTACK_CYW43_MAX_HCI_PROCESS_LOOP_COUNT);
    assert(dispatches==reads&&remaining==40-reads);
    while(remaining)hci_transport_cyw43_process();
    assert(dispatches==40);
    reads=dispatches=0;endless=true;hci_transport_cyw43_process();
    assert(reads==PICO_BTSTACK_CYW43_MAX_HCI_PROCESS_LOOP_COUNT&&dispatches==reads);
    endless=false;
#else
    assert(reads==41&&dispatches==40&&remaining==0);
#endif
    reads=dispatches=0;hci_transport_cyw43_process();assert(reads==1&&dispatches==0);
    reads=0;error=true;hci_transport_cyw43_process();assert(reads==1&&dispatches==0);
}
'''
with tempfile.TemporaryDirectory(prefix='picomado-hci-drain.') as d:
    p=Path(d);(p/'test.c').write_text(prefix+body+suffix)
    for limit in (0,1,4,16):
        flags=[] if not limit else [f'-DPICO_BTSTACK_CYW43_MAX_HCI_PROCESS_LOOP_COUNT={limit}']
        subprocess.run(['cc','-std=c11','-Wall','-Wextra','-Werror',*flags,str(p/'test.c'),'-o',str(p/'test')],check=True)
        subprocess.run([str(p/'test')],check=True)
    module=root/'cmake/bluetooth_hci_limit.cmake'
    for value in ('0','1','4','16','2147483647','-1','1.5','abc','2147483648','999999999999999999999','01',''):
        r=subprocess.run(['cmake',f'-DPM_BT_HCI_DRAIN_LIMIT={value}','-P',str(module)],capture_output=True)
        valid=value in ('0','1','4','16','2147483647')
        assert (r.returncode==0)==valid,(value,r.stderr)
    default=subprocess.run(['cmake','-P',str(module)],capture_output=True)
    assert default.returncode==0
print('HCI actual SDK body: default/1/4/16, next poll, endless bounded, empty/error; CMake validation PASS')
