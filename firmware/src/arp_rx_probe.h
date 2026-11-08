#pragma once
#include <stdint.h>
#include <stdbool.h>
#include <stddef.h>
#define PM_ARP_RX_CAP 32u
typedef struct {uint64_t at_us;uint8_t eth_src[6],eth_dst[6],sender_mac[6],target_mac[6],sender_ip[4],target_ip[4];uint8_t opcode,itf;} pm_arp_rx_row;
void pm_arp_rx_server(const uint8_t ip[4]);
bool pm_arp_rx_get(unsigned i,pm_arp_rx_row *row,uint32_t *count,uint32_t *dropped);
