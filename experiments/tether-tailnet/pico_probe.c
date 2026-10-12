#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>
#include <string.h>
#include "pico/stdlib.h"
#include "pico/cyw43_arch.h"
#include "lwip/ip4_addr.h"
#include "lwip/netif.h"
#include "lwip/tcp.h"
#include "socks5_wire.h"

#include "test_config.h"

#ifndef PROBE_USE_SOCKS5
#define PROBE_USE_SOCKS5 1
#endif
#ifndef SOCKS5_PROXY_IP
#define SOCKS5_PROXY_IP "10.101.65.232"
#endif
#ifndef SOCKS5_PROXY_PORT
#define SOCKS5_PROXY_PORT 48115
#endif
#ifndef SOCKS5_USERNAME
#define SOCKS5_USERNAME ""
#endif
#ifndef SOCKS5_PASSWORD
#define SOCKS5_PASSWORD ""
#endif

#define PORT_A 18180
#define PORT_B 18181
#define PORT_C 18182
#define RETRY_MS 15000u
#define STATUS_MS 10000u
#define MAX_ATTEMPTS 3u
#define PROBE_BUILD_ID "socks5-20261011-1"
#define SOCKS_STAGE_TIMEOUT_MS 12000u
#define APP_TIMEOUT_MS 20000u

typedef enum {
    ST_TCP_CONNECT,
    ST_SOCKS_METHOD,
    ST_SOCKS_AUTH,
    ST_SOCKS_CONNECT,
    ST_APPLICATION,
} stage_t;

typedef struct {
    struct tcp_pcb *pcb;
    char rx[512];
    size_t used;
    uint32_t last_try;
    uint32_t stage_since;
    uint16_t target_port;
    unsigned attempts;
    bool done;
    bool connected;
    char kind;
    stage_t stage;
} probe_conn;

static probe_conn a = {.kind = 'A'};
static probe_conn b = {.kind = 'B'};
static probe_conn c = {.kind = 'C'};
static struct tcp_pcb *b_listener;
static ip_addr_t pi_addr;
static ip_addr_t proxy_addr;

static const char *stage_name(stage_t stage) {
    switch (stage) {
    case ST_TCP_CONNECT: return "TCP connect";
    case ST_SOCKS_METHOD: return "SOCKS method";
    case ST_SOCKS_AUTH: return "SOCKS authentication";
    case ST_SOCKS_CONNECT: return "SOCKS CONNECT";
    case ST_APPLICATION: return "application";
    default: return "unknown";
    }
}

static void set_stage(probe_conn *s, stage_t stage) {
    s->stage = stage;
    s->stage_since = to_ms_since_boot(get_absolute_time());
}

static void drop(probe_conn *s) {
    struct tcp_pcb *pcb = s->pcb;
    s->pcb = NULL;
    s->used = 0;
    s->connected = false;
    if (!pcb) return;
    tcp_arg(pcb, NULL);
    tcp_recv(pcb, NULL);
    tcp_err(pcb, NULL);
    if (tcp_close(pcb) != ERR_OK) tcp_abort(pcb);
}

static err_t send_text(struct tcp_pcb *pcb, const char *msg) {
    err_t err = tcp_write(pcb, msg, strlen(msg), TCP_WRITE_FLAG_COPY);
    if (err == ERR_OK) err = tcp_output(pcb);
    return err;
}

static err_t send_bytes(struct tcp_pcb *pcb, const uint8_t *msg, size_t len) {
    err_t err = tcp_write(pcb, msg, len, TCP_WRITE_FLAG_COPY);
    if (err == ERR_OK) err = tcp_output(pcb);
    return err;
}

static void on_error(void *arg, err_t err) {
    probe_conn *s = arg;
    const char *stage = stage_name(s->stage);
    s->pcb = NULL; // lwIP has already freed this PCB.
    s->used = 0;
    s->connected = false;
    printf("%c stage=%s error=%d%s attempt=%u/%u id=%s\n", s->kind, stage,
           err, err == ERR_ABRT ? " (ERR_ABRT)" : "", s->attempts, MAX_ATTEMPTS, TEST_ID);
}

static const char *link_name(int status) {
    switch (status) {
    case CYW43_LINK_DOWN: return "DOWN";
    case CYW43_LINK_JOIN: return "JOIN";
    case CYW43_LINK_NOIP: return "NOIP";
    case CYW43_LINK_UP: return "UP";
    case CYW43_LINK_FAIL: return "FAIL";
    case CYW43_LINK_NONET: return "NONET";
    case CYW43_LINK_BADAUTH: return "BADAUTH";
    default: return "UNKNOWN";
    }
}

static void print_status(void) {
    int link = cyw43_tcpip_link_status(&cyw43_state, CYW43_ITF_STA);
    char ip[20] = "unavailable", mask[20] = "unavailable", gw[20] = "unavailable";
    if (netif_default) {
        ip4addr_ntoa_r(netif_ip4_addr(netif_default), ip, sizeof ip);
        ip4addr_ntoa_r(netif_ip4_netmask(netif_default), mask, sizeof mask);
        ip4addr_ntoa_r(netif_ip4_gw(netif_default), gw, sizeof gw);
    }
    printf("build=%s mode=%s uptime_ms=%lu link=%s(%d) pico_ipv4=%s netmask=%s gateway=%s pi_ipv4=%s\n",
           PROBE_BUILD_ID, PROBE_USE_SOCKS5 ? "SOCKS5" : "direct",
           (unsigned long)to_ms_since_boot(get_absolute_time()),
           link_name(link), link, ip, mask, gw, PI_TAILSCALE_IP);
}

static bool line_id(const char *line, const char *prefix, char *id, size_t cap) {
    size_t n = strlen(prefix);
    if (strncmp(line, prefix, n)) return false;
    const char *end = strchr(line + n, '\n');
    if (!end) return false;
    size_t len = (size_t)(end - (line + n));
    if (len && line[n + len - 1] == '\r') len--;
    if (!len || len >= cap) return false;
    memcpy(id, line + n, len);
    id[len] = 0;
    return true;
}

static void consume_rx(probe_conn *s, size_t count) {
    s->used -= count;
    memmove(s->rx, s->rx + count, s->used);
    s->rx[s->used] = 0;
}

static bool send_application(probe_conn *s) {
    char msg[256];
    if (s == &a) {
        char body[96];
        snprintf(body, sizeof body, "TEST A %s\n", TEST_ID);
        snprintf(msg, sizeof msg,
                 "POST /probe HTTP/1.1\r\nHost: pi5\r\nContent-Type: text/plain\r\n"
                 "Content-Length: %u\r\nConnection: close\r\n\r\n%s",
                 (unsigned)strlen(body), body);
    } else {
        snprintf(msg, sizeof msg, "HELLO C %s\n", TEST_ID);
    }
    set_stage(s, ST_APPLICATION);
    if (send_text(s->pcb, msg) != ERR_OK) {
        printf("%c application send failed, id=%s\n", s->kind, TEST_ID);
        drop(s);
        return false;
    }
    printf("%c application sent, id=%s\n", s->kind, TEST_ID);
    return true;
}

static bool send_socks_auth(probe_conn *s) {
    uint8_t req[513];
    size_t n = 0;
    if (!socks5_make_auth(SOCKS5_USERNAME, SOCKS5_PASSWORD, req, sizeof req, &n)) {
        printf("%c SOCKS authentication settings invalid\n", s->kind);
        drop(s);
        return false;
    }
    set_stage(s, ST_SOCKS_AUTH);
    if (send_bytes(s->pcb, req, n) != ERR_OK) {
        printf("%c SOCKS authentication send failed\n", s->kind);
        drop(s);
        return false;
    }
    printf("%c SOCKS authentication request sent (credentials hidden)\n", s->kind);
    return true;
}

static bool send_socks_connect(probe_conn *s) {
    uint8_t req[10];
    socks5_make_connect_ipv4((const uint8_t *)&ip_2_ip4(&pi_addr)->addr,
                             s->target_port, req);
    set_stage(s, ST_SOCKS_CONNECT);
    if (send_bytes(s->pcb, req, sizeof req) != ERR_OK) {
        printf("%c SOCKS CONNECT send failed\n", s->kind);
        drop(s);
        return false;
    }
    printf("%c SOCKS CONNECT requested Pi5 %s:%u\n", s->kind, PI_TAILSCALE_IP, s->target_port);
    return true;
}

// Returns true once the SOCKS tunnel is established. Replies may be split
// across pbufs, or followed by application bytes in the same pbuf.
static bool process_socks(probe_conn *s) {
    while (s->stage != ST_APPLICATION) {
        const uint8_t *rx = (const uint8_t *)s->rx;
        if (s->stage == ST_SOCKS_METHOD) {
            int result = socks5_method_result(rx, s->used);
            if (result == SOCKS5_INCOMPLETE) return false;
            if (result != SOCKS5_OK) {
                printf("%c SOCKS method rejected, version=%u method=%u\n", s->kind, rx[0], rx[1]);
                drop(s);
                return false;
            }
            consume_rx(s, 2);
            if (!send_socks_auth(s)) return false;
        } else if (s->stage == ST_SOCKS_AUTH) {
            int result = socks5_auth_result(rx, s->used);
            if (result == SOCKS5_INCOMPLETE) return false;
            if (result != SOCKS5_OK) {
                printf("%c SOCKS authentication rejected, version=%u status=%u\n", s->kind, rx[0], rx[1]);
                drop(s);
                return false;
            }
            consume_rx(s, 2);
            printf("%c SOCKS authentication accepted\n", s->kind);
            if (!send_socks_connect(s)) return false;
        } else if (s->stage == ST_SOCKS_CONNECT) {
            size_t length = 0;
            int result = socks5_connect_result(rx, s->used, &length);
            if (result == SOCKS5_INCOMPLETE) return false;
            if (result == SOCKS5_REJECTED) {
                printf("%c SOCKS CONNECT rejected, reply=%u\n", s->kind, rx[1]);
                drop(s);
                return false;
            }
            if (result != SOCKS5_OK) {
                printf("%c SOCKS CONNECT malformed response\n", s->kind);
                drop(s);
                return false;
            }
            consume_rx(s, length);
            printf("%c SOCKS tunnel established to Pi5, id=%s\n", s->kind, TEST_ID);
            if (!send_application(s)) return false;
        } else {
            printf("%c unexpected SOCKS stage\n", s->kind);
            drop(s);
            return false;
        }
    }
    return true;
}

static err_t on_recv(void *arg, struct tcp_pcb *pcb, struct pbuf *p, err_t err) {
    probe_conn *s = arg;
    if (!p) {
        if (s->stage != ST_APPLICATION)
            printf("%c TCP closed at stage=%s, id=%s\n", s->kind, stage_name(s->stage), TEST_ID);
        else if (s == &a && !s->done)
            printf("A TCP closed without matching HTTP 200 and ACK, id=%s\n", TEST_ID);
        else if (s == &c && !s->done)
            printf("C TCP closed before PUSH/ACK, id=%s\n", TEST_ID);
        drop(s);
        return ERR_OK;
    }
    if (err != ERR_OK || p->tot_len >= sizeof(s->rx) - s->used) {
        printf("%c receive error=%d or buffer full at stage=%s\n", s->kind, err, stage_name(s->stage));
        pbuf_free(p);
        drop(s);
        return ERR_OK;
    }
    pbuf_copy_partial(p, s->rx + s->used, p->tot_len, 0);
    s->used += p->tot_len;
    s->rx[s->used] = 0;
    tcp_recved(pcb, p->tot_len);
    pbuf_free(p);

    if (PROBE_USE_SOCKS5 && s != &b && s->stage != ST_APPLICATION) {
        if (!process_socks(s)) return ERR_OK;
    }
    if (!s->used) return ERR_OK;

    char expected[96];
    snprintf(expected, sizeof expected, "ACK A %s", TEST_ID);
    if (s == &a && strstr(s->rx, "HTTP/1.1 200 OK") && strstr(s->rx, expected)) {
        printf("A OK: HTTP POST acknowledged, id=%s\n", TEST_ID);
        s->done = true;
        drop(s);
    } else if (s == &b) {
        char id[64], reply[96];
        if (line_id(s->rx, "PUSH B ", id, sizeof id)) {
            snprintf(reply, sizeof reply, "ACK B %s\n", id);
            if (send_text(pcb, reply) == ERR_OK) printf("B PUSH received and ACK sent, id=%s\n", id);
            else printf("B ACK send failed, id=%s\n", id);
            drop(s);
        }
    } else if (s == &c) {
        char id[64], reply[96];
        if (line_id(s->rx, "PUSH C ", id, sizeof id) && strcmp(id, TEST_ID) == 0) {
            snprintf(reply, sizeof reply, "ACK C %s\n", id);
            if (send_text(pcb, reply) == ERR_OK) {
                printf("C PUSH received and ACK sent, id=%s\n", id);
                s->done = true;
            } else printf("C ACK send failed, id=%s\n", id);
            // Let the Pi read the ACK and close the persistent connection.
        }
    }
    return ERR_OK;
}

static err_t on_connected(void *arg, struct tcp_pcb *pcb, err_t err) {
    probe_conn *s = arg;
    if (err != ERR_OK) return err;
    s->connected = true;
    printf("%c TCP connected to %s, id=%s\n", s->kind,
           PROBE_USE_SOCKS5 ? "SOCKS5 proxy" : "Pi5", TEST_ID);
    if (PROBE_USE_SOCKS5) {
        const uint8_t greeting[] = {0x05, 0x01, 0x02}; // Username/password only.
        set_stage(s, ST_SOCKS_METHOD);
        if (send_bytes(pcb, greeting, sizeof greeting) != ERR_OK) {
            printf("%c SOCKS greeting send failed\n", s->kind);
            drop(s);
        } else printf("%c SOCKS greeting sent\n", s->kind);
    } else {
        send_application(s);
    }
    return ERR_OK;
}

static void connect_probe(probe_conn *s, uint16_t port) {
    if (s == &a) print_status();
    s->last_try = to_ms_since_boot(get_absolute_time());
    s->attempts++;
    s->used = 0;
    s->connected = false;
    s->target_port = port;
    set_stage(s, ST_TCP_CONNECT);
    s->pcb = tcp_new_ip_type(IPADDR_TYPE_V4);
    if (!s->pcb) return;
    tcp_arg(s->pcb, s);
    tcp_recv(s->pcb, on_recv);
    tcp_err(s->pcb, on_error);
    err_t err = tcp_connect(s->pcb, PROBE_USE_SOCKS5 ? &proxy_addr : &pi_addr,
                            PROBE_USE_SOCKS5 ? SOCKS5_PROXY_PORT : port, on_connected);
    if (err != ERR_OK) {
        printf("%c TCP connect start failed %d, attempt=%u/%u, id=%s\n",
               s->kind, err, s->attempts, MAX_ATTEMPTS, TEST_ID);
        drop(s);
    } else if (PROBE_USE_SOCKS5)
        printf("%c connecting to SOCKS5 proxy %s:%u for Pi5 %s:%u\n", s->kind,
               SOCKS5_PROXY_IP, SOCKS5_PROXY_PORT, PI_TAILSCALE_IP, port);
    else printf("%c connecting directly to Pi5 IPv4 %s:%u\n", s->kind, PI_TAILSCALE_IP, port);
}

static err_t on_accept(void *arg, struct tcp_pcb *pcb, err_t err) {
    (void)arg;
    if (err != ERR_OK || b.pcb) {
        tcp_abort(pcb);
        return ERR_ABRT;
    }
    b.pcb = pcb;
    b.used = 0;
    set_stage(&b, ST_APPLICATION);
    tcp_arg(pcb, &b);
    tcp_recv(pcb, on_recv);
    tcp_err(pcb, on_error);
    printf("B incoming TCP from %s\n", ipaddr_ntoa(&pcb->remote_ip));
    return ERR_OK;
}

int main(void) {
    stdio_init_all();
    sleep_ms(2000);
    printf("probe build=%s boot\n", PROBE_BUILD_ID);
    if (!WIFI_SSID[0] || !WIFI_PASSWORD[0] || !PI_TAILSCALE_IP[0] ||
        !TEST_ID[0] || strlen(TEST_ID) > 50 ||
        !ip4addr_aton(PI_TAILSCALE_IP, ip_2_ip4(&pi_addr))) {
        printf("Local Wi-Fi/Pi5 IPv4/test ID settings incomplete or invalid.\n");
        while (true) sleep_ms(1000);
    }
    IP_SET_TYPE_VAL(pi_addr, IPADDR_TYPE_V4);
    if (PROBE_USE_SOCKS5) {
        size_t ulen = strlen(SOCKS5_USERNAME), plen = strlen(SOCKS5_PASSWORD);
        if (!ulen || !plen || ulen > 255 || plen > 255 ||
            SOCKS5_PROXY_PORT < 1 || SOCKS5_PROXY_PORT > 65535 ||
            !ip4addr_aton(SOCKS5_PROXY_IP, ip_2_ip4(&proxy_addr))) {
            printf("SOCKS5 settings incomplete or invalid; fill SOCKS5_USERNAME and SOCKS5_PASSWORD locally.\n");
            while (true) sleep_ms(1000);
        }
        IP_SET_TYPE_VAL(proxy_addr, IPADDR_TYPE_V4);
    }
    if (cyw43_arch_init()) {
        printf("CYW43 init failed\n");
        return 1;
    }
    cyw43_arch_enable_sta_mode();
    int wifi_rc = cyw43_arch_wifi_connect_timeout_ms(WIFI_SSID, WIFI_PASSWORD,
                                                     CYW43_AUTH_WPA2_AES_PSK, 30000);
    if (wifi_rc) {
        printf("Wi-Fi/IP connection failed code=%d link=%s\n", wifi_rc,
               link_name(cyw43_tcpip_link_status(&cyw43_state, CYW43_ITF_STA)));
        return 1;
    }
    printf("Wi-Fi/IP assigned; id=%s\n", TEST_ID);
    print_status();
    printf("Pico tethering DHCP IPv4=%s; configured Pi5 IPv4=%s\n",
           ip4addr_ntoa(netif_ip4_addr(netif_default)), PI_TAILSCALE_IP);

    b_listener = tcp_new_ip_type(IPADDR_TYPE_V4);
    if (!b_listener || tcp_bind(b_listener, NULL, PORT_B) != ERR_OK) {
        printf("B listen bind failed\n");
        return 1;
    }
    b_listener = tcp_listen_with_backlog(b_listener, 1);
    if (!b_listener) {
        printf("B listen failed\n");
        return 1;
    }
    tcp_accept(b_listener, on_accept);
    printf("B listening on Pico DHCP IPv4 port %d\n", PORT_B);

    a.last_try = c.last_try = to_ms_since_boot(get_absolute_time()) - RETRY_MS;
    uint32_t last_status = to_ms_since_boot(get_absolute_time());
    while (true) {
        cyw43_arch_poll();
        uint32_t now = to_ms_since_boot(get_absolute_time());
        if (now - last_status >= STATUS_MS) {
            print_status();
            last_status = now;
        }
        probe_conn *outgoing[] = {&a, &c};
        for (size_t i = 0; i < 2; i++) {
            probe_conn *s = outgoing[i];
            uint32_t limit = s->stage == ST_APPLICATION ? APP_TIMEOUT_MS : SOCKS_STAGE_TIMEOUT_MS;
            if (s->pcb && !s->done && now - s->stage_since >= limit) {
                printf("%c timeout at stage=%s, attempt=%u/%u, id=%s\n",
                       s->kind, stage_name(s->stage), s->attempts, MAX_ATTEMPTS, TEST_ID);
                drop(s);
            }
        }
        if (!a.done && !a.pcb && a.attempts < MAX_ATTEMPTS && now - a.last_try >= RETRY_MS) connect_probe(&a, PORT_A);
        if (!c.done && !c.pcb && c.attempts < MAX_ATTEMPTS && now - c.last_try >= RETRY_MS) connect_probe(&c, PORT_C);
        sleep_ms(10);
    }
}
