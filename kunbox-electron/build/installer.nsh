!macro customHeader
  ; 开启高分屏 DPI 感知，解决 Windows 缩放下安装界面模糊问题
  ManifestDPIAware true
  ; 指定默认目录与末级文件夹名，浏览选择 D:\ 时自动追加 \KunBox
  InstallDir "D:\KunBox"
!macroend

!macro customInit
  ; 若未通过 /D 指定安装路径且尚无历史安装记录，在存在 D 盘时默认安装到 D:\KunBox；已有安装记录则保留原安装路径直接覆盖
  !insertmacro GetDParameter $R0
  ${If} $R0 == ""
    ReadRegStr $R1 HKCU "${INSTALL_REGISTRY_KEY}" InstallLocation
    ${If} $R1 == ""
      IfFileExists "D:\*.*" 0 +2
        StrCpy $INSTDIR "D:\KunBox"
    ${EndIf}
  ${EndIf}
!macroend
