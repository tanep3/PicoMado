#pragma once
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <string.h>

/* BT-AUDIO-20261107: pure bounded PCM ingress. This is NOT a wire decoder.
 * The adapter must serialize calls (same async-context lock) and decode bytes
 * explicitly. Samples are stereo signed16; positions count stereo frames. */
#define PM_BT_PCM_FRAMES 2048u
#define PM_BT_PCM_CHANNELS 2u
_Static_assert((PM_BT_PCM_FRAMES&(PM_BT_PCM_FRAMES-1))==0,"power of two ring");
typedef struct {
    int16_t samples[PM_BT_PCM_FRAMES*PM_BT_PCM_CHANNELS];
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
    s->generation=generation;s->rate=rate;s->next_frame=0;s->read=0;s->used=0;s->active=true;return true;
}
static inline pm_bluetooth_audio_result pm_bluetooth_audio_push(pm_bluetooth_audio *s,
        uint64_t generation,uint64_t first_frame,const int16_t *pcm,size_t frames){
    if(!s||!pcm||!frames||frames>PM_BT_PCM_FRAMES||first_frame>UINT64_MAX-frames)return PM_BT_PCM_INVALID;
    if(!s->active)return PM_BT_PCM_INACTIVE;
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
    if(!s||!pcm||!s->active||!frames||frames>s->used)return false;
    size_t first=frames<PM_BT_PCM_FRAMES-s->read?frames:PM_BT_PCM_FRAMES-s->read;
    memcpy(pcm,&s->samples[s->read*2],first*2*sizeof(int16_t));
    memcpy(pcm+first*2,s->samples,(frames-first)*2*sizeof(int16_t));
    s->read=(s->read+frames)&(PM_BT_PCM_FRAMES-1);s->used-=frames;return true;
}
