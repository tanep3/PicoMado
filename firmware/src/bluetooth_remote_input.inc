/* Shared runtime callback edge admission; no user/network IO. */
static void handle_remote_input(uint16_t c,uint8_t op,bool down){
 bool supported=op==AVRCP_OPERATION_ID_PLAY||op==AVRCP_OPERATION_ID_PAUSE||op==AVRCP_OPERATION_ID_FORWARD||op==AVRCP_OPERATION_ID_BACKWARD||op==AVRCP_OPERATION_ID_VOLUME_UP||op==AVRCP_OPERATION_ID_VOLUME_DOWN;
 if(!wanted||c!=remote_cid||!input_enabled||!supported)return;
 /* A lost edge must invalidate held state before any later release. */
 if(events.count>=4){input_epoch++;return;}
 /* SDK auto-acknowledges valid opcodes before this callback. */
 emit((pm_bt_event){.kind=PM_BT_INPUT,.operation=op,.pressed=down,.input_ms=btstack_run_loop_get_time_ms(),.input_epoch=input_epoch});
}
