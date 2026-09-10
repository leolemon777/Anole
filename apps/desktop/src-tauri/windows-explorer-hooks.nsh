!macro FW_CONVERT_VERB ASSOC VERB TARGET LABEL
  WriteRegStr SHCTX "Software\Classes\SystemFileAssociations\${ASSOC}\shell\${VERB}" "MUIVerb" "${LABEL}"
  WriteRegStr SHCTX "Software\Classes\SystemFileAssociations\${ASSOC}\shell\${VERB}" "Icon" "$INSTDIR\${MAINBINARYNAME}.exe,0"
  WriteRegStr SHCTX "Software\Classes\SystemFileAssociations\${ASSOC}\shell\${VERB}\command" "" '"$INSTDIR\${MAINBINARYNAME}.exe" --shell-convert --to ${TARGET} "%1"'
!macroend

!macro FW_DELETE_CONVERT_VERB ASSOC VERB
  DeleteRegKey SHCTX "Software\Classes\SystemFileAssociations\${ASSOC}\shell\${VERB}"
!macroend

!macro FW_DIRECTORY_CONVERT_VERB VERB TARGET LABEL
  WriteRegStr SHCTX "Software\Classes\Directory\shell\${VERB}" "MUIVerb" "${LABEL}"
  WriteRegStr SHCTX "Software\Classes\Directory\shell\${VERB}" "Icon" "$INSTDIR\${MAINBINARYNAME}.exe,0"
  WriteRegStr SHCTX "Software\Classes\Directory\shell\${VERB}\command" "" '"$INSTDIR\${MAINBINARYNAME}.exe" --shell-convert --to ${TARGET} "%1"'
!macroend

!macro FW_DELETE_DIRECTORY_CONVERT_VERB VERB
  DeleteRegKey SHCTX "Software\Classes\Directory\shell\${VERB}"
!macroend

!macro NSIS_HOOK_POSTINSTALL
  WriteRegStr SHCTX "Software\Classes\*\shell\Anole" "MUIVerb" "Open in Anole"
  WriteRegStr SHCTX "Software\Classes\*\shell\Anole" "Icon" "$INSTDIR\${MAINBINARYNAME}.exe,0"
  WriteRegStr SHCTX "Software\Classes\*\shell\Anole\command" "" '"$INSTDIR\${MAINBINARYNAME}.exe" --shell-open "%1"'

  WriteRegStr SHCTX "Software\Classes\Directory\shell\Anole" "MUIVerb" "Open in Anole"
  WriteRegStr SHCTX "Software\Classes\Directory\shell\Anole" "Icon" "$INSTDIR\${MAINBINARYNAME}.exe,0"
  WriteRegStr SHCTX "Software\Classes\Directory\shell\Anole\command" "" '"$INSTDIR\${MAINBINARYNAME}.exe" --shell-open "%1"'

  ExecWait '"$INSTDIR\${MAINBINARYNAME}.exe" --register-shell'
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  DeleteRegKey SHCTX "Software\Classes\*\shell\Anole"
  DeleteRegKey SHCTX "Software\Classes\Directory\shell\Anole"
  !insertmacro FW_DELETE_CONVERT_VERB ".pdf" "Anole.ToPng"
  !insertmacro FW_DELETE_CONVERT_VERB ".pdf" "Anole.ToJpg"
  !insertmacro FW_DELETE_CONVERT_VERB ".png" "Anole.ToWebp"
  !insertmacro FW_DELETE_CONVERT_VERB ".jpg" "Anole.ToWebp"
  !insertmacro FW_DELETE_CONVERT_VERB ".jpeg" "Anole.ToWebp"
  !insertmacro FW_DELETE_CONVERT_VERB ".json" "Anole.ToYaml"
  !insertmacro FW_DELETE_CONVERT_VERB ".csv" "Anole.ToJson"
  !insertmacro FW_DELETE_CONVERT_VERB ".yaml" "Anole.ToJson"
  !insertmacro FW_DELETE_CONVERT_VERB ".yml" "Anole.ToJson"
  !insertmacro FW_DELETE_CONVERT_VERB ".xml" "Anole.ToJson"
  !insertmacro FW_DELETE_CONVERT_VERB ".mp4" "Anole.ToMp3"
  !insertmacro FW_DELETE_CONVERT_VERB ".mkv" "Anole.ToMp4"
  !insertmacro FW_DELETE_CONVERT_VERB ".mov" "Anole.ToMp4"
  !insertmacro FW_DELETE_CONVERT_VERB ".avi" "Anole.ToMp4"
  !insertmacro FW_DELETE_CONVERT_VERB ".webm" "Anole.ToMp4"
  !insertmacro FW_DELETE_CONVERT_VERB ".mp3" "Anole.ToWav"
  !insertmacro FW_DELETE_CONVERT_VERB ".wav" "Anole.ToMp3"
  !insertmacro FW_DELETE_DIRECTORY_CONVERT_VERB "Anole.FolderToJpg"
  !insertmacro FW_DELETE_DIRECTORY_CONVERT_VERB "Anole.FolderToWebp"
!macroend
