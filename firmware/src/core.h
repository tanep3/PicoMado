#pragma once
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include "input_types.h"
#ifdef PM_USE_SAMPLE_CONFIG
#include "config.sample.h"
#else
#include "config.h"
#endif

#define PM_W PM_DISPLAY_WIDTH
#define PM_H PM_DISPLAY_HEIGHT
// mono1 is a continuous MSB-first, white=1 bitstream (no row padding).
#if PM_PIXEL_FORMAT == PM_PIXEL_MONO1
#define PM_IMAGE_BYTES ((PM_W * PM_H + 7) / 8)
#else
#define PM_IMAGE_BYTES (PM_W * PM_H * PM_PIXEL_BYTES)
#endif
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
bool pm_frame_header_valid(uint64_t width,uint64_t height,const char *format,
                           uint64_t length,uint64_t crc);
bool pm_start(pm_assembly *a, pm_token token, unsigned width, unsigned height,
              const char *format, uint32_t length, uint32_t crc);
bool pm_chunk(pm_assembly *a, pm_token token, uint32_t offset,
              const uint8_t *bytes, size_t length, bool *complete);
bool pm_token_equal(pm_token a, pm_token b);
bool pm_token_current(pm_token token, uint64_t incarnation, uint64_t session,
                      uint64_t display, uint64_t owner);

#define PM_BUTTON_DEBOUNCE_MS 35u
#define PM_BUTTON_MAX_SAMPLE_GAP_MS PM_BUTTON_DEBOUNCE_MS
typedef struct {
    bool raw, stable;
    uint32_t changed_ms, held_ms;
    bool wake_reported, wake_armed;
    uint32_t press_long_ms;
    bool long_reported;
    uint32_t sampled_ms;
    bool sample_seen, release_required;
} pm_button;
bool pm_button_step(pm_button *button, bool pressed, uint32_t now_ms);

// GPIO gesture bindings are data. The processor works for any layout length.
typedef struct {
    pm_operation short_operation,long_operation;
    bool on_press,short_wakes,long_wakes;
} pm_button_binding;
const pm_button_binding *pm_button_binding_at(unsigned count,unsigned index);
/* Physical samples: cancel across untrusted gaps, then require a released
 * debounce before rearming. The virtual signal entry below retains its owner
 * supplied continuity (diagnostic SIM); both use the same gesture FSM. */
bool pm_button_sampled_binding_step(pm_button *button,const pm_button_binding *binding,
    bool pressed,uint32_t now_ms,uint32_t long_ms,bool standby,
    pm_operation *operation,bool *long_press);
bool pm_button_binding_step(pm_button *button,const pm_button_binding *binding,
    bool pressed,uint32_t now_ms,uint32_t long_ms,bool standby,
    pm_operation *operation,bool *long_press);
bool pm_button_event_is_long(unsigned count,unsigned index,unsigned logical);


// Four-button POWER index2 wakes on the existing debounced press.
// Two-button DOWN index1 wakes once after the configured long hold.
bool pm_button_wakes(unsigned count,unsigned index,bool long_press);
bool pm_button_wake_step(pm_button *button,unsigned count,unsigned index,
                         bool pressed,uint32_t now_ms,uint32_t long_ms);

// Logical indices match the four-button contract: SUB, UP, POWER, DOWN.
// Returns -1 for no event. Two-button short events occur on stable release.
int pm_button_input_step(pm_button *button, unsigned count, unsigned index,
                         bool pressed, uint32_t now_ms, uint32_t long_ms,
                         bool standby);
