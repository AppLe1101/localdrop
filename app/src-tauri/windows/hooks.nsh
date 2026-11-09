!macro NSIS_HOOK_POSTINSTALL
  WriteRegStr HKCU "Software\Classes\*\shell\LocalDrop" "" "Send with LocalDrop"
  WriteRegStr HKCU "Software\Classes\*\shell\LocalDrop" "Icon" "$INSTDIR\${MAINBINARYNAME}.exe"
  WriteRegStr HKCU "Software\Classes\*\shell\LocalDrop\command" "" '"$INSTDIR\${MAINBINARYNAME}.exe" --send "%1"'
  WriteRegStr HKCU "Software\Classes\Directory\shell\LocalDrop" "" "Send with LocalDrop"
  WriteRegStr HKCU "Software\Classes\Directory\shell\LocalDrop" "Icon" "$INSTDIR\${MAINBINARYNAME}.exe"
  WriteRegStr HKCU "Software\Classes\Directory\shell\LocalDrop\command" "" '"$INSTDIR\${MAINBINARYNAME}.exe" --send "%1"'
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  DeleteRegKey HKCU "Software\Classes\*\shell\LocalDrop"
  DeleteRegKey HKCU "Software\Classes\Directory\shell\LocalDrop"
!macroend