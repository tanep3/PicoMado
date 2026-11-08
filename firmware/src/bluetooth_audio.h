#pragma once
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <string.h>
#include "bluetooth_sbc.h"

/* BT-AUDIO-20261107: pure bounded PCM ingress. This is NOT a wire decoder.
 * The adapter must serialize calls (same async-context lock) and decode bytes
 * explicitly. Samples are stereo signed16; positions count stereo frames. */
#define PM_BT_PCM_FRAMES 4096u
#define PM_BT_PCM_CHANNELS 2u
_Static_assert((PM_BT_PCM_FRAMES&(PM_BT_PCM_FRAMES-1))==0,"power of two ring");
typedef struct {
    union {
        int16_t samples[PM_BT_PCM_FRAMES*PM_BT_PCM_CHANNELS];
        uint8_t sbc_bytes[PM_BT_SBC_STORAGE];
    };
    pm_bt_sbc_config sbc_config;
    uint64_t codec_epoch,sbc_consumed;
    bool sbc;
    uint64_t generation,next_frame;
    uint32_t rate;
    size_t read,used;
    bool active;
} pm_bluetooth_audio;
typedef enum {PM_BT_PCM_OK,PM_BT_PCM_INACTIVE,PM_BT_PCM_STALE,
    PM_BT_PCM_GAP,PM_BT_PCM_FULL,PM_BT_PCM_INVALID} pm_bluetooth_audio_result;
static inline void pm_bluetooth_audio_stop(pm_bluetooth_audio *s){
    if(!s)return;
    s->active=false;s->read=0;s->used=0;s->next_frame=0;
}
static inline bool pm_bluetooth_audio_start(pm_bluetooth_audio *s,uint64_t generation,uint32_t rate){
    if(!s||!generation||(rate!=44100&&rate!=48000))return false;
    s->sbc=false;s->generation=generation;s->rate=rate;s->next_frame=0;s->read=0;s->used=0;s->active=true;return true;
}
static inline pm_bluetooth_audio_result pm_bluetooth_audio_push(pm_bluetooth_audio *s,
        uint64_t generation,uint64_t first_frame,const int16_t *pcm,size_t frames){
    if(!s||!pcm||!frames||frames>PM_BT_PCM_FRAMES||first_frame>UINT64_MAX-frames)return PM_BT_PCM_INVALID;
    if(!s->active)return PM_BT_PCM_INACTIVE;
    if(s->sbc)return PM_BT_PCM_INVALID;
    if(s->generation!=generation)return PM_BT_PCM_STALE;
    if(first_frame!=s->next_frame)return PM_BT_PCM_GAP;
    if(frames>PM_BT_PCM_FRAMES-s->used)return PM_BT_PCM_FULL;
    size_t end=(s->read+s->used)&(PM_BT_PCM_FRAMES-1);
    size_t first=frames<PM_BT_PCM_FRAMES-end?frames:PM_BT_PCM_FRAMES-end;
    memcpy(&s->samples[end*2],pcm,first*2*sizeof(int16_t));
    memcpy(s->samples,pcm+first*2,(frames-first)*2*sizeof(int16_t));
    s->used+=frames;s->next_frame+=frames;return PM_BT_PCM_OK;
}
/* Exact-frame consume: underflow is reported, never short/old audio or an
 * implicit latest-frame drop. The output buffer is untouched on failure. */
static inline bool pm_bluetooth_audio_pop(pm_bluetooth_audio *s,int16_t *pcm,size_t frames){
    if(!s||!pcm||!s->active||s->sbc||!frames||frames>s->used)return false;
    size_t first=frames<PM_BT_PCM_FRAMES-s->read?frames:PM_BT_PCM_FRAMES-s->read;
    memcpy(pcm,&s->samples[s->read*2],first*2*sizeof(int16_t));
    memcpy(pcm+first*2,s->samples,(frames-first)*2*sizeof(int16_t));
    s->read=(s->read+frames)&(PM_BT_PCM_FRAMES-1);s->used-=frames;return true;
}

_Static_assert(sizeof(((pm_bluetooth_audio *)0)->samples)==PM_BT_SBC_STORAGE,"shared audio storage");
static inline size_t pm_bluetooth_audio_capacity(const pm_bluetooth_audio *s){
    return s->sbc?PM_BT_SBC_FRAMES:PM_BT_PCM_FRAMES;
}
static inline bool pm_bluetooth_audio_sbc_start(pm_bluetooth_audio *s,uint64_t generation,
        uint64_t first,uint64_t epoch,const pm_bt_sbc_config *config){
    if(!s||!generation||!epoch||!pm_bt_sbc_config_valid(config))return false;
    s->sbc=true;s->sbc_config=*config;s->codec_epoch=epoch;s->sbc_consumed=first;
    s->generation=generation;s->rate=config->rate;s->next_frame=first;s->read=0;s->used=0;s->active=true;
    return true;
}
static inline pm_bluetooth_audio_result pm_bluetooth_audio_sbc_push(pm_bluetooth_audio *s,
        uint64_t generation,uint64_t epoch,uint64_t first,const uint8_t *p,size_t n){
    if(!s||!s->active)return PM_BT_PCM_INACTIVE;
    if(!s->sbc)return PM_BT_PCM_INVALID;
    if(s->generation!=generation||s->codec_epoch!=epoch)return PM_BT_PCM_STALE;
    size_t frames;
    if(!pm_bt_sbc_validate(&s->sbc_config,p,n,&frames)||first>UINT64_MAX-frames)return PM_BT_PCM_INVALID;
    if(first!=s->next_frame)return PM_BT_PCM_GAP;
    uint64_t outstanding=s->next_frame-s->sbc_consumed;
    size_t used=s->used/128*pm_bt_sbc_frame_bytes(&s->sbc_config);
    if(outstanding>PM_BT_SBC_FRAMES||frames>PM_BT_SBC_FRAMES-outstanding||n>PM_BT_SBC_STORAGE-used)return PM_BT_PCM_FULL;
    size_t end=(s->read+used)%PM_BT_SBC_STORAGE;
    size_t first_bytes=n<PM_BT_SBC_STORAGE-end?n:PM_BT_SBC_STORAGE-end;
    memcpy(s->sbc_bytes+end,p,first_bytes);memcpy(s->sbc_bytes,p+first_bytes,n-first_bytes);
    s->used+=frames;s->next_frame+=frames;return PM_BT_PCM_OK;
}
/* Moving into a prepared RTP packet releases bytes, but not sender credit. */
static inline bool pm_bluetooth_audio_sbc_pop(pm_bluetooth_audio *s,uint8_t *out,size_t size){
    if(!s||!s->active||!s->sbc||!out||s->used<128)return false;
    size_t n=pm_bt_sbc_frame_bytes(&s->sbc_config);if(size<n)return false;
    size_t first=n<PM_BT_SBC_STORAGE-s->read?n:PM_BT_SBC_STORAGE-s->read;
    memcpy(out,s->sbc_bytes+s->read,first);memcpy(out+first,s->sbc_bytes,n-first);
    s->read=(s->read+n)%PM_BT_SBC_STORAGE;s->used-=128;return true;
}
static inline bool pm_bluetooth_audio_sbc_sent(pm_bluetooth_audio *s,size_t frames){
    if(!s||!s->active||!s->sbc||!frames||frames%128||frames>s->next_frame-s->sbc_consumed-s->used)return false;
    s->sbc_consumed+=frames;return true;
}
