#pragma once
#include <stdbool.h>

/* Read-only local lifecycle query. No plugin IDs, IO, clock or mutable globals. */
typedef enum { WIFI, SERVER, SESSION, ACTIVE, MENU, STANDBY, RETRY, ENDPOINT } app_state;
typedef struct {app_state state;unsigned buttons;bool sub_mode;} pm_input_view;
typedef enum {
    PM_INPUT_IGNORE, PM_INPUT_WAKE, PM_INPUT_MENU_MOVE, PM_INPUT_MENU_ACCEPT,
    PM_INPUT_MENU, PM_INPUT_STANDBY, PM_INPUT_POWER, PM_INPUT_SUB,
    PM_INPUT_UP, PM_INPUT_DOWN
} pm_input_action;
typedef struct {pm_input_action action;bool clear_retained,await_before_send;} pm_input_transition;
static inline pm_input_transition pm_input_decide(pm_input_view q,unsigned key,bool long_press){
    pm_input_transition t={PM_INPUT_IGNORE,false,false};
    if(q.state==STANDBY){
        if(key==2 && (q.buttons==4 || long_press))t.action=PM_INPUT_WAKE;
    }else if(q.state==MENU){
        if(key==1 || key==3)t.action=PM_INPUT_MENU_MOVE;
        else if(key==0)t.action=PM_INPUT_MENU_ACCEPT;
        else if(key==2)t.action=PM_INPUT_WAKE;
    }else if(q.state==ACTIVE){
        t.clear_retained=true;
        if(key==2)t.action=PM_INPUT_POWER;
        else if(key==0)t.action=PM_INPUT_SUB;
        else if(key==1)t.action=PM_INPUT_UP;
        else if(key==3)t.action=PM_INPUT_DOWN;
        t.await_before_send=!q.sub_mode && (key==1 || key==3);
    }else{
        if(key==0)t.action=PM_INPUT_MENU;
        else if(key==2)t.action=PM_INPUT_STANDBY;
    }
    return t;
}
