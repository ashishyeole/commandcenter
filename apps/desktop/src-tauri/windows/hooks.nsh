; Interactive removal offers an escape to the app's verified-backup/reset flow.
; Never delete personal data, OS keys or backups in an installer hook.
!macro NSIS_HOOK_PREUNINSTALL
  IfSilent pcc_keep_data
  MessageBox MB_OKCANCEL|MB_ICONINFORMATION|MB_DEFBUTTON2 "Before removing Command Center, open Data & privacy in the app and create a verified encrypted backup. For intentional data deletion, use the app's reset flow first. This hook does not delete your workspace or OS credentials. Cancel to return and back up; OK to continue removing the application." IDOK pcc_keep_data
  Abort
  pcc_keep_data:
!macroend
