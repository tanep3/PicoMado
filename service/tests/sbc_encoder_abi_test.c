/* Direct ABI memory/validation fixture; no SDK, service or device. */
#include <assert.h>
#include <stdint.h>
#include <string.h>
#include <stdio.h>
void *pm_sbc_encoder_new(uint32_t,uint8_t,uint8_t,uint8_t,uint8_t,uint8_t,uint8_t);
int pm_sbc_encoder_encode(void *,const uint8_t *,uint32_t,uint8_t *,uint32_t);
void pm_sbc_encoder_free(void *);
int main(void) {
    uint8_t pcm[512], guarded[121];
    for (unsigned i=0;i<256;i++) {
        int32_t value=(int32_t)i*231-28000;
        uint16_t sample=(uint16_t)(value<0?value+65536:value);
        pcm[2*i]=(uint8_t)sample;pcm[2*i+1]=(uint8_t)(sample>>8);
    }
    assert(!pm_sbc_encoder_new(44100,2,16,8,3,0,32));
    assert(!pm_sbc_encoder_new(48000,1,16,8,3,0,32));
    assert(!pm_sbc_encoder_new(48000,2,8,8,3,0,32));
    assert(!pm_sbc_encoder_new(48000,2,16,4,3,0,32));
    assert(!pm_sbc_encoder_new(48000,2,16,8,1,0,32));
    assert(!pm_sbc_encoder_new(48000,2,16,8,3,2,32));
    assert(!pm_sbc_encoder_new(48000,2,16,8,3,0,54));
    for (unsigned mode=2;mode<=3;mode++) for (unsigned alloc=0;alloc<=1;alloc++)
    for (unsigned bp=2;bp<=53;bp++) {
        void *ctx=pm_sbc_encoder_new(48000,2,16,8,mode,alloc,bp);assert(ctx);
        unsigned length=12+(mode==3)+2*bp;
        memset(guarded,0xa5,sizeof guarded);
        assert(pm_sbc_encoder_encode(ctx,pcm,511,guarded+1,119)<0);
        assert(pm_sbc_encoder_encode(ctx,pcm,513,guarded+1,119)<0);
        assert(pm_sbc_encoder_encode(ctx,pcm,512,guarded+1,length-1)<0);
        assert(pm_sbc_encoder_encode(ctx,NULL,512,guarded+1,119)<0);
        for (unsigned i=0;i<sizeof guarded;i++)assert(guarded[i]==0xa5);
        for (unsigned i=0;i<4;i++) {
            memset(guarded,0xa5,sizeof guarded);
            assert(pm_sbc_encoder_encode(ctx,pcm,512,guarded+1,length)==(int)length);
            assert(guarded[0]==0xa5&&guarded[1+length]==0xa5&&guarded[120]==0xa5);
            assert(guarded[1]==0x9c&&guarded[3]==bp);
        }
        pm_sbc_encoder_free(ctx);
    }
    pm_sbc_encoder_free(NULL);
    puts("PASS: C ABI rejection and 832 encoded frame output sentinels");
}
