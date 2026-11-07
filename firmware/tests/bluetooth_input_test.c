#include <assert.h>
#include <stdio.h>
#include "bluetooth_input.h"
int main(void){
    pm_bluetooth_input s={0};pm_operation op={0};
    assert(!pm_bluetooth_input_event(&s,0,0x44,false,&op));
    pm_bluetooth_input_reset(&s,7,true);
    assert(!pm_bluetooth_input_event(&s,6,0x44,false,&op));
    assert(!pm_bluetooth_input_event(&s,7,0x44,false,NULL));
    assert(!pm_bluetooth_input_event(&s,7,0x44,true,&op));
    assert(pm_bluetooth_input_event(&s,7,0x44,false,&op)&&op.value==PM_OP_SUB);
    assert(!pm_bluetooth_input_event(&s,7,0x44,false,&op));
    assert(!pm_bluetooth_input_event(&s,7,0x44,true,&op));
    assert(pm_bluetooth_input_event(&s,7,0x44,false,&op));
    assert(pm_bluetooth_input_event(&s,7,0x4b,false,&op)&&op.value==PM_OP_DOWN);
    assert(pm_bluetooth_input_event(&s,7,0x4c,false,&op)&&op.value==PM_OP_UP);
    assert(pm_bluetooth_input_event(&s,7,0x46,false,&op)&&op.value==PM_OP_SUB);
    for(unsigned c=0;c<256;c++)if(c!=0x44&&c!=0x46&&c!=0x4b&&c!=0x4c)
        assert(!pm_bluetooth_input_event(&s,7,(uint8_t)c,false,&op));
    pm_bluetooth_input_reset(&s,8,true);
    assert(!pm_bluetooth_input_event(&s,7,0x44,false,&op));
    assert(pm_bluetooth_input_event(&s,8,0x44,false,&op));
    pm_bluetooth_input_reset(&s,8,false);
    assert(!pm_bluetooth_input_event(&s,8,0x44,false,&op));
    puts("Bluetooth AVRCP semantic input: press/release, duplicate, stale generation, unknown, reconnect PASS");
}
