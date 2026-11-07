#include <assert.h>
#include <stdio.h>
#include "bluetooth_runtime.h"
#define AVRCP_OPERATION_ID_PLAY 0x44
#define AVRCP_OPERATION_ID_PAUSE 0x46
#define AVRCP_OPERATION_ID_FORWARD 0x4b
#define AVRCP_OPERATION_ID_BACKWARD 0x4c
#define AVRCP_OPERATION_ID_VOLUME_UP 0x41
#define AVRCP_OPERATION_ID_VOLUME_DOWN 0x42
static bool wanted=true,input_enabled=true;
static uint16_t remote_cid=2;
static uint32_t input_epoch=1;
static struct {unsigned count;} events;
static pm_bt_event last;
static unsigned emitted;
static uint32_t btstack_run_loop_get_time_ms(void){return 12345;}
static void emit(pm_bt_event e){last=e;emitted++;}
#include "../src/bluetooth_remote_input.inc"
int main(void){
    const uint8_t codes[]={0x44,0x46,0x4b,0x4c,0x41,0x42};
    for(unsigned i=0;i<6;i++){
        handle_remote_input(2,codes[i],true);assert(emitted==2*i+1);
        assert(last.operation==codes[i]&&last.pressed&&last.input_ms==12345&&last.input_epoch==1);
        handle_remote_input(2,codes[i],false);assert(emitted==2*i+2&&!last.pressed);
    }
    handle_remote_input(3,0x44,true);assert(emitted==12);
    handle_remote_input(2,0x40,true);assert(emitted==12);
    wanted=false;handle_remote_input(2,0x44,true);assert(emitted==12);wanted=true;
    input_enabled=false;handle_remote_input(2,0x44,true);assert(emitted==12);input_enabled=true;
    events.count=4;handle_remote_input(2,0x44,false);assert(emitted==12&&input_epoch==2);
    events.count=0;handle_remote_input(2,0x44,false);assert(emitted==13&&last.input_epoch==2);
    puts("Bluetooth actual callback: six opcodes, timestamp, owner/off, bounded queue and lost-edge invalidation PASS");
}
