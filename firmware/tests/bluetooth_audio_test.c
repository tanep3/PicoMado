#include <assert.h>
#include <stdio.h>
#include "bluetooth_audio.h"
int main(void){
    pm_bluetooth_audio s={0};int16_t in[PM_BT_PCM_FRAMES*2],out[PM_BT_PCM_FRAMES*2];
    for(size_t i=0;i<PM_BT_PCM_FRAMES*2;i++)in[i]=(int16_t)i;
    assert(!pm_bluetooth_audio_start(&s,0,48000));
    assert(!pm_bluetooth_audio_start(&s,1,8000));
    assert(pm_bluetooth_audio_push(&s,1,0,in,2)==PM_BT_PCM_INACTIVE);
    assert(pm_bluetooth_audio_start(&s,1,48000));
    assert(pm_bluetooth_audio_push(&s,2,0,in,2)==PM_BT_PCM_STALE);
    assert(pm_bluetooth_audio_push(&s,1,1,in,2)==PM_BT_PCM_GAP);
    assert(pm_bluetooth_audio_push(&s,1,0,in,PM_BT_PCM_FRAMES)==PM_BT_PCM_OK);
    assert(pm_bluetooth_audio_push(&s,1,PM_BT_PCM_FRAMES,in,1)==PM_BT_PCM_FULL);
    assert(s.next_frame==PM_BT_PCM_FRAMES);
    assert(pm_bluetooth_audio_pop(&s,out,100));assert(!memcmp(in,out,400));
    assert(pm_bluetooth_audio_push(&s,1,PM_BT_PCM_FRAMES,in,100)==PM_BT_PCM_OK);
    assert(pm_bluetooth_audio_pop(&s,out,PM_BT_PCM_FRAMES));
    assert(!memcmp(out,in+200,(PM_BT_PCM_FRAMES-100)*4));
    assert(!memcmp(out+(PM_BT_PCM_FRAMES-100)*2,in,400));
    out[0]=123;assert(!pm_bluetooth_audio_pop(&s,out,1)&&out[0]==123);
    assert(pm_bluetooth_audio_start(&s,2,44100));
    assert(pm_bluetooth_audio_push(&s,1,0,in,1)==PM_BT_PCM_STALE);
    assert(pm_bluetooth_audio_push(&s,2,UINT64_MAX,in,1)==PM_BT_PCM_INVALID);
    assert(pm_bluetooth_audio_push(&s,2,0,in,1)==PM_BT_PCM_OK);
    pm_bluetooth_audio_stop(&s);assert(s.used==0&&!s.active);
    assert(!pm_bluetooth_audio_pop(&s,out,1));
    puts("Bluetooth PCM: stereo order, wrap, overflow, underflow, generation, reset PASS");
}
