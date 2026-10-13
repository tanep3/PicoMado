#include <assert.h>
#include <stdio.h>
#include <string.h>
#include "lcd.h"
#include "lcd_transform.h"
#include "pico/stdlib.h"
#include "hardware/spi.h"
#if defined(PM_LCD_CAPTURE_ENABLE) && PM_LCD_CAPTURE_ENABLE
#include "capture.h"
uint64_t time_us_64(void) { static uint64_t tick; return ++tick; }
#endif

static spi_inst_t instance;
spi_inst_t *spi0 = &instance;
static int levels[30], output[30];
static unsigned baud, rate_changes, frames;
static unsigned cs_asserts, cs_releases, busy_remaining, busy_waits;
static unsigned expected_control=1000000, expected_frame=20000000;
static uint8_t command, registers[128], x[4], y[4];
static size_t register_count;
static uint8_t captured[PM_IMAGE_BYTES];

void gpio_init(uint pin) { assert(pin<30); output[pin]=0; }
void gpio_put(uint pin, int value) {
    assert(pin<30);
    if(pin==17 && output[pin] && value!=levels[pin]) {
        if(value) { assert(busy_remaining==0);cs_releases++; }
        else cs_asserts++;
    }
    levels[pin]=value;
}
void gpio_set_dir(uint pin, int value) {
    assert(pin<30 && value==GPIO_OUT);
    output[pin]=1;
}
void gpio_set_function(uint pin, int function) {
    assert((pin==18 || pin==19) && function==GPIO_FUNC_SPI);
}
void sleep_ms(unsigned milliseconds) { assert(milliseconds>0); }
unsigned spi_init(spi_inst_t *spi, unsigned requested) {
    assert(spi==spi0); baud=requested; return requested;
}
unsigned spi_set_baudrate(spi_inst_t *spi, unsigned requested) {
    assert(spi==spi0 && levels[17]==1 && requested>0);
    rate_changes++; baud=requested; return requested;
}
unsigned spi_get_baudrate(const spi_inst_t *spi) {
    assert(spi==spi0); return baud;
}
bool spi_is_busy(const spi_inst_t *spi) { assert(spi==spi0);return busy_remaining>0; }
void tight_loop_contents(void) { assert(busy_remaining>0);busy_remaining--;busy_waits++; }
void spi_set_format(spi_inst_t *spi, int bits, int cpol, int cpha, int order) {
    assert(spi==spi0 && bits==8 && !cpol && !cpha && !order);
}
int spi_write_blocking(spi_inst_t *spi, const uint8_t *bytes, size_t length) {
    assert(spi==spi0 && levels[17]==0 && output[17] && output[20]);
    if (!levels[20]) {
        assert(baud==expected_control && length==1 && register_count<sizeof registers);
        command=bytes[0]; registers[register_count++]=command;
    } else if (command==0x2c) {
        assert(baud==expected_frame && length==PM_IMAGE_BYTES);
        memcpy(captured,bytes,length); frames++;
#if defined(PM_LCD_FRAME_CS_HOLD) && PM_LCD_FRAME_CS_HOLD
        if(expected_control==expected_frame)busy_remaining=2;
#endif
    } else {
        assert(baud==expected_control);
        if(command==0x2a) { assert(length==4); memcpy(x,bytes,4); }
        if(command==0x2b) { assert(length==4); memcpy(y,bytes,4); }
    }
    return (int)length;
}
int main(void) {
#ifdef PM_TEST_NORMAL_SPI_BAUD
    expected_control=expected_frame=PM_TEST_NORMAL_SPI_BAUD;
    pm_lcd_init();
    unsigned normal_changes=rate_changes;
    unsigned normal_asserts=cs_asserts, normal_releases=cs_releases;
    pm_lcd_fixed_test_frame();
    assert(rate_changes==normal_changes);
    assert(cs_asserts==normal_asserts+1 && cs_releases==normal_releases+1);
    assert(pm_lcd_last_frame_baud()==PM_TEST_NORMAL_SPI_BAUD);
    assert(pm_lcd_last_transfer_bytes()==25600 && busy_remaining==0);
    printf("normal default: control/frame=%u CS assert/release=1/1 reclock=0 bytes=25600\n",PM_TEST_NORMAL_SPI_BAUD);
    expected_control=1000000;expected_frame=20000000;
    pm_lcd_configure_bus(expected_control,expected_frame);
    rate_changes=frames=0;register_count=0;
#endif
    pm_lcd_init();
    const uint8_t init[]={0x01,0x11,0x3a,0x36,0x21,0xe0,0xe1,0x29};
    assert(register_count==sizeof init && !memcmp(registers,init,sizeof init));
    static uint8_t image[PM_IMAGE_BYTES], expected[PM_IMAGE_BYTES];
    for(size_t i=0;i<sizeof image;i++)image[i]=(uint8_t)(i*17u+3u);
    memcpy(expected,image,sizeof image);
    pm_lcd_transform(expected,PM_W*PM_H,true,true);
    pm_lcd_draw(image);
    const uint8_t expected_x[]={0,0,0,159}, expected_y[]={0,24,0,103};
    assert(!memcmp(x,expected_x,4) && !memcmp(y,expected_y,4));
    assert(frames==1 && !memcmp(captured,expected,sizeof expected));
    assert(baud==1000000 && levels[17]==1 && rate_changes==2);
    assert(pm_lcd_last_frame_baud()==20000000);
    assert(pm_lcd_last_transfer_bytes()==25600);
    puts("different rates: control=1000000 frame=20000000 draw_reconfigs=2; control restored; frame bytes/window unchanged");
    pm_lcd_configure_bus(0,0); // Invalid requests preserve the bus state.
    assert(baud==1000000 && rate_changes==2);
    uint32_t crc=pm_lcd_fixed_test_frame();
    static uint8_t fixture[PM_IMAGE_BYTES];memcpy(fixture,captured,sizeof fixture);
    assert(pm_lcd_fixed_test_frame()==crc && !memcmp(fixture,captured,sizeof fixture));
    const unsigned uniform_rates[]={5000000,20000000,1000000};
    for(unsigned i=0;i<3;i++) {
        expected_control=expected_frame=uniform_rates[i];
        unsigned before_config=rate_changes;
        pm_lcd_configure_bus(expected_control,expected_frame);
        assert(rate_changes==before_config+1); // Explicit/ button rate changes remain.
        pm_lcd_init();
        unsigned before_draw=rate_changes;
        unsigned before_asserts=cs_asserts, before_releases=cs_releases;
        unsigned before_busy_waits=busy_waits;
        assert(pm_lcd_fixed_test_frame()==crc);
        assert(rate_changes==before_draw); // No SPI disable/re-enable during drawing.
        assert(!memcmp(fixture,captured,sizeof fixture));
        assert(pm_lcd_last_transfer_bytes()==25600 && baud==uniform_rates[i]);
        assert(pm_lcd_last_frame_baud()==uniform_rates[i]);
#if defined(PM_LCD_FRAME_CS_HOLD) && PM_LCD_FRAME_CS_HOLD
        assert(cs_asserts==before_asserts+1 && cs_releases==before_releases+1);
        assert(busy_waits==before_busy_waits+2 && busy_remaining==0);
        printf("held CS: rate=%u assert/release=1/1 CASET/RASET/RAMWR/pixels=25600B; DC correct; BSY cleared before release\n",uniform_rates[i]);
#else
        assert(cs_asserts==before_asserts+6 && cs_releases==before_releases+6);
        assert(busy_waits==before_busy_waits);
#endif
        printf("same rates: control=frame=%u draw_reconfigs=0; config change preserved; crc=%08x\n",uniform_rates[i],crc);
    }
    puts("lcd bus: commands 1MHz; complete 25600B frame 20MHz; CS idle at every rate change; pixels/window OK");
    printf("fixed frame: repeated bytes identical at 1/5/20MHz; crc=%08x\n",crc);
#if defined(PM_LCD_CAPTURE_ENABLE) && PM_LCD_CAPTURE_ENABLE
    expected_control=expected_frame=5000000;
    pm_lcd_configure_bus(expected_control,expected_frame);pm_lcd_init();
    const unsigned capture_frames[]={1,2,7};
    for(unsigned i=0;i<3;i++) {
        pm_capture_set_context(capture_frames[i],i==2?"wifi_after":"wifi_before",5000000);
        assert(pm_lcd_fixed_test_frame()==crc);
        assert(pm_capture_ready()==(i==2));
    }
    // Reuse/overwrite the original LCD buffer before export. Retained snapshots
    // must still contain the exact earlier bytes, not a pointer to this buffer.
    pm_capture_set_context(8,"wifi_after",5000000);
    pm_lcd_local("CHANGED","AFTER");
    assert(memcmp(fixture,captured,sizeof fixture));
    pm_capture_dump();
    puts("capture host: frames 1/2/7 retained after original LCD buffer overwrite");
#endif
    return 0;
}
