#pragma once
#include "input_types.h"

/* Read-only application state and semantic capabilities. GPIO layout and
 * transport do not participate in deciding the meaning of an operation. */
typedef enum { WIFI, SERVER, SESSION, ACTIVE, MENU, STANDBY, RETRY, ENDPOINT } app_state;
typedef struct {app_state state;bool sub_mode;uint8_t operations;bool blocked;} pm_input_view;
typedef enum {
    PM_INPUT_IGNORE, PM_INPUT_WAKE, PM_INPUT_MENU_MOVE, PM_INPUT_MENU_ACCEPT,
    PM_INPUT_MENU, PM_INPUT_STANDBY, PM_INPUT_POWER, PM_INPUT_SUB,
    PM_INPUT_UP, PM_INPUT_DOWN
} pm_input_action;
typedef struct {pm_input_action action;bool clear_retained,await_before_send;} pm_input_transition;
typedef struct {pm_input_result result;pm_input_transition transition;} pm_input_plan;
static inline pm_input_plan pm_input_decide(pm_input_view q,pm_operation operation){
    pm_input_plan p={PM_INPUT_INVALID,{PM_INPUT_IGNORE,false,false}};
    if(!pm_operation_valid(operation)||(q.operations&~PM_OPERATION_MASK)||(unsigned)q.state>ENDPOINT)return p;
    if(!(q.operations&(1u<<pm_operation_index(operation)))){p.result=PM_INPUT_UNSUPPORTED;return p;}
    if(q.blocked){p.result=PM_INPUT_BUSY;return p;}
    pm_input_transition *t=&p.transition;unsigned key=pm_operation_index(operation);
    if(q.state==STANDBY){
        if(key==2)t->action=PM_INPUT_WAKE;
    }else if(q.state==MENU){
        if(key==1||key==3)t->action=PM_INPUT_MENU_MOVE;
        else if(key==0)t->action=PM_INPUT_MENU_ACCEPT;
        else if(key==2)t->action=PM_INPUT_WAKE;
    }else if(q.state==ACTIVE){
        t->clear_retained=true;
        if(key==2)t->action=PM_INPUT_POWER;
        else if(key==0)t->action=PM_INPUT_SUB;
        else if(key==1)t->action=PM_INPUT_UP;
        else if(key==3)t->action=PM_INPUT_DOWN;
        t->await_before_send=!q.sub_mode&&(key==1||key==3);
    }else{
        if(key==0)t->action=PM_INPUT_MENU;
        else if(key==2)t->action=PM_INPUT_STANDBY;
    }
    p.result=t->action==PM_INPUT_IGNORE?PM_INPUT_IGNORED:PM_INPUT_READY;return p;
}
