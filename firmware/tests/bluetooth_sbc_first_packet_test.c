#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include "sbc_encoder.h"
int main(void){SBC_ENC_PARAMS p;memset(&p,0,sizeof p);int16_t pcm[256]={0};uint8_t out[256]={0};p.s16NumOfBlocks=16;p.s16NumOfSubBands=8;p.s16AllocationMethod=SBC_LOUDNESS;p.s16BitPool=53;p.s16ChannelMode=SBC_JOINT_STEREO;p.s16NumOfChannels=2;p.s16SamplingFreq=SBC_sf48000;SBC_Encoder_Init(&p);assert(p.u16PacketLength==0);p.ps16PcmBuffer=pcm;p.pu8Packet=out;SBC_Encoder(&p);assert(out[0]==0x9c);assert(p.u16PacketLength==119);uint8_t batch[1002];memset(batch,0xa5,sizeof batch);batch[0]=7;unsigned used=1;
for(unsigned i=0;i<7;i++){p.pu8Packet=batch+used;SBC_Encoder(&p);assert(p.u16PacketLength==119);assert(batch[used]==0x9c);used+=p.u16PacketLength;}
assert(used==834&&batch[1000]==0xa5&&batch[1001]==0xa5);
p.s16BitPool=32;SBC_Encoder_Init(&p);memset(batch,0xa5,sizeof batch);batch[0]=9;used=1;
for(unsigned i=0;i<9;i++){p.pu8Packet=batch+used;SBC_Encoder(&p);assert(p.u16PacketLength==77);assert(batch[used]==0x9c);used+=p.u16PacketLength;}
assert(used==694&&batch[694]==0xa5&&batch[704]==0xa5);
printf("SBC bitpool32 batch9/694bytes underMTU704 PASS; batch7/834bytes bounds PASS; first packet: pre-encode length0; post-encode119, sync9c PASS\n");}
