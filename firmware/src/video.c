#include "video.h"
#include <string.h>
void pm_video_reset(pm_video *q){
    q->active=q->ready=q->busy=q->have_received=false;
    q->received=q->rendered=q->dropped=0;q->next_us=q->last_draw_us=q->max_gap_us=0;
}
void pm_video_prepare(pm_video *q,pm_token t,uint32_t fps,uint64_t gen,uint64_t now){
    if(!q->active || !pm_token_current(t,q->context.incarnation,q->context.session_epoch,
        q->context.display_epoch,q->context.screen_owner_epoch) || q->generation!=gen || q->fps!=fps){
        pm_video_reset(q);q->context=t;q->generation=gen;q->fps=fps;q->first_us=now;q->active=true;
    }
}
bool pm_video_offer(pm_video *q,pm_token t,uint64_t seq,const uint8_t *pixels,size_t n){
    if(!q->active || n!=PM_IMAGE_BYTES || !pm_token_current(t,q->context.incarnation,
        q->context.session_epoch,q->context.display_epoch,q->context.screen_owner_epoch))return false;
    q->received++;
    if(q->busy || (q->have_received && seq<=q->last_sequence)){q->dropped++;return false;}
    if(q->ready)q->dropped++;
    memcpy(q->pixels,pixels,n);q->token=t;q->sequence=seq;q->last_sequence=seq;
    q->have_received=q->ready=true;return true;
}
uint8_t *pm_video_begin_draw(pm_video *q,uint64_t now){
    if(!q->active || !q->ready || q->busy || (q->next_us && now<q->next_us))return NULL;
    uint64_t period=1000000u/q->fps;
    q->next_us=(q->next_us && now-q->next_us<period)?q->next_us+period:now+period;
    q->ready=false;q->busy=true;return q->pixels;
}
void pm_video_finish_draw(pm_video *q,uint64_t now){
    if(!q->busy)return;
    q->busy=false;q->rendered++;q->last_rendered=q->token;
    if(q->last_draw_us && now-q->last_draw_us>q->max_gap_us)q->max_gap_us=now-q->last_draw_us;
    q->last_draw_us=now;
}
