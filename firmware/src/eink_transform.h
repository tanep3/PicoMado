#pragma once
#include <stddef.h>
#include <stdint.h>
#include <string.h>

// Match BOTH stages of the legacy path: png_to_wire builds W[x*16+u/8],
// u=121-y; Panel.display sends landscape_buffer by j=15..0, x=0..249.
// Final SPI order is W[x*16+byte] for byte=0..15 then x=0..249.
// Each byte plane is 250B; low six bits in plane15 remain white.
#define PM_EINK_RAM_BYTES 4000
static inline void pm_eink_pack_landscape(const uint8_t *mono,uint8_t *panel){
    memset(panel,0xff,PM_EINK_RAM_BYTES);
    for(unsigned y=0;y<122;y++)for(unsigned x=0;x<250;x++){
        unsigned bit=y*250+x,u=121-y;
        if(!(mono[bit/8] & (0x80u>>(bit&7))))
            panel[(u/8)*250+x]&=(uint8_t)~(0x80u>>(u&7));
    }
}
