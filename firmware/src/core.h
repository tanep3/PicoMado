#pragma once
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#ifdef PM_USE_SAMPLE_CONFIG
#include "config.sample.h"
#else
#include "config.h"
#endif

#define PM_W PM_DISPLAY_WIDTH
#define PM_H PM_DISPLAY_HEIGHT
#define PM_IMAGE_BYTES (PM_W * PM_H * PM_PIXEL_BYTES)
#define PM_FRAGMENT_BYTES 4096

typedef struct {
    uint64_t incarnation, session_epoch, display_epoch, screen_owner_epoch, frame_id;
} pm_token;

typedef struct {
    pm_token token;
    uint32_t expected, used, crc;
    bool active;
    uint8_t pixels[PM_IMAGE_BYTES];
} pm_assembly;

uint32_t pm_crc32(const uint8_t *bytes, size_t len);
void pm_reset(pm_assembly *a);
bool pm_start(pm_assembly *a, pm_token token, unsigned width, unsigned height,
              const char *format, uint32_t length, uint32_t crc);
bool pm_chunk(pm_assembly *a, pm_token token, uint32_t offset,
              const uint8_t *bytes, size_t length, bool *complete);
bool pm_token_equal(pm_token a, pm_token b);
bool pm_token_current(pm_token token, uint64_t incarnation, uint64_t session,
                      uint64_t display, uint64_t owner);

typedef struct { bool raw, stable; uint32_t changed_ms; uint32_t held_ms; bool wake_reported, wake_armed; } pm_button;
bool pm_button_step(pm_button *button, bool pressed, uint32_t now_ms);

// Four-button POWER index2 wakes on the existing debounced press.
// Two-button DOWN index1 wakes once after the configured long hold.
bool pm_button_wakes(unsigned count,unsigned index,bool long_press);
bool pm_button_wake_step(pm_button *button,unsigned count,unsigned index,
                         bool pressed,uint32_t now_ms,uint32_t long_ms);
