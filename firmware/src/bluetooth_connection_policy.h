#pragma once
#include <stdbool.h>
#include <stdint.h>
#include <string.h>
/* Incoming ownership comes from the current explicit target, never CID alone. */
static inline bool pm_bt_accept_peer(bool wanted,bool stopping,const uint8_t peer[6],const uint8_t incoming[6]){
 return wanted&&!stopping&&memcmp(peer,incoming,6)==0;
}
static inline bool pm_bt_owns_failure(uint16_t owned,uint16_t callback){return owned!=0&&owned==callback;}
