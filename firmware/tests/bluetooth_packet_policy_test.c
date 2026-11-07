#include <assert.h>
#include <stdio.h>
#include "bluetooth_packet_policy.h"
int main(void){
 assert(pm_bt_packet_frames(48000,128,119,1000,1000)==8);
 assert(pm_bt_packet_frames(44100,128,119,1000,1000)==8);
 assert(pm_bt_packet_frames(48000,128,119,600,1000)==5);
 assert(pm_bt_packet_frames(48000,128,119,119,1000)==0);
 assert(pm_bt_packet_frames(48000,128,119,120,1000)==1);
 assert(pm_bt_packet_frames(48000,128,0,1000,1000)==0);
 assert(pm_bt_packet_frames(48000,128,119,2000,256)==2);
 assert(pm_bt_packet_delay(100,896,48000,100)==18);
 assert(pm_bt_packet_delay(100,896,48000,200)==-82);
 assert(pm_bt_packet_delay(100,896,48000,369)==-251);
 assert(pm_bt_packet_delay(UINT32_MAX-9,896,48000,10)==-2);
 assert(pm_bt_choose_bitpool(2,53)==32);
 assert(pm_bt_choose_bitpool(35,53)==35);
 assert(pm_bt_choose_bitpool(2,25)==25);
 assert(pm_bt_choose_bitpool(35,32)==0);
 assert(pm_bt_choose_bitpool(1,53)==0);
 assert(pm_bt_choose_bitpool(2,54)==0);
 assert(pm_bt_sbc_bytes(32,1)==77&&pm_bt_sbc_bytes(32,0)==76);
 assert(pm_bt_packet_frames(48000,128,77,704,1000)==9);
 assert(pm_bt_packet_frames(48000,128,77,1000,1000)==12);
 assert(pm_bt_packet_frames(48000,128,17,1000,1000)==15);
 assert(pm_bt_packet_frames(44100,128,17,1000,1000)==13);
 assert(pm_bt_ready_packet(9,128,1024,false,1024,0)==0);
 assert(pm_bt_ready_packet(9,128,1152,false,1152,0)==9);
 assert(pm_bt_ready_packet(9,128,2048,false,2048,0)==9);
 assert(pm_bt_ready_packet(9,128,256,true,256,256)==2);
 assert(pm_bt_ready_packet(9,128,256,true,256,512)==0);
 assert(pm_bt_ready_packet(9,128,0,true,256,256)==0);
 puts("Bluetooth packet policy: bounded aggregation, MTU, cadence, lateness, rollover PASS");
}
