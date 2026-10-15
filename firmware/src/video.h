#pragma once
#include "core.h"
// Main owns completed pixels; IRQ only appends transport bytes.
typedef struct {
    pm_token context,token,last_rendered;
    bool active,ready,busy,have_received;
    uint32_t fps;
    uint64_t generation,sequence,last_sequence,first_us,next_us,last_draw_us,max_gap_us;
    uint64_t received,rendered,dropped;
    uint8_t pixels[PM_IMAGE_BYTES];
} pm_video;
void pm_video_reset(pm_video *q);
void pm_video_prepare(pm_video *q,pm_token token,uint32_t fps,uint64_t generation,uint64_t now);
bool pm_video_offer(pm_video *q,pm_token token,uint64_t sequence,const uint8_t *pixels,size_t n);
uint8_t *pm_video_begin_draw(pm_video *q,uint64_t now);
void pm_video_finish_draw(pm_video *q,uint64_t now);
