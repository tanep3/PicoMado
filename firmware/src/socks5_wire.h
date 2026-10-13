#ifndef PICOMADO_SOCKS5_WIRE_H
#define PICOMADO_SOCKS5_WIRE_H

#include <stddef.h>
#include <stdint.h>
#include <string.h>

enum {
    SOCKS5_INCOMPLETE = 0,
    SOCKS5_OK = 1,
    SOCKS5_REJECTED = -1,
    SOCKS5_MALFORMED = -2,
};

// RFC 1929 username/password request. The caller must never log the bytes.
static inline int socks5_make_auth(const char *user, const char *password,
                                   uint8_t *out, size_t cap, size_t *length) {
    size_t ulen = strlen(user), plen = strlen(password);
    size_t required = 3 + ulen + plen;
    if (!ulen || !plen || ulen > 255 || plen > 255 || cap < required) return 0;
    out[0] = 0x01;
    out[1] = (uint8_t)ulen;
    memcpy(out + 2, user, ulen);
    out[2 + ulen] = (uint8_t)plen;
    memcpy(out + 3 + ulen, password, plen);
    *length = required;
    return 1;
}

static inline void socks5_make_connect_ipv4(const uint8_t ipv4[4], uint16_t port,
                                            uint8_t out[10]) {
    const uint8_t prefix[4] = {0x05, 0x01, 0x00, 0x01};
    memcpy(out, prefix, 4);
    memcpy(out + 4, ipv4, 4);
    out[8] = (uint8_t)(port >> 8);
    out[9] = (uint8_t)port;
}

static inline int socks5_method_result(const uint8_t *buf, size_t n) {
    if (n < 2) return SOCKS5_INCOMPLETE;
    if (buf[0] != 0x05) return SOCKS5_MALFORMED;
    return buf[1] == 0x02 ? SOCKS5_OK : SOCKS5_REJECTED;
}

static inline int socks5_auth_result(const uint8_t *buf, size_t n) {
    if (n < 2) return SOCKS5_INCOMPLETE;
    if (buf[0] != 0x01) return SOCKS5_MALFORMED;
    return buf[1] == 0x00 ? SOCKS5_OK : SOCKS5_REJECTED;
}

// CONNECT replies have variable-length BND.ADDR. Report rejection as soon as
// its four-byte header arrives, even if the proxy closes without an address.
static inline int socks5_connect_result(const uint8_t *buf, size_t n,
                                        size_t *consumed) {
    if (n < 4) return SOCKS5_INCOMPLETE;
    if (buf[0] != 0x05 || buf[2] != 0x00) return SOCKS5_MALFORMED;
    if (buf[1] != 0x00) return SOCKS5_REJECTED;
    size_t required;
    if (buf[3] == 0x01) required = 10;
    else if (buf[3] == 0x04) required = 22;
    else if (buf[3] == 0x03) {
        if (n < 5) return SOCKS5_INCOMPLETE;
        if (buf[4] == 0) return SOCKS5_MALFORMED;
        required = 7 + buf[4];
    } else return SOCKS5_MALFORMED;
    if (n < required) return SOCKS5_INCOMPLETE;
    *consumed = required;
    return SOCKS5_OK;
}

#endif
