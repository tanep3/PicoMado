static void handle_signaling_released(uint16_t released_cid){
 if(released_cid!=cid)return;
 bool was_wanted=wanted;
 media_stop();encoder=NULL;rate=0;cid=0;
 if(recovery_releasing())return; /* Internal teardown is not user DISCONNECTED. */
 recovery_cancel();wanted=false;
 /* Failed/cancelled setup must not become Idle and trigger host reconciliation. */
 if(was_wanted)emit((pm_bt_event){.kind=PM_BT_DISCONNECTED});
 stopped_if_idle();
}
