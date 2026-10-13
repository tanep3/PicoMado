#pragma once
#include <stddef.h>
#include <stdint.h>
#include "ip_addr.h"
typedef int err_t;
typedef uint16_t u16_t;
#define ERR_OK 0
#define SOF_KEEPALIVE 1
#define TCP_WRITE_FLAG_COPY 1
#define TCP_WND (16*1460)
struct tcp_pcb { unsigned so_options,keep_idle,keep_intvl,keep_cnt; };
struct pbuf { unsigned tot_len;const uint8_t *payload; };
typedef err_t (*tcp_connected_fn)(void *,struct tcp_pcb *,err_t);
typedef err_t (*tcp_recv_fn)(void *,struct tcp_pcb *,struct pbuf *,err_t);
typedef void (*tcp_err_fn)(void *,err_t);
struct tcp_pcb *tcp_new_ip_type(int type);
void tcp_arg(struct tcp_pcb *pcb,void *arg);
void tcp_recv(struct tcp_pcb *pcb,tcp_recv_fn callback);
void tcp_err(struct tcp_pcb *pcb,tcp_err_fn callback);
err_t tcp_connect(struct tcp_pcb *pcb,const ip_addr_t *address,uint16_t port,tcp_connected_fn callback);
unsigned tcp_sndbuf(struct tcp_pcb *pcb);
err_t tcp_write(struct tcp_pcb *pcb,const void *bytes,size_t length,int flags);
err_t tcp_output(struct tcp_pcb *pcb);
err_t tcp_close(struct tcp_pcb *pcb);
void tcp_abort(struct tcp_pcb *pcb);
void tcp_recved(struct tcp_pcb *pcb,u16_t length);
unsigned pbuf_copy_partial(const struct pbuf *p,void *destination,unsigned length,unsigned offset);
void pbuf_free(struct pbuf *p);
