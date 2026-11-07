#include <assert.h>
#include <stdio.h>
#include <string.h>
#include "bluetooth_input.h"
static pm_bluetooth_input s;
static pm_media_operation op;
static bool input(uint8_t code,bool up,uint32_t received,uint32_t now){
    return pm_bluetooth_input_event(&s,7,code,up,received,now,&op);
}
static void fresh(void){pm_bluetooth_input_reset(&s,7,true);}
int main(void){
    const uint32_t durations[]={0,1799,1800,9999,10000};
    for(unsigned c=0;c<2;c++)for(unsigned i=0;i<5;i++){
        fresh();uint8_t code=c?0x46:0x44;
        assert(!input(code,false,100,100));
        bool result=input(code,true,100+durations[i],100+durations[i]);
        assert(result==(durations[i]<10000));
        if(result)assert(op==(durations[i]>=1800?PM_MEDIA_CANCEL:PM_MEDIA_TOGGLE));
        assert(!input(code,true,101+durations[i],101+durations[i]));
    }
    fresh();assert(!input(0x44,true,0,0));
    assert(!input(0x44,false,0,0));assert(!input(0x44,false,1700,1700));
    assert(input(0x44,true,1800,1800)&&op==PM_MEDIA_CANCEL);
    /* Queue processing delay must not convert a 100ms hold into long press. */
    fresh();assert(!input(0x44,false,100,2000));
    assert(input(0x44,true,200,2100)&&op==PM_MEDIA_TOGGLE);
    fresh();assert(!input(0x44,false,1,10001));assert(!input(0x44,true,10002,10002));
    fresh();assert(!input(0x44,false,1,1));pm_bluetooth_input_expire(&s,10001);
    assert(!input(0x44,true,10002,10002));
    assert(!input(0x44,false,10003,10003));assert(input(0x44,true,10004,10004));
    fresh();uint32_t start=UINT32_MAX-900;
    assert(!input(0x46,false,start,start));assert(input(0x46,true,start+1800,start+1800)&&op==PM_MEDIA_CANCEL);
    const uint8_t codes[]={0x4b,0x4c,0x41,0x42};
    const pm_media_operation ops[]={PM_MEDIA_NEXT,PM_MEDIA_PREVIOUS,PM_MEDIA_VOLUME_UP,PM_MEDIA_VOLUME_DOWN};
    for(unsigned i=0;i<4;i++){
        fresh();assert(input(codes[i],false,1,1)&&op==ops[i]);
        assert(!input(codes[i],false,2,2));assert(!input(codes[i],true,3,3));
        assert(input(codes[i],false,4,4));
    }
    fresh();for(unsigned c=0;c<256;c++)if(c!=0x44&&c!=0x46&&c!=0x4b&&c!=0x4c&&c!=0x41&&c!=0x42)
        assert(!input((uint8_t)c,false,0,0));
    assert(!pm_bluetooth_input_event(&s,6,0x44,false,0,0,&op));
    assert(!pm_bluetooth_input_event(&s,7,0x44,false,0,0,NULL));
    assert(!input(0x44,false,0,0));pm_bluetooth_input_reset(&s,8,true);
    assert(!pm_bluetooth_input_event(&s,8,0x44,true,10,10,&op));
    pm_bluetooth_input_reset(&s,7,false);assert(!input(0x41,false,0,0));
    pm_media_input_session session={0};
    assert(!pm_media_input_allowed(&session,1,2,true));
    pm_media_input_welcome(&session,1,2,1);assert(pm_media_input_allowed(&session,1,2,true));
    assert(!pm_media_input_allowed(&session,1,2,false));assert(!pm_media_input_allowed(&session,2,2,true));
    assert(!pm_media_input_allowed(&session,1,3,true));
    pm_media_input_welcome(&session,1,3,0);assert(!pm_media_input_allowed(&session,1,3,true));
    pm_media_input_welcome(&session,1,3,2);assert(!pm_media_input_allowed(&session,1,3,true));
    assert(!pm_media_operation_name((pm_media_operation)99));
    assert(!strcmp(pm_media_operation_name(PM_MEDIA_TOGGLE),"toggle_playback"));
    puts("Bluetooth input: six operations, release-only toggle/cancel, hold boundaries, queue delay, expiry, wrap, duplicate, generation/session PASS");
}
