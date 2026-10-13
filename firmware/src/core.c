#include "core.h"
#include <string.h>

uint32_t pm_crc32(const uint8_t *bytes, size_t len) {
    uint32_t crc = 0xffffffffu;
    for (size_t i = 0; i < len; ++i) {
        crc ^= bytes[i];
        for (unsigned b = 0; b < 8; ++b)
            crc = (crc >> 1) ^ (0xedb88320u & (0u - (crc & 1u)));
    }
    return ~crc;
}

void pm_reset(pm_assembly *a) { a->active = false; a->used = 0; }

bool pm_token_equal(pm_token a, pm_token b) {
    return a.incarnation == b.incarnation && a.session_epoch == b.session_epoch &&
           a.display_epoch == b.display_epoch && a.screen_owner_epoch == b.screen_owner_epoch &&
           a.frame_id == b.frame_id;
}

bool pm_token_current(pm_token t, uint64_t i, uint64_t s, uint64_t d, uint64_t o) {
    return t.incarnation == i && t.session_epoch == s &&
           t.display_epoch == d && t.screen_owner_epoch == o;
}

bool pm_start(pm_assembly *a, pm_token token, unsigned width, unsigned height,
              const char *format, uint32_t length, uint32_t crc) {
    pm_reset(a);
    if (width != PM_W || height != PM_H || strcmp(format, PM_PIXEL_FORMAT_NAME) ||
        length != PM_IMAGE_BYTES) return false;
    a->token = token; a->expected = length; a->crc = crc; a->active = true;
    return true;
}

bool pm_chunk(pm_assembly *a, pm_token token, uint32_t offset,
              const uint8_t *bytes, size_t length, bool *complete) {
    *complete = false;
    if (!a->active || !pm_token_equal(a->token, token) || offset != a->used ||
        !length || length > PM_FRAGMENT_BYTES || length > a->expected - a->used) {
        pm_reset(a); return false;
    }
    memcpy(a->pixels + a->used, bytes, length);
    a->used += length;
    if (a->used == a->expected) {
        if (pm_crc32(a->pixels, a->used) != a->crc) { pm_reset(a); return false; }
        *complete = true;
        a->active = false;
    }
    return true;
}

bool pm_button_step(pm_button *b, bool pressed, uint32_t now_ms) {
    if (pressed != b->raw) { b->raw = pressed; b->changed_ms = now_ms; }
    if (pressed != b->stable && (uint32_t)(now_ms - b->changed_ms) >= 35) {
        b->stable = pressed;
        return pressed;
    }
    return false;
}
