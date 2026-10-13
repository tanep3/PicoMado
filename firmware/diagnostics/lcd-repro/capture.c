#include "capture.h"
#include "lcd.h"
#include "pico/stdlib.h"
#include <stdio.h>
#include <string.h>

enum { CAPTURE_COUNT=3, CHUNK_BYTES=256 };
static const uint32_t selected_frames[CAPTURE_COUNT]={1,2,7};
typedef struct {
    uint32_t frame, requested_hz, copy_us;
    uint64_t captured_us;
    uintptr_t source;
    char phase[24];
    bool valid;
    uint8_t pixels[PM_IMAGE_BYTES];
} snapshot;
static snapshot snapshots[CAPTURE_COUNT];
static uint32_t context_frame, context_requested_hz;
static const char *context_phase;
static unsigned captured_count;
static char hex_chunk[2*CHUNK_BYTES+1];
_Static_assert(PM_IMAGE_BYTES==25600, "Capture format is the verified 160x80 RGB565 frame");

void pm_capture_set_context(uint32_t frame, const char *phase, uint32_t requested_hz) {
    context_frame=frame;context_phase=phase;context_requested_hz=requested_hz;
}
void pm_lcd_capture_before_transfer(const uint8_t *pixels, size_t length) {
    if(!pixels || length!=PM_IMAGE_BYTES || !context_phase)return;
    for(unsigned i=0;i<CAPTURE_COUNT;i++) {
        if(selected_frames[i]!=context_frame || snapshots[i].valid)continue;
        snapshot *s=&snapshots[i];
        uint64_t before=time_us_64();
        s->frame=context_frame;s->requested_hz=context_requested_hz;
        s->captured_us=before;s->source=(uintptr_t)pixels;
        strncpy(s->phase,context_phase,sizeof s->phase-1);
        memcpy(s->pixels,pixels,length);
        s->copy_us=(uint32_t)(time_us_64()-before);
        s->valid=true;captured_count++;
        // No USB output or checksum work between this copy and the SPI call.
        return;
    }
}
bool pm_capture_ready(void) { return captured_count==CAPTURE_COUNT; }
void pm_capture_dump(void) {
    if(!pm_capture_ready())return;
    printf("LCDCAP BEGIN count=%u\n",(unsigned)CAPTURE_COUNT);
    for(unsigned i=0;i<CAPTURE_COUNT;i++) {
        const snapshot *s=&snapshots[i];
        uint32_t crc=pm_crc32(s->pixels,sizeof s->pixels);
        printf("LCDCAP FRAME frame=%lu bytes=%u crc=%08lx captured_us=%llu "
               "copy_us=%lu source=%lx request=%lu phase=%s\n",
               (unsigned long)s->frame,(unsigned)sizeof s->pixels,(unsigned long)crc,
               (unsigned long long)s->captured_us,(unsigned long)s->copy_us,
               (unsigned long)s->source,(unsigned long)s->requested_hz,s->phase);
        for(size_t offset=0;offset<sizeof s->pixels;offset+=CHUNK_BYTES) {
            size_t count=sizeof s->pixels-offset;
            if(count>CHUNK_BYTES)count=CHUNK_BYTES;
            static const char digits[]="0123456789abcdef";
            for(size_t n=0;n<count;n++) {
                uint8_t b=s->pixels[offset+n];
                hex_chunk[2*n]=digits[b>>4];hex_chunk[2*n+1]=digits[b&15];
            }
            hex_chunk[2*count]=0;
            printf("LCDCAP DATA frame=%lu offset=%u hex=%s\n",
                   (unsigned long)s->frame,(unsigned)offset,hex_chunk);
        }
        printf("LCDCAP END frame=%lu\n",(unsigned long)s->frame);
    }
    printf("LCDCAP DONE count=%u\n",(unsigned)CAPTURE_COUNT);
}
