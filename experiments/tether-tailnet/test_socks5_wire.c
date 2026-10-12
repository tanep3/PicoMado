#include <assert.h>
#include <stdio.h>
#include <string.h>
#include "socks5_wire.h"

int main(void) {
    uint8_t auth[513];
    size_t length = 0;
    assert(socks5_make_auth("u", "p", auth, sizeof auth, &length));
    const uint8_t expected_auth[] = {1, 1, 'u', 1, 'p'};
    assert(length == sizeof expected_auth);
    assert(memcmp(auth, expected_auth, length) == 0);
    assert(!socks5_make_auth("", "p", auth, sizeof auth, &length));
    assert(!socks5_make_auth("u", "", auth, sizeof auth, &length));
    char max_name[256], overlong[257];
    memset(max_name, 'u', 255); max_name[255] = 0;
    memset(overlong, 'u', 256); overlong[256] = 0;
    assert(socks5_make_auth(max_name, max_name, auth, sizeof auth, &length));
    assert(length == 513);
    assert(!socks5_make_auth(max_name, max_name, auth, 512, &length));
    assert(!socks5_make_auth(overlong, "p", auth, sizeof auth, &length));

    const uint8_t pi_ipv4[4] = {100, 64, 0, 9};
    uint8_t connect[10];
    socks5_make_connect_ipv4(pi_ipv4, 18182, connect);
    const uint8_t expected_connect[10] = {5, 1, 0, 1, 100, 64, 0, 9, 0x47, 0x06};
    assert(memcmp(connect, expected_connect, sizeof connect) == 0);

    const uint8_t method[] = {5, 2};
    assert(socks5_method_result(method, 1) == SOCKS5_INCOMPLETE);
    assert(socks5_method_result(method, 2) == SOCKS5_OK);
    const uint8_t no_auth[] = {5, 0};
    const uint8_t bad_version[] = {4, 2};
    assert(socks5_method_result(no_auth, 2) == SOCKS5_REJECTED);
    assert(socks5_method_result(bad_version, 2) == SOCKS5_MALFORMED);

    const uint8_t auth_ok[] = {1, 0}, auth_rejected[] = {1, 1};
    assert(socks5_auth_result(auth_ok, 1) == SOCKS5_INCOMPLETE);
    assert(socks5_auth_result(auth_ok, 2) == SOCKS5_OK);
    assert(socks5_auth_result(auth_rejected, 2) == SOCKS5_REJECTED);

    const uint8_t reply_v4[10] = {5, 0, 0, 1};
    size_t consumed = 99;
    for (size_t n = 0; n < sizeof reply_v4; n++) {
        assert(socks5_connect_result(reply_v4, n, &consumed) == SOCKS5_INCOMPLETE);
        assert(consumed == 99);
    }
    assert(socks5_connect_result(reply_v4, sizeof reply_v4, &consumed) == SOCKS5_OK);
    assert(consumed == 10);

    const uint8_t reply_domain[10] = {5, 0, 0, 3, 3, 'p', 'i', '5', 0, 0};
    assert(socks5_connect_result(reply_domain, 4, &consumed) == SOCKS5_INCOMPLETE);
    assert(socks5_connect_result(reply_domain, 9, &consumed) == SOCKS5_INCOMPLETE);
    assert(socks5_connect_result(reply_domain, 10, &consumed) == SOCKS5_OK);
    assert(consumed == 10);
    uint8_t reply_v6[22] = {5, 0, 0, 4};
    assert(socks5_connect_result(reply_v6, 21, &consumed) == SOCKS5_INCOMPLETE);
    assert(socks5_connect_result(reply_v6, 22, &consumed) == SOCKS5_OK);
    assert(consumed == 22);

    const uint8_t rejected[] = {5, 5, 0, 1};
    const uint8_t malformed[] = {5, 0, 1, 1};
    const uint8_t unknown_type[] = {5, 0, 0, 9};
    const uint8_t empty_domain[] = {5, 0, 0, 3, 0};
    assert(socks5_connect_result(rejected, 4, &consumed) == SOCKS5_REJECTED);
    assert(socks5_connect_result(malformed, 4, &consumed) == SOCKS5_MALFORMED);
    assert(socks5_connect_result(unknown_type, 4, &consumed) == SOCKS5_MALFORMED);
    assert(socks5_connect_result(empty_domain, 5, &consumed) == SOCKS5_MALFORMED);

    puts("SOCKS5 wire tests passed");
}
