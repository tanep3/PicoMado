#include <assert.h>
#include <stdio.h>
#include <string.h>
#include "../src/cyw43_frame_validity.h"

static uint8_t frame[176];
static void put16(uint8_t *p,uint16_t n){p[0]=(uint8_t)(n>>8);p[1]=(uint8_t)n;}
static void prepare(void){
 memset(frame,0,sizeof frame);
 const uint8_t mac[]={2,4,6,8,10,12};memcpy(frame+18,mac,6);
 frame[0]=171;frame[2]=0x54;frame[3]=0xff;frame[4]=19;frame[5]=2;frame[7]=14;
 frame[14]=0x20;frame[30]=0x08;frame[31]=0x00;
 uint8_t *ip=frame+32,*tcp=ip+20;
 ip[0]=0x45;put16(ip+2,139);put16(ip+4,3471);ip[8]=64;ip[9]=6;
 const uint8_t src[]={192,168,0,173},dst[]={192,168,0,120};
 memcpy(ip+12,src,4);memcpy(ip+16,dst,4);
 put16(tcp,61826);put16(tcp+2,16153);tcp[12]=0x50;tcp[13]=0x18;put16(tcp+14,23348);
 for(unsigned i=0;i<99;i++)tcp[20+i]=(uint8_t)(i*7+1);
 put16(ip+10,(uint16_t)~pm_fv_fold(pm_fv_sum(0,ip,20)));
 uint32_t sum=pm_fv_sum(0,ip+12,8)+6+119;
 put16(tcp+16,(uint16_t)~pm_fv_fold(pm_fv_sum(sum,tcp,119)));
}
int main(void){
 prepare();pm_frame_validity first=pm_cyw_validate_frame(frame,sizeof frame,153,19);
 assert(first.checks==127 && first.dest_mac[0]==2 && first.dest_mac[5]==12);
 frame[175]=0x93;assert(pm_cyw_validate_frame(frame,sizeof frame,153,19).checks==127); /* pad outside length */
 uint8_t retry[sizeof frame];memcpy(retry,frame,sizeof retry);
 retry[4]=20;put16(retry+36,3472); /* new SDPCM seq and IPID; recompute IPv4 checksum */
 retry[42]=retry[43]=0;put16(retry+42,(uint16_t)~pm_fv_fold(pm_fv_sum(0,retry+32,20)));
 pm_frame_validity second=pm_cyw_validate_frame(retry,sizeof retry,153,20);
 assert(second.checks==127 && !memcmp(first.dest_mac,second.dest_mac,6));
 frame[42]^=1;assert(pm_cyw_validate_frame(frame,sizeof frame,153,19).checks==(127&~PM_FV_IP_OK));frame[42]^=1;
 frame[170]^=1;assert(pm_cyw_validate_frame(frame,sizeof frame,153,19).checks==(127&~PM_FV_TCP_OK));frame[170]^=1;
 frame[2]^=1;assert(pm_cyw_validate_frame(frame,sizeof frame,153,19).checks==(127&~PM_FV_SDPCM_OK));frame[2]^=1;
 frame[6]=1;assert(pm_cyw_validate_frame(frame,sizeof frame,153,19).checks==(127&~PM_FV_SDPCM_OK));frame[6]=0;
 frame[17]=1;assert(pm_cyw_validate_frame(frame,sizeof frame,153,19).checks==(127&~PM_FV_SDPCM_OK));frame[17]=0;
 assert(pm_cyw_validate_frame(frame,50,153,19).checks==0);
 frame[18]=0x44;assert(pm_cyw_validate_frame(frame,sizeof frame,153,19).dest_mac[0]!=second.dest_mac[0]);
 puts("PASS frame validity: odd TCP checksum, IPv4 checksum, SDPCM complement/offset, padding, MAC, retransmission, bounds");
}
