#include <assert.h>
#include <string.h>
#include "core.h"

int main(void) {
    assert(pm_crc32((const uint8_t *)"123456789",9)==0xcbf43926u);
    static pm_assembly a;
    pm_token t={1,2,3,4,5}, old={1,2,2,4,5};
    static uint8_t image[PM_IMAGE_BYTES];
    for(size_t i=0;i<sizeof image;i++) image[i]=(uint8_t)i;
    uint32_t crc=pm_crc32(image,sizeof image);
    assert(!pm_start(&a,t,80,160,"rgb565be",sizeof image,crc));
    assert(!pm_start(&a,t,160,80,"rgb565le",sizeof image,crc));
    assert(!pm_start(&a,t,160,80,"rgb565be",sizeof image-2,crc));
    assert(pm_start(&a,t,160,80,"rgb565be",sizeof image,crc));
    bool done=false;
    assert(!pm_chunk(&a,t,4,image,100,&done));
    assert(pm_start(&a,t,160,80,"rgb565be",sizeof image,crc));
    assert(!pm_chunk(&a,old,0,image,100,&done));
    assert(pm_start(&a,t,160,80,"rgb565be",sizeof image,crc));
    for(size_t i=0;i<sizeof image;i+=4096) {
        size_t n=sizeof image-i; if(n>4096)n=4096;
        assert(pm_chunk(&a,t,i,image+i,n,&done));
    }
    assert(done && !memcmp(a.pixels,image,sizeof image));
    assert(!pm_token_current(old,1,2,3,4));
    assert(pm_token_current(t,1,2,3,4));
    image[0]^=1;
    assert(pm_start(&a,t,160,80,"rgb565be",sizeof image,crc));
    for(size_t i=0;i<sizeof image;i+=4096) {
        size_t n=sizeof image-i; if(n>4096)n=4096;
        bool ok=pm_chunk(&a,t,i,image+i,n,&done);
        if(i+n==sizeof image) assert(!ok && !done);
        else assert(ok && !done);
    }
    pm_button b={0};
    assert(!pm_button_step(&b,true,100));
    assert(!pm_button_step(&b,false,110));
    assert(!pm_button_step(&b,true,120));
    assert(!pm_button_step(&b,true,154));
    assert(pm_button_step(&b,true,155));
    assert(!pm_button_step(&b,true,160));
    assert(!pm_button_step(&b,false,170));
    assert(!pm_button_step(&b,false,204));
    assert(!pm_button_step(&b,false,205));
    assert(!pm_button_step(&b,true,220));
    assert(pm_button_step(&b,true,255));
    return 0;
}
