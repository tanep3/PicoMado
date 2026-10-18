#include "trace.h"
#include "lcd.h"
#include "lcd_transform.h"
#include <string.h>
#include "pico/stdlib.h"
#include "hardware/spi.h"
#if defined(PM_NORMAL_BUILD) && PM_NORMAL_BUILD
#include <stdio.h>
#endif
#ifdef PM_NETWORK_BUILD
#include "status_bitmaps.h"
#endif

// Verified SPI0/ST7735S wiring in PicoW_Youtube_Player/docs/wiring.md.
#define CS PM_LCD_PIN_CS
#define SCK PM_LCD_PIN_SCK
#define MOSI PM_LCD_PIN_MOSI
#define DC PM_LCD_PIN_DC
#define RES PM_LCD_PIN_RESET
static uint8_t local_pixels[PM_IMAGE_BYTES];
#ifndef PM_LCD_CONTROL_SPI_BAUD
#define PM_LCD_CONTROL_SPI_BAUD 1000000u
#endif
static uint32_t control_baud = PM_LCD_CONTROL_SPI_BAUD;
static uint32_t frame_baud = PM_LCD_SPI_BAUD;
static uint32_t last_frame_baud;
static int last_transfer_bytes;
static bool initialized;
#if PM_LCD_BLK_CONNECTED
static bool backlight_initialized;
#endif

void pm_lcd_configure_bus(uint32_t control_hz, uint32_t frame_hz) {
    // Reject a zero request rather than passing it to SDK divisor arithmetic.
    if (!control_hz || !frame_hz) return;
    control_baud = control_hz;
    frame_baud = frame_hz;
    if (initialized) spi_set_baudrate(spi0, control_baud);
}
uint32_t pm_lcd_last_frame_baud(void) { return last_frame_baud; }
int pm_lcd_last_transfer_bytes(void) { return last_transfer_bytes; }

static void command(uint8_t c) {
    gpio_put(DC, 0); gpio_put(CS, 0); spi_write_blocking(spi0, &c, 1); gpio_put(CS, 1);
}
static int data(const uint8_t *p, size_t n) {
    gpio_put(DC, 1); gpio_put(CS, 0);
    int sent = spi_write_blocking(spi0, p, n);
    gpio_put(CS, 1); return sent;
}
static void reg8(uint8_t r, uint8_t v) { command(r); data(&v, 1); }

#if defined(PM_LCD_FRAME_CS_HOLD) && PM_LCD_FRAME_CS_HOLD
// Frame-only helpers change DC and write bytes. The outer transaction owns CS.
// Initialization continues to use the independent command()/data() operations.
static void frame_command(uint8_t c) {
    gpio_put(DC, 0); spi_write_blocking(spi0, &c, 1);
}
static int frame_data(const uint8_t *p, size_t n) {
    gpio_put(DC, 1); return spi_write_blocking(spi0, p, n);
}
#endif

void pm_lcd_init(void) {
    // A lost register/window byte corrupts a whole frame. Keep control writes
    // at a conservative rate; switch to the configured frame rate only for RAM.
    spi_init(spi0, control_baud);
    spi_set_format(spi0, 8, SPI_CPOL_0, SPI_CPHA_0, SPI_MSB_FIRST);
    gpio_set_function(SCK, GPIO_FUNC_SPI); gpio_set_function(MOSI, GPIO_FUNC_SPI);
    const uint pins[] = {CS, DC, RES};
    for (unsigned i=0; i<3; i++) { gpio_init(pins[i]); gpio_set_dir(pins[i], GPIO_OUT); }
    gpio_put(CS, 1); gpio_put(DC, 1);
    gpio_put(RES, 0); sleep_ms(20); gpio_put(RES, 1); sleep_ms(120);
    command(0x01); sleep_ms(150); command(0x11); sleep_ms(120);
    reg8(0x3a, 0x05); reg8(0x36, PM_LCD_MADCTL);
    command(PM_LCD_INVERT ? 0x21 : 0x20);
    static const uint8_t positive[16] = {
        0x02,0x1c,0x07,0x12,0x37,0x32,0x29,0x2d,
        0x29,0x25,0x2b,0x39,0x00,0x01,0x03,0x10};
    static const uint8_t negative[16] = {
        0x03,0x1d,0x07,0x06,0x2e,0x2c,0x29,0x2d,
        0x2e,0x2e,0x37,0x3f,0x00,0x00,0x02,0x10};
    command(0xe0); data(positive, sizeof positive);
    command(0xe1); data(negative, sizeof negative);
    command(0x29); sleep_ms(20);
    initialized = true;
    pm_lcd_backlight(true);
#if defined(PM_NORMAL_BUILD) && PM_NORMAL_BUILD
    pm_logf("lcd ready: control_request=%lu frame_request=%lu actual=%lu frame_cs_hold=1\n",
           (unsigned long)control_baud,(unsigned long)frame_baud,
           (unsigned long)spi_get_baudrate(spi0));
#endif
}

void pm_lcd_backlight(bool on) {
#if PM_LCD_BLK_CONNECTED
    bool level=on == !!PM_LCD_BLK_ACTIVE_HIGH;
    if(!backlight_initialized){
        gpio_init(PM_LCD_BLK_PIN); // SDK selects SIO while the output is disabled.
        gpio_put(PM_LCD_BLK_PIN,level); // Preload the level before enabling drive.
        gpio_set_dir(PM_LCD_BLK_PIN,GPIO_OUT);
        backlight_initialized=true;
    }else gpio_put(PM_LCD_BLK_PIN,level);
#if defined(PM_NORMAL_BUILD) && PM_NORMAL_BUILD
    pm_logf("lcd backlight: pin=%u on=%u output=%u gpio_read=%u\n",PM_LCD_BLK_PIN,
        on?1u:0u,level?1u:0u,gpio_get(PM_LCD_BLK_PIN)?1u:0u);
#endif
#else
    (void)on;
#endif
}

static void transfer_pixels(uint8_t *pixels) {
    pm_trace_event(PM_TRACE_SPI_BEGIN,pm_trace_current_frame(),0,0);
#if defined(PM_LCD_CAPTURE_ENABLE) && PM_LCD_CAPTURE_ENABLE
    pm_lcd_capture_before_transfer(pixels, PM_IMAGE_BYTES);
#endif
    const uint8_t x[]={PM_LCD_X_OFFSET>>8,PM_LCD_X_OFFSET,
                       (PM_LCD_X_OFFSET+PM_W-1)>>8,PM_LCD_X_OFFSET+PM_W-1};
    const uint8_t y[]={PM_LCD_Y_OFFSET>>8,PM_LCD_Y_OFFSET,
                       (PM_LCD_Y_OFFSET+PM_H-1)>>8,PM_LCD_Y_OFFSET+PM_H-1};
#if defined(PM_LCD_FRAME_CS_HOLD) && PM_LCD_FRAME_CS_HOLD
    // The comparison selects equal rates throughout. Preserve the original
    // unequal-rate path rather than disable/re-enable the SPI while CS is low.
    if (control_baud == frame_baud) {
        last_frame_baud = spi_get_baudrate(spi0);
        gpio_put(DC, 0); gpio_put(CS, 0);
        frame_command(0x2a); frame_data(x, sizeof x);
        frame_command(0x2b); frame_data(y, sizeof y);
        frame_command(0x2c);
        last_transfer_bytes = frame_data(pixels, PM_IMAGE_BYTES);
        // SDK write_blocking already waits for BSY. Explicitly guard CS release
        // at the transaction boundary as well; never cut off the last pixel.
        while (spi_is_busy(spi0)) tight_loop_contents();
        gpio_put(CS, 1);
        pm_trace_event(PM_TRACE_SPI_END,pm_trace_current_frame(),last_transfer_bytes,last_frame_baud);
        return;
    }
#endif
    command(0x2a); data(x, sizeof x); command(0x2b); data(y, sizeof y);
    command(0x2c);
    // Every command/parameter transfer has completed with CS high here.
    const bool switch_baud = control_baud != frame_baud;
    last_frame_baud = switch_baud ? spi_set_baudrate(spi0, frame_baud)
                                  : spi_get_baudrate(spi0);
    last_transfer_bytes = data(pixels, PM_IMAGE_BYTES);
    if (switch_baud) spi_set_baudrate(spi0, control_baud);
    pm_trace_event(PM_TRACE_SPI_END,pm_trace_current_frame(),last_transfer_bytes,last_frame_baud);
}
void pm_lcd_draw(uint8_t *pixels) {
    uint64_t started=pm_stats_clock();
    pm_lcd_transform(pixels,PM_W*PM_H,
                     PM_LCD_ROTATE_180,PM_LCD_COMPENSATE_INVERT_BGR);
    transfer_pixels(pixels);
    pm_stats_note(PM_STAT_DRAW,started);
}

static void dot(unsigned x, unsigned y, uint16_t c) {
    if (x>=PM_W || y>=PM_H) return;
    size_t p=2*(y*PM_W+x); local_pixels[p]=c>>8; local_pixels[p+1]=c;
}
// Exact white border + horizontal RGB bands used by the working YouTube
// pico_player.c show_test_pattern(), sent through the same LCD transfer path.
void pm_lcd_reference_pattern(void) {
    for (unsigned y=0;y<PM_H;y++) for (unsigned x=0;x<PM_W;x++) {
        uint16_t c=(x==0 || y==0 || x==PM_W-1 || y==PM_H-1)
            ? 0xffff : (y<27 ? 0xf800 : (y<54 ? 0x07e0 : 0x001f));
        dot(x,y,c);
    }
    // The working YouTube boot pattern is sent without sender compensation.
    // Keep its exact SPI payload; the regular image path still compensates.
    pm_lcd_transform(local_pixels,PM_W*PM_H,PM_LCD_ROTATE_180,false);
    transfer_pixels(local_pixels);
}
void pm_lcd_diagnostic(void) {
    for(unsigned y=0;y<PM_H;y++)for(unsigned x=0;x<PM_W;x++){
        uint16_t color=x<PM_W/4?0xf800:x<PM_W/2?0x07e0:
                       x<3*PM_W/4?0x001f:0xffff;
        dot(x,y,color);
    }
    pm_lcd_draw(local_pixels);
}

// Compact uppercase 5x7 font for local connection/settings screens.
static const uint8_t glyphs[][5] = {
    {0x7e,0x11,0x11,0x11,0x7e}, // A
    {0x7f,0x49,0x49,0x49,0x36}, // B
    {0x3e,0x41,0x41,0x41,0x22}, // C
    {0x7f,0x41,0x41,0x22,0x1c}, // D
    {0x7f,0x49,0x49,0x49,0x41}, // E
    {0x7f,0x09,0x09,0x09,0x01}, // F
    {0x3e,0x41,0x49,0x49,0x7a}, // G
    {0x7f,0x08,0x08,0x08,0x7f}, // H
    {0x41,0x41,0x7f,0x41,0x41}, // I
    {0x20,0x40,0x41,0x3f,0x01}, // J
    {0x7f,0x08,0x14,0x22,0x41}, // K
    {0x7f,0x40,0x40,0x40,0x40}, // L
    {0x7f,0x02,0x0c,0x02,0x7f}, // M
    {0x7f,0x04,0x08,0x10,0x7f}, // N
    {0x3e,0x41,0x41,0x41,0x3e}, // O
    {0x7f,0x09,0x09,0x09,0x06}, // P
    {0x3e,0x41,0x51,0x21,0x5e}, // Q
    {0x7f,0x09,0x19,0x29,0x46}, // R
    {0x46,0x49,0x49,0x49,0x31}, // S
    {0x01,0x01,0x7f,0x01,0x01}, // T
    {0x3f,0x40,0x40,0x40,0x3f}, // U
    {0x1f,0x20,0x40,0x20,0x1f}, // V
    {0x7f,0x20,0x18,0x20,0x7f}, // W
    {0x63,0x14,0x08,0x14,0x63}, // X
    {0x03,0x04,0x78,0x04,0x03}, // Y
    {0x61,0x51,0x49,0x45,0x43}, // Z
};
static void label(const char *s, unsigned y, uint16_t color) {
    size_t n=strlen(s); if (n>13) n=13;
    unsigned x=(PM_W-(unsigned)n*12)/2;
    for (size_t i=0;i<n;i++,x+=12) {
        if (s[i]<'A'||s[i]>'Z') continue;
        for (unsigned col=0;col<5;col++) for (unsigned row=0;row<7;row++)
            if (glyphs[s[i]-'A'][col] & (1u<<row))
                for (unsigned a=0;a<2;a++) for (unsigned b=0;b<2;b++)
                    dot(x+2*col+a,2*y+2*row+b,color);
    }
}
void pm_lcd_local(const char *one, const char *two) {
    memset(local_pixels,0,sizeof local_pixels);
#ifdef PM_NETWORK_BUILD
    const uint8_t *bitmap=NULL;
    if (!strcmp(one,"WIFI") && (!strcmp(two,"CONNECTING") || !strcmp(two,"RETRY")))
        bitmap=pm_status_wifi;
    else if (!strcmp(one,"SERVER") && !strcmp(two,"CONNECTING"))
        bitmap=pm_status_server;
    if (bitmap) {
        for(unsigned y=0;y<PM_STATUS_H;y++) for(unsigned x=0;x<PM_STATUS_W;x++)
            if(bitmap[y*(PM_STATUS_W/8)+x/8] & (0x80u>>(x&7)))dot(x,y+28,0xffff);
        pm_lcd_draw(local_pixels);return;
    }
#endif
    label(one,10,0xffff); label(two,28,0x07ff);
    pm_lcd_draw(local_pixels);
}
void pm_lcd_black(void) { memset(local_pixels,0,sizeof local_pixels); pm_lcd_draw(local_pixels); }

// Rebuild the identical complete frame before each diagnostic transfer.
// No phase label/counter is drawn: Wi-Fi and bus-rate comparisons keep pixels
// unchanged, including the black fill that must replace previous LCD contents.
uint32_t pm_lcd_fixed_test_frame(void) {
    memset(local_pixels,0,sizeof local_pixels);
    for (unsigned y=0;y<PM_H;y++) for (unsigned x=0;x<PM_W;x++) {
        if (x==0 || x==PM_W-1 || y==0 || y==PM_H-1) dot(x,y,0xffff);
        else if (y<10) dot(x,y,x<54?0xf800:(x<107?0x07e0:0x001f));
    }
    label("PICO MADO",10,0xffff);
    label("FIXED FRAME",28,0x07ff);
    pm_lcd_draw(local_pixels);
    return pm_crc32(local_pixels,sizeof local_pixels);
}
