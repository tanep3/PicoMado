# BT-AUDIO-20261107 specification67d7d31. Zero leaves SDK behavior unchanged.
set(PM_BT_HCI_DRAIN_LIMIT "0" CACHE STRING "Experimental HCI drain iterations; 0 leaves SDK limit undefined")
if(NOT "${PM_BT_HCI_DRAIN_LIMIT}" MATCHES "^(0|[1-9][0-9]*)$")
  message(FATAL_ERROR "PM_BT_HCI_DRAIN_LIMIT must be 0 or a positive decimal integer")
endif()
string(LENGTH "${PM_BT_HCI_DRAIN_LIMIT}" _pm_bt_limit_digits)
if(_pm_bt_limit_digits GREATER 10 OR PM_BT_HCI_DRAIN_LIMIT GREATER 2147483647)
  message(FATAL_ERROR "PM_BT_HCI_DRAIN_LIMIT exceeds 2147483647")
endif()
