#include "video.h"
#include <assert.h>
#include <stdio.h>
#include <string.h>
static pm_video q;
static uint8_t a[PM_IMAGE_BYTES],b[PM_IMAGE_BYTES];
static pm_token t={1,1,1,1,1};
int main(void){
    memset(a,0x37,sizeof a);memset(b,0xa9,sizeof b);
    for(unsigned fps=6;fps<=30;fps+=6){
        pm_video_reset(&q);pm_video_prepare(&q,t,fps,1,0);
        uint64_t prior=UINT64_MAX,draws=0;
        for(uint64_t now=0;now<6000000;now+=1000){
            uint64_t seq=now*fps/1000000;
            if(seq!=prior){t.frame_id++;assert(pm_video_offer(&q,t,seq,a,sizeof a));prior=seq;}
            uint8_t *pixels=pm_video_begin_draw(&q,now);
            if(pixels){
                assert(pixels[0]==0x37);uint32_t before=pm_crc32(pixels,sizeof a);
                // A reentrant offer cannot overwrite a drawing region.
                pm_token other=t;other.frame_id++;
                assert(!pm_video_offer(&q,other,seq+1,b,sizeof b));
                assert(pm_crc32(pixels,sizeof a)==before);
                now+=16000;pm_video_finish_draw(&q,now);draws++;
            }
        }
        assert(draws>=fps*6-1 && draws<=fps*6+1);
        assert(sizeof q<PM_IMAGE_BYTES+256);
        printf("video nominal %u fps: %llu draws, bounded two-pixel-region design PASS\n",fps,(unsigned long long)draws);
    }
    pm_video_reset(&q);pm_video_prepare(&q,t,30,2,0);
    for(unsigned n=0;n<100;n++){t.frame_id++;assert(pm_video_offer(&q,t,n,n==99?b:a,sizeof a));}
    assert(q.received==100&&q.dropped==99&&q.pixels[0]==0xa9);
    uint8_t *pixels=pm_video_begin_draw(&q,0);assert(pixels&&pixels[0]==0xa9);
    pm_video_finish_draw(&q,16000);assert(q.rendered==1);
    assert(!pm_video_begin_draw(&q,1000000)); // no repeat/catch-up without a new frame
    pm_token newer=t;newer.display_epoch++;pm_video_prepare(&q,newer,6,3,1000000);
    assert(!q.ready&&q.received==0&&!pm_video_offer(&q,t,101,a,sizeof a));
    puts("video overwrite/drop/order/generation/no-catchup PASS");
}
