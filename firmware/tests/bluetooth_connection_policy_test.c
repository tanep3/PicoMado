#include <assert.h>
#include <stdio.h>
#include "bluetooth_connection_policy.h"
int main(void){
 uint8_t target[6]={1,2,3,4,5,6},other[6]={1,2,3,4,5,7};
 uint16_t cid=11;
 assert(pm_bt_accept_peer(true,false,target,target));
 /* Fake successful incoming wins collision; late outgoing failure is ignored. */
 if(pm_bt_accept_peer(true,false,target,target))cid=12;
 assert(!pm_bt_owns_failure(cid,11));assert(pm_bt_owns_failure(cid,12));
 assert(!pm_bt_owns_failure(0,0));
 assert(!pm_bt_accept_peer(true,false,target,other));
 assert(!pm_bt_accept_peer(false,false,target,target));
 assert(!pm_bt_accept_peer(true,true,target,target));
 puts("Bluetooth connection ownership: collision, stale CID, other peer, OFF PASS");
}
