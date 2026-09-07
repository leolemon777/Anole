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
  WriteRegStr SHCTX "Software\Classes\*\shell\FormatWright" "MUIVerb" "Open in Anole"
  WriteRegStr SHCTX "Software\Classes\*\shell\FormatWright" "Icon" "$INSTDIR\${MAINBINARYNAME}.exe,0"
  WriteRegStr SHCTX "Software\Classes\*\shell\FormatWright\command" "" '"$INSTDIR\${MAINBINARYNAME}.exe" --shell-open "%1"'

  WriteRegStr SHCTX "Software\Classes\Directory\shell\FormatWright" "MUIVerb" "Open in Anole"
  WriteRegStr SHCTX "Software\Classes\Directory\shell\FormatWright" "Icon" "$INSTDIR\${MAINBINARYNAME}.exe,0"
  WriteRegStr SHCTX "Software\Classes\Directory\shell\FormatWright\command" "" '"$INSTDIR\${MAINBINARYNAME}.exe" --shell-open "%1"'

  ExecWait '"$INSTDIR\${MAINBINARYNAME}.exe" --register-shell'
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  DeleteRegKey SHCTX "Software\Classes\*\shell\FormatWright"
  DeleteRegKey SHCTX "Software\Classes\Directory\shell\FormatWright"
  !insertmacro FW_DELETE_CONVERT_VERB ".pdf" "FormatWright.ToPng"
  !insertmacro FW_DELETE_CONVERT_VERB ".pdf" "FormatWright.ToJpg"
  !insertmacro FW_DELETE_CONVERT_VERB ".png" "FormatWright.ToWebp"
  !insertmacro FW_DELETE_CONVERT_VERB ".jpg" "FormatWright.ToWebp"
  !insertmacro FW_DELETE_CONVERT_VERB ".jpeg" "FormatWright.ToWebp"
  !insertmacro FW_DELETE_CONVERT_VERB ".json" "FormatWright.ToYaml"
  !insertmacro FW_DELETE_CONVERT_VERB ".csv" "FormatWright.ToJson"
  !insertmacro FW_DELETE_CONVERT_VERB ".yaml" "FormatWright.ToJson"
  !insertmacro FW_DELETE_CONVERT_VERB ".yml" "FormatWright.ToJson"
  !insertmacro FW_DELETE_CONVERT_VERB ".xml" "FormatWright.ToJson"
  !insertmacro FW_DELETE_CONVERT_VERB ".mp4" "FormatWright.ToMp3"
  !insertmacro FW_DELETE_CONVERT_VERB ".mkv" "FormatWright.ToMp4"
  !insertmacro FW_DELETE_CONVERT_VERB ".mov" "FormatWright.ToMp4"
  !insertmacro FW_DELETE_CONVERT_VERB ".avi" "FormatWright.ToMp4"
  !insertmacro FW_DELETE_CONVERT_VERB ".webm" "FormatWright.ToMp4"
  !insertmacro FW_DELETE_CONVERT_VERB ".mp3" "FormatWright.ToWav"
  !insertmacro FW_DELETE_CONVERT_VERB ".wav" "FormatWright.ToMp3"
  !insertmacro FW_DELETE_DIRECTORY_CONVERT_VERB "FormatWright.FolderToJpg"
  !insertmacro FW_DELETE_DIRECTORY_CONVERT_VERB "FormatWright.FolderToWebp"
!macroend
