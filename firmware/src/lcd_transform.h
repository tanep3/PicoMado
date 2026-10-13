#pragma once
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

// The verified YouTube sender precompensates this ST7735S panel with exactly
// this inverse + red/blue swap. PicoMado sends canonical RGB565BE instead,
// so apply the same panel transform immediately before SPI.
static inline uint16_t pm_panel_compensate(uint16_t color) {
    color ^= 0xffff;
    return (uint16_t)(((color & 0x001f) << 11) |
                      (color & 0x07e0) | (color >> 11));
}

static inline void pm_lcd_transform(uint8_t *pixels, size_t count,
                                    bool rotate180, bool compensate) {
    if(compensate)for(size_t i=0;i<count;i++) {
        uint16_t c=(uint16_t)pixels[2*i]<<8|pixels[2*i+1];
        c=pm_panel_compensate(c);
        pixels[2*i]=(uint8_t)(c>>8);pixels[2*i+1]=(uint8_t)c;
    }
    if(rotate180 && count>1)for(size_t left=0,right=count-1;left<right;left++,right--) {
        uint8_t hi=pixels[2*left],lo=pixels[2*left+1];
        pixels[2*left]=pixels[2*right];pixels[2*left+1]=pixels[2*right+1];
        pixels[2*right]=hi;pixels[2*right+1]=lo;
    }
}
