#include <assert.h>
#include <stdio.h>
#include "eink_pending.h"

int main(void){
    pm_eink_pending p={0};pm_eink_frame out;
    pm_token one={1,2,3,4,5},two={1,2,4,4,6},three={1,2,5,4,7};
    assert(!pm_eink_pending_take(&p,true,true,1,2,3,4,&out));
    pm_eink_pending_start(&p,one,10,100);
    assert(p.phase==PM_EINK_RECEIVING);
    assert(!pm_eink_pending_complete(&p,two));
    assert(!pm_eink_pending_take(&p,true,true,1,2,3,4,&out));
    assert(pm_eink_pending_complete(&p,one));
    assert(!pm_eink_pending_complete(&p,one));
    assert(!pm_eink_pending_take(&p,false,true,1,2,3,4,&out));
    assert(p.phase==PM_EINK_READY);
    // A newer accepted header replaces a ready or partial older image.
    pm_eink_pending_start(&p,two,20,101);
    assert(!pm_eink_pending_take(&p,true,true,1,2,4,4,&out));
    pm_eink_pending_start(&p,three,0,0);
    assert(!pm_eink_pending_complete(&p,two));
    assert(pm_eink_pending_complete(&p,three));
    assert(pm_eink_pending_take(&p,true,true,1,2,5,4,&out));
    assert(pm_token_equal(out.token,three)&&!out.refresh_minutes&&!out.refresh_minute);
    assert(!pm_eink_pending_take(&p,true,true,1,2,5,4,&out));
    for(unsigned invalid=0;invalid<5;invalid++){
        pm_eink_pending_start(&p,one,10,100);assert(pm_eink_pending_complete(&p,one));
        assert(!pm_eink_pending_take(&p,true,invalid!=0,1+(invalid==1),2+(invalid==2),3+(invalid==3),4+(invalid==4),&out));
        assert(p.phase==PM_EINK_EMPTY);
    }
    pm_eink_pending_start(&p,two,15,200);pm_eink_pending_reset(&p);
    assert(!pm_eink_pending_complete(&p,two));
    assert(pm_frame_header_valid(PM_W,PM_H,PM_PIXEL_FORMAT_NAME,PM_IMAGE_BYTES,UINT32_MAX));
    assert(!pm_frame_header_valid((1ull<<32)+PM_W,PM_H,PM_PIXEL_FORMAT_NAME,PM_IMAGE_BYTES,0));
    assert(!pm_frame_header_valid(PM_W,PM_H,PM_PIXEL_FORMAT_NAME,(1ull<<32)+PM_IMAGE_BYTES,0));
    assert(!pm_frame_header_valid(PM_W,PM_H,PM_PIXEL_FORMAT_NAME,PM_IMAGE_BYTES,1ull<<32));
    printf("eink pending: pure receive/ready/latest/scope/once/metadata, no pixel buffer (%zu bytes) PASS\n",sizeof p);
}
