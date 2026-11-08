#pragma once
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
/* BT-AUDIO-20261107: immutable, explicit wire values, not SDK enum casts. */
#define PM_BT_SBC_FRAMES 16384u
#define PM_BT_SBC_STORAGE 16384u
#define PM_BT_SBC_SAMPLES 128u
#define PM_BT_SBC_CHUNK 8u
typedef struct {
    uint32_t rate;
    uint8_t channels,blocks,subbands,channel_mode,allocation,bitpool;
} pm_bt_sbc_config;
static inline bool pm_bt_sbc_config_valid(const pm_bt_sbc_config *c){
    return c&&c->rate==48000&&c->channels==2&&c->blocks==16&&c->subbands==8&&
        (c->channel_mode==2||c->channel_mode==3)&&c->allocation<=1&&c->bitpool>=2&&c->bitpool<=53;
}
static inline bool pm_bt_sbc_config_equal(const pm_bt_sbc_config *a,const pm_bt_sbc_config *b){
    return a->rate==b->rate&&a->channels==b->channels&&a->blocks==b->blocks&&
        a->subbands==b->subbands&&a->channel_mode==b->channel_mode&&a->allocation==b->allocation&&a->bitpool==b->bitpool;
}
static inline unsigned pm_bt_sbc_frame_bytes(const pm_bt_sbc_config *c){
    return 12u+(c->channel_mode==3)+2u*c->bitpool;
}
static inline uint8_t pm_bt_sbc_crc_byte(uint8_t crc,uint8_t byte){
    for(unsigned i=0;i<8;i++){
        bool feedback=((crc^byte)&0x80)!=0;
        crc=(uint8_t)(crc<<1);if(feedback)crc^=0x1d;byte=(uint8_t)(byte<<1);
    }
    return crc;
}
static inline bool pm_bt_sbc_validate(const pm_bt_sbc_config *c,const uint8_t *p,size_t n,size_t *frames){
    if(!pm_bt_sbc_config_valid(c)||!p)return false;
    unsigned bytes=pm_bt_sbc_frame_bytes(c);
    if(!n||n%bytes||n/bytes>PM_BT_SBC_CHUNK)return false;
    uint8_t header=(uint8_t)(0xf1|(c->channel_mode<<2)|(c->allocation<<1));
    for(size_t off=0;off<n;off+=bytes){
        const uint8_t *f=p+off;
        if(f[0]!=0x9c||f[1]!=header||f[2]!=c->bitpool||(c->channel_mode==3&&(f[4]&1)))return false;
        uint8_t crc=pm_bt_sbc_crc_byte(pm_bt_sbc_crc_byte(0x0f,f[1]),f[2]);
        for(unsigned j=4;j<12u+(c->channel_mode==3);j++)crc=pm_bt_sbc_crc_byte(crc,f[j]);
        if(crc!=f[3])return false;
    }
    if(frames)*frames=n/bytes*PM_BT_SBC_SAMPLES;
    return true;
}
