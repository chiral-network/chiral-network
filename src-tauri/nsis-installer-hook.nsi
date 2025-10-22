; NSIS Installer Hook for Tauri
; This script automatically clears Windows icon cache during installation
; to ensure users see the latest app icons immediately

!macro NSIS_HOOK_POSTINSTALL
  ; Clear icon cache after installation
  DetailPrint "Clearing Windows icon cache..."

  ; Use ie4uinit to refresh icons
  nsExec::ExecToLog 'ie4uinit.exe -show'

  DetailPrint "Icon cache cleared successfully"
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ; Optional: Clear cache on uninstall too
  nsExec::ExecToLog 'ie4uinit.exe -show'
!macroend
