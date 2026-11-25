; Gerado por scripts/i18n.mjs sync — não edite à mão.
; Grava o idioma escolhido no instalador; o Larafim usa na primeira vez que abre.
; Atualizações feitas pelo app ($UpdateMode = 1) não mexem na chave.
!include "LogicLib.nsh"

!macro NSIS_HOOK_POSTINSTALL
  ${If} $UpdateMode <> 1
    StrCpy $0 "pt-BR"
    ${If} $LANGUAGE == 1046
      StrCpy $0 "pt-BR"
    ${ElseIf} $LANGUAGE == 1033
      StrCpy $0 "en"
    ${EndIf}
    WriteRegStr HKCU "Software\Larafim" "Language" $0
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $UpdateMode <> 1
    DeleteRegKey HKCU "Software\Larafim"
  ${EndIf}
!macroend
