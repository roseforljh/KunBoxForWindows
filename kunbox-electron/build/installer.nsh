!macro customHeader
  ; 开启高分屏 DPI 感知，解决 Windows 缩放下安装界面模糊问题
  ManifestDPIAware true
  ; 指定默认目录与末级文件夹名，浏览选择 D:\ 时自动追加 \KunBox
  InstallDir "D:\KunBox"
!macroend

!macro customInit
  ; 若未通过 /D 指定安装路径且存在 D 盘，默认安装并覆盖到 D:\KunBox
  !insertmacro GetDParameter $R0
  ${If} $R0 == ""
    IfFileExists "D:\*.*" 0 +2
      StrCpy $INSTDIR "D:\KunBox"
  ${EndIf}
!macroend
