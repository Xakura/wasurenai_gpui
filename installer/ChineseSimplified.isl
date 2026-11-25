; *** Wasurenai 简体中文语言包（配合 compiler:Default.isl 兜底使用）***

[LangOptions]
LanguageName=<7B80><4F53><4E2D><6587>
LanguageID=$0804
LanguageCodePage=936

[Messages]

; *** Application titles
SetupAppTitle=安装程序
SetupWindowTitle=%1 安装向导
UninstallAppTitle=卸载
UninstallAppFullTitle=卸载 %1

; *** Misc. common
InformationTitle=提示
ConfirmTitle=确认
ErrorTitle=错误

; *** SetupLdr messages
SetupLdrStartupMessage=即将安装 %1。是否继续？
LdrCannotCreateTemp=无法创建临时文件，安装已中止
LdrCannotExecTemp=无法在临时目录中运行文件，安装已中止

; *** Startup error messages
LastErrorMessage=%1。%n%n错误 %2：%3
SetupFileMissing=安装目录缺少文件 %1。请修复问题或重新获取安装程序。
SetupFileCorrupt=安装文件已损坏，请重新获取安装程序。
SetupFileCorruptOrWrongVer=安装文件已损坏，或与当前安装程序版本不兼容。请重新获取安装程序。
InvalidParameter=命令行参数无效：%n%n%1
SetupAlreadyRunning=安装程序已在运行。
WindowsVersionNotSupported=本程序不支持当前系统的 Windows 版本。
WindowsServicePackRequired=本程序需要 %1 Service Pack %2 或更高版本。
NotOnThisPlatform=本程序无法在 %1 上运行。
OnlyOnThisPlatform=本程序必须在 %1 上运行。
OnlyOnTheseArchitectures=本程序仅支持以下处理器架构的 Windows 系统：%n%n%1
WinVersionTooLowError=本程序需要 %1 %2 或更高版本。
WinVersionTooHighError=本程序无法安装在 %1 %2 或更高版本上。
AdminPrivilegesRequired=安装本程序需要管理员权限，请以管理员身份登录后重试。
PowerUserPrivilegesRequired=安装本程序需要管理员或 Power Users 组权限。
SetupAppRunningError=检测到 %1 正在运行。%n%n请先退出所有正在运行的实例，然后点击“确定”继续，或点击“取消”退出安装。
UninstallAppRunningError=检测到 %1 正在运行。%n%n请先退出所有正在运行的实例，然后点击“确定”继续，或点击“取消”退出卸载。

; *** Startup questions
PrivilegesRequiredOverrideTitle=选择安装模式
PrivilegesRequiredOverrideInstruction=选择安装范围
PrivilegesRequiredOverrideText1=%1 可安装为所有用户可用（需要管理员权限），或仅为您当前用户安装。
PrivilegesRequiredOverrideText2=%1 可仅为您当前用户安装，或安装为所有用户可用（需要管理员权限）。
PrivilegesRequiredOverrideAllUsers=为所有用户安装(&A)
PrivilegesRequiredOverrideAllUsersRecommended=为所有用户安装（推荐）(&A)
PrivilegesRequiredOverrideCurrentUser=仅为当前用户安装(&M)
PrivilegesRequiredOverrideCurrentUserRecommended=仅为当前用户安装（推荐）(&M)

; *** Misc. errors
ErrorCreatingDir=安装程序无法创建目录“%1”
ErrorTooManyFilesInDir=无法在目录“%1”中创建文件，目录内文件过多

; *** Setup common messages
ExitSetupTitle=退出安装
ExitSetupMessage=安装尚未完成。如果现在退出，程序将不会被安装。%n%n您可以稍后重新运行安装程序。%n%n确定要退出安装吗？
AboutSetupMenuItem=关于安装程序(&A)...
AboutSetupTitle=关于安装程序
AboutSetupMessage=%1 版本 %2%n%3%n%n%1 主页：%n%4

; *** Buttons
ButtonBack=< 上一步(&B)
ButtonNext=下一步(&N) >
ButtonInstall=安装(&I)
ButtonOK=确定
ButtonCancel=取消
ButtonYes=是(&Y)
ButtonYesToAll=全部是(&A)
ButtonNo=否(&N)
ButtonNoToAll=全部否(&O)
ButtonFinish=完成(&F)
ButtonBrowse=浏览(&B)...
ButtonWizardBrowse=浏览(&R)...
ButtonNewFolder=新建文件夹(&M)

; *** Common wizard text
ClickNext=点击“下一步”继续，或点击“取消”退出安装。
BrowseDialogTitle=选择文件夹
BrowseDialogLabel=请在下方列表中选择文件夹，然后点击“确定”。
NewFolderName=新建文件夹

; *** "Welcome" wizard page
WelcomeLabel1=欢迎使用 [name] 安装向导
WelcomeLabel2=本向导将在您的计算机上安装 [name/ver]。%n%n建议先关闭正在运行的 [name] 再继续安装。

; *** "Select Destination Location" wizard page
WizardSelectDir=选择安装位置
SelectDirDesc=将 [name] 安装到何处？
SelectDirLabel3=安装程序将把 [name] 安装到以下文件夹中。
SelectDirBrowseLabel=点击“下一步”继续，或点击“浏览”选择其他文件夹。
DiskSpaceGBLabel=至少需要 [gb] GB 可用磁盘空间。
DiskSpaceMBLabel=至少需要 [mb] MB 可用磁盘空间。
CannotInstallToNetworkDrive=安装程序无法安装到网络驱动器。
CannotInstallToUNCPath=安装程序无法安装到 UNC 路径。
InvalidPath=请输入包含盘符的完整路径，例如：%n%nC:\APP%n%n或 UNC 路径：%n%n\\server\share
InvalidDrive=您选择的驱动器或共享路径不存在或不可访问，请重新选择。
DiskSpaceWarningTitle=磁盘空间不足
DiskSpaceWarning=安装至少需要 %1 KB 可用空间，而所选驱动器仅剩 %2 KB。%n%n仍要继续安装吗？
DirNameTooLong=文件夹名称或路径过长。
InvalidDirName=文件夹名称无效。
BadDirName32=文件夹名称不能包含以下字符：%n%n%1
DirExistsTitle=文件夹已存在
DirExists=文件夹：%n%n%1%n%n已存在。是否仍要安装到此文件夹？
DirDoesntExistTitle=文件夹不存在
DirDoesntExist=文件夹：%n%n%1%n%n不存在。是否创建该文件夹？

; *** "Select Additional Tasks" wizard page
WizardSelectTasks=选择附加任务
SelectTasksDesc=安装过程中要执行哪些附加操作？
SelectTasksLabel2=请选择在安装 [name] 时需要执行的附加操作，然后点击“下一步”。

; *** "Ready to Install" wizard page
WizardReady=准备安装
ReadyLabel1=安装程序已准备好在您的计算机上安装 [name]。
ReadyLabel2a=点击“安装”开始安装，或点击“上一步”检查并修改设置。
ReadyLabel2b=点击“安装”开始安装。
ReadyMemoDir=目标位置：
ReadyMemoGroup=开始菜单文件夹：
ReadyMemoTasks=附加任务：

; *** "Preparing to Install" wizard page
WizardPreparing=正在准备安装
PreparingDesc=安装程序正在准备在您的计算机上安装 [name]。
PreviousInstallNotCompleted=上一次程序的安装或卸载尚未完成，需要重启计算机以完成操作。%n%n重启后请再次运行安装程序。
CannotContinue=安装无法继续，请点击“取消”退出。
ApplicationsFound=检测到以下程序正在使用需要更新的文件，建议允许安装程序自动关闭它们。
ApplicationsFound2=检测到以下程序正在使用需要更新的文件，建议允许安装程序自动关闭它们；安装完成后可尝试恢复运行。
CloseApplications=自动关闭相关程序(&A)
DontCloseApplications=不关闭相关程序(&D)
ErrorCloseApplications=安装程序无法自动关闭所有占用文件的程序，建议先手动退出它们再继续。
PrepareToInstallNeedsRestart=需要重启计算机以完成配置，重启后请再次运行本安装程序。%n%n是否立即重启？

; *** "Installing" wizard page
WizardInstalling=正在安装
InstallingLabel=正在将 [name] 安装到您的计算机上，请稍候。

; *** "Setup Completed" wizard page
FinishedHeadingLabel=[name] 安装完成
FinishedLabelNoIcons=[name] 已成功安装到您的计算机上。
FinishedLabel=[name] 已成功安装到您的计算机上，可通过开始菜单或桌面快捷方式启动。
ClickFinish=点击“完成”退出安装向导。
FinishedRestartLabel=需要重启计算机才能完成 [name] 的安装。是否立即重启？
FinishedRestartMessage=需要重启计算机才能完成 [name] 的安装。%n%n是否立即重启？
RunEntryExec=启动 %1

; *** Installation phase messages
SetupAborted=安装未完成。%n%n请排查问题后重新运行安装程序。
AbortRetryIgnoreSelectAction=选择操作
AbortRetryIgnoreRetry=重试(&T)
AbortRetryIgnoreIgnore=忽略错误并继续(&I)
AbortRetryIgnoreCancel=取消安装
RetryCancelSelectAction=选择操作
RetryCancelRetry=重试(&T)
RetryCancelCancel=取消

; *** Installation status messages
StatusClosingApplications=正在关闭占用文件的程序...
StatusCreateDirs=正在创建目录...
StatusExtractFiles=正在释放程序文件...
StatusCreateIcons=正在创建快捷方式...
StatusCreateIniEntries=正在写入配置...
StatusCreateRegistryEntries=正在写入注册表项...
StatusRegisterFiles=正在注册文件...
StatusSavingUninstall=正在保存卸载信息...
StatusRunProgram=正在完成安装...
StatusRestartingApplications=正在恢复应用程序...
StatusRollback=正在回滚更改...

; *** File copying & registry errors
ErrorRegOpenKey=打开注册表项失败：%n%1\%2
ErrorRegCreateKey=创建注册表项失败：%n%1\%2
ErrorRegWriteKey=写入注册表项失败：%n%1\%2
SourceDoesntExist=找不到源文件“%1”
ExistingFileReadOnly2=目标文件具有只读属性，无法替换。
ExistingFileReadOnlyRetry=去掉只读属性并重试(&R)
ExistingFileReadOnlyKeepExisting=保留现有文件(&K)
ErrorReadingExistingDest=读取现有文件时出错：
FileExistsSelectAction=选择操作
FileExists2=文件已存在。
FileExistsOverwriteExisting=覆盖现有文件(&O)
FileExistsKeepExisting=保留现有文件(&K)
FileExistsOverwriteOrKeepAll=对后续冲突执行相同操作(&D)
ErrorCopying=复制文件时出错：
ErrorReplacingExistingFile=替换现有文件时出错：
ErrorRenamingTemp=在目标目录重命名文件时出错：

; *** Uninstall display name markings
UninstallDisplayNameMark=%1（%2）
UninstallDisplayNameMarks=%1（%2，%3）
UninstallDisplayNameMark32Bit=32 位
UninstallDisplayNameMark64Bit=64 位
UninstallDisplayNameMarkAllUsers=所有用户
UninstallDisplayNameMarkCurrentUser=当前用户

; *** Uninstaller messages
UninstallNotFound=找不到文件“%1”，无法卸载。
UninstallOpenError=无法打开文件“%1”，无法卸载
UninstallUnsupportedVer=卸载日志“%1”格式不被当前卸载程序识别，无法卸载
UninstallUnknownEntry=卸载日志中存在未知记录（%1）
ConfirmUninstall=确定要彻底卸载 %1 及其所有组件吗？
UninstallOnlyOnWin64=该安装仅支持在 64 位 Windows 上卸载。
OnlyAdminCanUninstall=该安装仅可由拥有管理员权限的用户卸载。
UninstallStatusLabel=正在从您的计算机中移除 %1，请稍候。
UninstalledAll=已成功将 %1 从您的计算机中移除。
UninstalledMost=%1 卸载完成。%n%n部分文件未能移除，您可以稍后手动清理。
UninstalledAndNeedsRestart=需要重启计算机以完成 %1 的卸载。%n%n是否立即重启？
UninstallDataCorrupted=“%1”文件已损坏，无法卸载

; *** Uninstallation phase messages
WizardUninstalling=卸载进度
StatusUninstalling=正在卸载 %1...

; *** Shutdown block reasons
ShutdownBlockReasonInstallingApp=正在安装 %1。
ShutdownBlockReasonUninstallingApp=正在卸载 %1。

[CustomMessages]
NameAndVersion=%1 %2
AdditionalIcons=附加快捷方式：
CreateDesktopIcon=创建桌面快捷方式(&D)
UninstallProgram=卸载 %1
LaunchProgram=运行 %1
AutoStartProgramGroupDescription=启动选项：
AutoStartProgram=开机自动启动 %1
