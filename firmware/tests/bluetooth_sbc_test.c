#include <assert.h>
#include <stdio.h>
#include "bluetooth_audio.h"
static void frame(pm_bt_sbc_config c,uint8_t *p){
 unsigned n=pm_bt_sbc_frame_bytes(&c);memset(p,0,n);p[0]=0x9c;p[1]=0xf1|(c.channel_mode<<2)|(c.allocation<<1);p[2]=c.bitpool;
 /* Independent bit-at-a-time protected-header CRC. */
 uint8_t crc=15;
 for(unsigned byte=1;byte<12u+(c.channel_mode==3);byte++){
  if(byte==3)continue;
  for(int bit=7;bit>=0;bit--){unsigned top=crc>>7;crc=(uint8_t)(crc<<1);if(top^((p[byte]>>bit)&1))crc^=29;}
 }
 p[3]=crc;
}
static void golden(const char *path){
 FILE *f=fopen(path,"r");assert(f);char line[1024];unsigned count=0;
 while(fgets(line,sizeof line,f)){
  if(line[0]=='#')continue;
  unsigned mode,alloc,pool;char hex[240];
  if(sscanf(line,"%u %u %u %239s",&mode,&alloc,&pool,hex)!=4)continue;
  pm_bt_sbc_config c={48000,2,16,8,mode,alloc,pool};uint8_t bytes[119];size_t n=strlen(hex)/2;
  assert(n<=sizeof bytes);
  for(size_t i=0;i<n;i++){unsigned b;assert(sscanf(hex+2*i,"%2x",&b)==1);bytes[i]=(uint8_t)b;}
  assert(pm_bt_sbc_validate(&c,bytes,n,0));count++;
 }
 fclose(f);assert(count==12);
}
int main(int argc,char **argv){
 if(argc==2)golden(argv[1]);
 pm_bluetooth_audio s={0};uint8_t payload[8*119],out[119];size_t frames;
 for(unsigned mode=2;mode<=3;mode++)for(unsigned alloc=0;alloc<=1;alloc++)for(unsigned pool=2;pool<=53;pool++){
  pm_bt_sbc_config c={48000,2,16,8,mode,alloc,pool};unsigned n=pm_bt_sbc_frame_bytes(&c);
  for(unsigned i=0;i<8;i++)frame(c,payload+i*n);
  assert(pm_bt_sbc_validate(&c,payload,8*n,&frames)&&frames==1024);
  assert(!pm_bt_sbc_validate(&c,payload,8*n-1,0)&&!pm_bt_sbc_validate(&c,payload,0,0));
  assert(!pm_bt_sbc_validate(&c,payload,9*n,0));
  if(mode==3){payload[4]=1;assert(!pm_bt_sbc_validate(&c,payload,n,0));payload[4]=0;}
  for(unsigned b=0;b<4;b++){payload[7*n+b]^=1;assert(!pm_bt_sbc_validate(&c,payload,8*n,0));payload[7*n+b]^=1;}
  assert(pm_bluetooth_audio_sbc_start(&s,7,13,2,&c));
  for(unsigned i=0;i<16;i++)assert(pm_bluetooth_audio_sbc_push(&s,7,2,13+i*1024,payload,8*n)==PM_BT_PCM_OK);
  assert(s.used==16384);assert(pm_bluetooth_audio_sbc_push(&s,7,2,s.next_frame,payload,n)==PM_BT_PCM_FULL);
  assert(pm_bluetooth_audio_sbc_pop(&s,out,sizeof out)&&!memcmp(payload,out,n));
  assert(pm_bluetooth_audio_sbc_push(&s,7,2,s.next_frame,payload,n)==PM_BT_PCM_FULL); // prepared != consumed
  assert(pm_bluetooth_audio_sbc_sent(&s,128));assert(pm_bluetooth_audio_sbc_push(&s,7,2,s.next_frame,payload,n)==PM_BT_PCM_OK);
  // Repeated ring wrap, atomic malformed tail, stale and future rejection.
  for(unsigned i=0;i<1000;i++){
   assert(pm_bluetooth_audio_sbc_pop(&s,out,sizeof out)&&!memcmp(payload,out,n));assert(pm_bluetooth_audio_sbc_sent(&s,128));
   uint64_t next=s.next_frame;size_t used=s.used;payload[n-1]=1; // audio payload is not CRC-protected
   assert(pm_bluetooth_audio_sbc_push(&s,8,2,next,payload,n)==PM_BT_PCM_STALE);
   assert(pm_bluetooth_audio_sbc_push(&s,7,3,next,payload,n)==PM_BT_PCM_STALE);
   payload[n-1]=0;payload[3]^=1;
   assert(pm_bluetooth_audio_sbc_push(&s,7,2,next,payload,n)==PM_BT_PCM_INVALID);assert(s.next_frame==next&&s.used==used);payload[3]^=1;
   assert(pm_bluetooth_audio_sbc_push(&s,7,2,next+128,payload,n)==PM_BT_PCM_GAP);
   assert(pm_bluetooth_audio_sbc_push(&s,7,2,next,payload,n)==PM_BT_PCM_OK);
  }
  assert(pm_bluetooth_audio_sbc_start(&s,7,UINT64_MAX-127,2,&c));assert(pm_bluetooth_audio_sbc_push(&s,7,2,s.next_frame,payload,n)==PM_BT_PCM_INVALID);
  pm_bluetooth_audio_stop(&s);assert(pm_bluetooth_audio_sbc_push(&s,7,2,0,payload,n)==PM_BT_PCM_INACTIVE);
 }
 pm_bt_sbc_config good={48000,2,16,8,3,0,32};
 for(unsigned i=0;i<8;i++){
  pm_bt_sbc_config bad=good;
  switch(i){case 0:bad.rate=44100;break;case 1:bad.channels=1;break;case 2:bad.blocks=8;break;
   case 3:bad.subbands=4;break;case 4:bad.channel_mode=1;break;case 5:bad.allocation=2;break;
   case 6:bad.bitpool=1;break;default:bad.bitpool=54;break;}
  assert(!pm_bluetooth_audio_sbc_start(&s,7,0,1,&bad));
 }
 assert(!pm_bluetooth_audio_sbc_start(&s,7,0,0,&good));
 printf("SBC 208 tuples, CRC negatives, capacity, prepared credit, ring wrap, overflow PASS (audio=%zu bytes)\n",sizeof s);
}
