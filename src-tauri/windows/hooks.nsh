; aion2dps installer hooks (Tauri v2 NSIS, bundle.windows.nsis.installerHooks).
;
; After the files are installed, check for Npcap. aion2dps needs it for live
; capture, but the app itself installs and opens fine without it, so a missing
; Npcap never stops the install: it only shows a notice and offers the download
; page. Silent installs (/S) show nothing. Npcap is never bundled (its free
; license does not allow redistribution).

!macro NSIS_HOOK_POSTINSTALL
  Push $0
  StrCpy $0 ""

  ; 1. Npcap's own registry key. The installer is 32-bit, so read the 64-bit
  ;    view first, then the default view.
  SetRegView 64
  ReadRegStr $0 HKLM "SOFTWARE\Npcap" ""
  SetRegView lastused
  ${If} $0 == ""
    ReadRegStr $0 HKLM "SOFTWARE\Npcap" ""
  ${EndIf}

  ; 2. The DLL the app loads (C:\Windows\System32\Npcap\wpcap.dll). Sysnative
  ;    reaches the real System32 from a 32-bit installer on 64-bit Windows.
  ${If} $0 == ""
    ${If} ${FileExists} "$WINDIR\Sysnative\Npcap\wpcap.dll"
      StrCpy $0 "found"
    ${ElseIf} ${FileExists} "$WINDIR\System32\Npcap\wpcap.dll"
      StrCpy $0 "found"
    ${EndIf}
  ${EndIf}

  ${If} $0 == ""
    DetailPrint "Npcap was not found. Install it from https://npcap.com for live capture."
    IfSilent aion2dps_npcap_done
    MessageBox MB_YESNO|MB_ICONINFORMATION "Npcap was not found. aion2dps needs Npcap for live capture.$\r$\n$\r$\nInstall it from https://npcap.com with 'WinPcap API-compatible Mode' checked, and run aion2dps as Administrator.$\r$\n$\r$\nOpen the Npcap download page now?" IDNO aion2dps_npcap_done
    ExecShell "open" "https://npcap.com"
  ${Else}
    DetailPrint "Npcap found."
  ${EndIf}
  aion2dps_npcap_done:

  Pop $0
!macroend