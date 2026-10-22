#pragma once
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include "core.h"

// The app owns connectivity, frame generations and buttons. The selected
// backend owns panel commands, pixels, local status and physical standby.
const char *pm_display_capability(void);
void pm_display_init(void);
void pm_display_status(const char *line1, const char *line2);
bool pm_display_frame(uint8_t *pixels, size_t length);
void pm_display_standby(void);
void pm_display_wake(void);
void pm_display_reference_pattern(void);

/* Local presentation only: never creates remote receipt/rendered results.
 * REPLACE cancels unstarted hint work when the real image is ready; RESTORE
 * is for an acknowledged no-op and restores the preceding display if needed. */
typedef enum { PM_DISPLAY_FEEDBACK_SHOW, PM_DISPLAY_FEEDBACK_REPLACE,
               PM_DISPLAY_FEEDBACK_RESTORE } pm_display_feedback_action;
void pm_display_feedback(pm_display_feedback_action action);

// E-ink advances one bounded stage per main-loop poll; no BUSY sleep loop.
void pm_display_poll(void);
bool pm_display_busy(void);
bool pm_display_take_result(bool *succeeded);

#if PM_DISPLAY_TYPE == PM_DISPLAY_EINK
// Clock-only opt-in; all ordinary plugin/local frames remain full refreshes.
bool pm_display_clock_frame(uint8_t *pixels, size_t length, unsigned minutes,
                            uint64_t minute, bool reset_base);
#endif
