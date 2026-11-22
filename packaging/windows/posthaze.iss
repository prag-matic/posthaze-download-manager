#define MyAppName "Posthaze"
#define MyAppDisplayName "Posthaze Download Manager"
#ifndef MyAppVersion
  #define MyAppVersion "0.2.1"
#endif
#define MyAppPublisher "Posthaze Download Manager"
#define MyAppURL "https://github.com/prag-matic/posthaze-download-manager"
#define MyAppExeName "posthaze-ui.exe"
#define MyAppCliName "posthaze-cli.exe"

[Setup]
AppId={{8E9B7D1A-2B4C-4F6E-9A1D-5C3E7F9B0A12}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppVerName={#MyAppDisplayName} {#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
AppSupportURL={#MyAppURL}/issues
AppUpdatesURL={#MyAppURL}/releases
DefaultDirName={localappdata}\Programs\Posthaze
DefaultGroupName={#MyAppName}
DisableWelcomePage=no
DisableProgramGroupPage=yes
AllowNoIcons=yes
UsePreviousAppDir=yes
UsePreviousTasks=yes
PrivilegesRequired=lowest
MinVersion=10.0.17763
OutputDir=..\..\dist
OutputBaseFilename=posthaze-{#MyAppVersion}-windows-x64-setup
SetupIconFile=..\..\assets\posthaze.ico
UninstallDisplayIcon={app}\{#MyAppExeName}
UninstallDisplayName={#MyAppDisplayName}
VersionInfoVersion={#MyAppVersion}.0
VersionInfoCompany={#MyAppPublisher}
VersionInfoProductName={#MyAppDisplayName}
VersionInfoDescription={#MyAppDisplayName} Setup
Compression=lzma2/max
SolidCompression=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
CloseApplications=yes
CloseApplicationsFilter=*.exe
RestartApplications=yes
ChangesEnvironment=yes

; Windows 11 native dynamic light/dark styling & Chronophotographic brand artwork (Inno Setup 6.7+)
WizardStyle=modern dynamic windows11 hidebevels
WizardSizePercent=115,115
WizardBackColor=#F7F5F0
WizardBackColorDynamicDark=#141311
WizardImageFile=assets\wizard-sidebar-light.png
WizardImageFileDynamicDark=assets\wizard-sidebar-dark.png
WizardSmallImageFile=assets\wizard-small-light.png
WizardSmallImageFileDynamicDark=assets\wizard-small-dark.png
WizardImageStretch=yes

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Messages]
WelcomeLabel1=Welcome to {#MyAppDisplayName}
WelcomeLabel2=Install {#MyAppDisplayName} {#MyAppVersion} for your Windows user account — no administrator privileges required.%n%nIncludes the native GPUI desktop app, posthaze-cli command-line tool, and Chromium Browser Capture extension.
SelectDirLabel3=Choose where {#MyAppDisplayName} should be installed.
SelectDirBrowseLabel=Click Next to use the default per-user folder, or Browse to select another location.
FinishedHeadingLabel={#MyAppDisplayName} is ready
FinishedLabel=Posthaze {#MyAppVersion} has been installed on your computer.%n%nTip: To enable Browser Capture for Chrome, Edge, or Brave, load the bundled extension and click Register inside Settings -> Integration.

[Tasks]
Name: "addtopath"; Description: "Add posthaze-cli to user &PATH (for PowerShell and Terminal)"; GroupDescription: "Command-Line Integration:"
Name: "desktopicon"; Description: "Create a &desktop shortcut"; GroupDescription: "Shortcuts:"; Flags: unchecked
Name: "startup"; Description: "Launch Posthaze minimized to System Tray on Windows &startup"; GroupDescription: "Background Integration:"; Flags: unchecked

[Files]
Source: "..\..\target\release\{#MyAppExeName}"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\target\release\{#MyAppCliName}"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\extensions\chrome\*"; DestDir: "{app}\extensions\chrome"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{group}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; Comment: "Multi-connection HTTP/HTTPS download manager"
Name: "{group}\{cm:UninstallProgram,{#MyAppName}}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; Comment: "Multi-connection HTTP/HTTPS download manager"; Tasks: desktopicon

[Registry]
; Optional Launch on Windows Startup (minimized to tray)
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "Posthaze"; ValueData: """{app}\{#MyAppExeName}"" --minimized"; Flags: uninsdeletevalue; Tasks: startup

; Windows App Paths (enables Win+R -> posthaze-ui / posthaze-cli)
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\App Paths\{#MyAppExeName}"; ValueType: string; ValueName: ""; ValueData: "{app}\{#MyAppExeName}"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\App Paths\{#MyAppExeName}"; ValueType: string; ValueName: "Path"; ValueData: "{app}"
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\App Paths\{#MyAppCliName}"; ValueType: string; ValueName: ""; ValueData: "{app}\{#MyAppCliName}"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\App Paths\{#MyAppCliName}"; ValueType: string; ValueName: "Path"; ValueData: "{app}"

; Clean up Chromium Native Messaging Host keys on uninstall without touching them during install/upgrade
Root: HKCU; Subkey: "Software\Google\Chrome\NativeMessagingHosts\com.posthaze.host"; Flags: dontcreatekey uninsdeletekey
Root: HKCU; Subkey: "Software\Microsoft\Edge\NativeMessagingHosts\com.posthaze.host"; Flags: dontcreatekey uninsdeletekey
Root: HKCU; Subkey: "Software\BraveSoftware\Brave-Browser\NativeMessagingHosts\com.posthaze.host"; Flags: dontcreatekey uninsdeletekey

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "Launch {#MyAppName}"; Flags: nowait postinstall skipifsilent
Filename: "{win}\explorer.exe"; Parameters: """{app}\extensions\chrome"""; Description: "Open Chrome extension folder (for Browser Capture)"; Flags: nowait postinstall skipifsilent unchecked
Filename: "{app}\{#MyAppExeName}"; Flags: nowait skipifnotsilent

[Code]
const
  EnvironmentKey = 'Environment';
  PersonalizeKey = 'Software\Microsoft\Windows\CurrentVersion\Themes\Personalize';

var
  StepLabel: TNewStaticText;

function IsSystemDarkMode(): Boolean;
var
  AppsUseLight: Cardinal;
begin
  Result := True;
  if RegQueryDWordValue(HKEY_CURRENT_USER, PersonalizeKey, 'AppsUseLightTheme', AppsUseLight) then
    Result := (AppsUseLight = 0);
end;

function AddInfoRow(
  Parent: TWinControl;
  TopPos: Integer;
  const BadgeText, DescText: string;
  AccentColor, TextColor, MutedColor: TColor
): Integer;
var
  BadgeLbl, DescLbl: TNewStaticText;
begin
  BadgeLbl := TNewStaticText.Create(WizardForm);
  BadgeLbl.Parent := Parent;
  BadgeLbl.Left := ScaleX(14);
  BadgeLbl.Top := TopPos;
  BadgeLbl.ShowAccelChar := False;
  BadgeLbl.Caption := BadgeText;
  BadgeLbl.Font.Name := 'Consolas';
  BadgeLbl.Font.Size := 9;
  BadgeLbl.Font.Style := [fsBold];
  BadgeLbl.Font.Color := AccentColor;
  BadgeLbl.AutoSize := True;

  DescLbl := TNewStaticText.Create(WizardForm);
  DescLbl.Parent := Parent;
  DescLbl.Left := ScaleX(158);
  DescLbl.Top := TopPos;
  DescLbl.Width := Parent.ClientWidth - ScaleX(172);
  DescLbl.ShowAccelChar := False;
  DescLbl.Caption := DescText;
  DescLbl.Font.Name := 'Segoe UI';
  DescLbl.Font.Size := 9;
  DescLbl.Font.Color := TextColor;
  DescLbl.AutoSize := True;

  Result := TopPos + ScaleY(24);
end;

procedure InitializeWizard();
var
  DarkMode: Boolean;
  HeaderBg, SurfaceBg, BorderCol, TextCol, MutedCol, AmberCol: TColor;
  HeaderLine, FooterLine: TPanel;
  CardOuter, CardInner: TPanel;
  CardTitle: TNewStaticText;
  RowY: Integer;
begin
  DarkMode := IsSystemDarkMode();

  // Inno Setup TColor is $00BBGGRR
  AmberCol := $00358AE5; // Selenium Amber #E58A35
  if DarkMode then
  begin
    HeaderBg  := $00171A1B; // #1B1A17 graphite-elevated
    SurfaceBg := $001A1D1E; // #1E1D1A table-bg
    BorderCol := $0025282A; // #2A2825 graphite-border
    TextCol   := $00DFE6EA; // #EAE6DF silver-text
    MutedCol  := $008D969B; // #9B968D silver-muted
  end
  else
  begin
    HeaderBg  := $00E2EAED; // #EDEAE2 elevated paper
    SurfaceBg := $00FCFEFF; // #FFFEFC crisp paper card
    BorderCol := $00C8D3D8; // #D8D3C8 hairline border
    TextCol   := $00181B1C; // #1C1B18 carbon-text
    MutedCol  := $005D666B; // #6B665D carbon-muted
  end;

  // 1. Typography hierarchy on Welcome & Finished pages
  WizardForm.WelcomeLabel1.Font.Name := 'Segoe UI';
  WizardForm.WelcomeLabel1.Font.Size := 16;
  WizardForm.WelcomeLabel1.Font.Style := [fsBold];
  WizardForm.WelcomeLabel2.Font.Name := 'Segoe UI';
  WizardForm.WelcomeLabel2.Font.Size := 10;

  WizardForm.FinishedHeadingLabel.Font.Name := 'Segoe UI';
  WizardForm.FinishedHeadingLabel.Font.Size := 16;
  WizardForm.FinishedHeadingLabel.Font.Style := [fsBold];
  WizardForm.FinishedLabel.Font.Name := 'Segoe UI';
  WizardForm.FinishedLabel.Font.Size := 10;

  // 2. Architectural header band on interior pages
  WizardForm.MainPanel.Color := HeaderBg;
  WizardForm.MainPanel.Height := ScaleY(74);

  WizardForm.WizardSmallBitmapImage.BackColor := HeaderBg;
  WizardForm.WizardSmallBitmapImage.Width := ScaleX(52);
  WizardForm.WizardSmallBitmapImage.Height := ScaleY(52);
  WizardForm.WizardSmallBitmapImage.Left := WizardForm.MainPanel.Width - ScaleX(68);
  WizardForm.WizardSmallBitmapImage.Top := ScaleY(11);

  WizardForm.PageNameLabel.Left := ScaleX(24);
  WizardForm.PageNameLabel.Top := ScaleY(12);
  WizardForm.PageNameLabel.Width := WizardForm.WizardSmallBitmapImage.Left - ScaleX(36);
  WizardForm.PageNameLabel.Height := ScaleY(26);
  WizardForm.PageNameLabel.Font.Name := 'Segoe UI';
  WizardForm.PageNameLabel.Font.Size := 13;
  WizardForm.PageNameLabel.Font.Style := [fsBold];
  WizardForm.PageNameLabel.Font.Color := TextCol;
  WizardForm.PageNameLabel.AutoSize := True;

  WizardForm.PageDescriptionLabel.Left := ScaleX(24);
  WizardForm.PageDescriptionLabel.Top := ScaleY(42);
  WizardForm.PageDescriptionLabel.Width := WizardForm.WizardSmallBitmapImage.Left - ScaleX(36);
  WizardForm.PageDescriptionLabel.Height := ScaleY(20);
  WizardForm.PageDescriptionLabel.Font.Name := 'Segoe UI';
  WizardForm.PageDescriptionLabel.Font.Size := 9;
  WizardForm.PageDescriptionLabel.Font.Color := MutedCol;
  WizardForm.PageDescriptionLabel.AutoSize := True;

  // 1px bottom hairline separating header from page body
  HeaderLine := TPanel.Create(WizardForm);
  HeaderLine.Parent := WizardForm.MainPanel;
  HeaderLine.Left := 0;
  HeaderLine.Top := WizardForm.MainPanel.Height - 1;
  HeaderLine.Width := WizardForm.MainPanel.Width;
  HeaderLine.Height := 1;
  HeaderLine.BevelOuter := bvNone;
  HeaderLine.ParentBackground := False;
  HeaderLine.Color := BorderCol;

  // 3. Refine Select Destination Location page (remove dated yellow folder icon & fill empty void)
  WizardForm.SelectDirBitmapImage.Visible := False;
  WizardForm.SelectDirLabel.Left := 0;
  WizardForm.SelectDirLabel.Top := ScaleY(6);
  WizardForm.SelectDirLabel.Width := WizardForm.SelectDirPage.Width;
  WizardForm.SelectDirLabel.Font.Size := 9;

  WizardForm.SelectDirBrowseLabel.Left := 0;
  WizardForm.SelectDirBrowseLabel.Top := ScaleY(28);
  WizardForm.SelectDirBrowseLabel.Width := WizardForm.SelectDirPage.Width;
  WizardForm.SelectDirBrowseLabel.Font.Color := MutedCol;

  WizardForm.DirEdit.Top := ScaleY(56);
  WizardForm.DirBrowseButton.Top := ScaleY(55);

  // Structured "Included in this installation" plate below path input
  CardOuter := TPanel.Create(WizardForm);
  CardOuter.Parent := WizardForm.SelectDirPage;
  CardOuter.Left := 0;
  CardOuter.Top := ScaleY(102);
  CardOuter.Width := WizardForm.SelectDirPage.Width;
  CardOuter.Height := ScaleY(122);
  CardOuter.BevelOuter := bvNone;
  CardOuter.ParentBackground := False;
  CardOuter.Color := BorderCol;

  CardInner := TPanel.Create(WizardForm);
  CardInner.Parent := CardOuter;
  CardInner.Left := 1;
  CardInner.Top := 1;
  CardInner.Width := CardOuter.Width - 2;
  CardInner.Height := CardOuter.Height - 2;
  CardInner.BevelOuter := bvNone;
  CardInner.ParentBackground := False;
  CardInner.Color := SurfaceBg;

  CardTitle := TNewStaticText.Create(WizardForm);
  CardTitle.Parent := CardInner;
  CardTitle.Left := ScaleX(14);
  CardTitle.Top := ScaleY(12);
  CardTitle.Caption := 'Included components (per-user install, no UAC required)';
  CardTitle.Font.Name := 'Segoe UI';
  CardTitle.Font.Size := 9;
  CardTitle.Font.Style := [fsBold];
  CardTitle.Font.Color := MutedCol;
  CardTitle.AutoSize := True;

  RowY := ScaleY(38);
  RowY := AddInfoRow(CardInner, RowY, 'posthaze-ui.exe', 'Native GPUI desktop app & System Tray', AmberCol, TextCol, MutedCol);
  RowY := AddInfoRow(CardInner, RowY, 'posthaze-cli.exe', 'Multi-segment CLI, benchmark & Native Host', AmberCol, TextCol, MutedCol);
  AddInfoRow(CardInner, RowY, 'extensions\chrome\', 'Chromium Browser Capture extension', AmberCol, TextCol, MutedCol);

  WizardForm.DiskSpaceLabel.Font.Color := MutedCol;

  // 4. Footer 1px hairline and calibrated Step Indicator
  FooterLine := TPanel.Create(WizardForm);
  FooterLine.Parent := WizardForm;
  FooterLine.Left := 0;
  FooterLine.Top := WizardForm.NextButton.Top - ScaleY(12);
  FooterLine.Width := WizardForm.ClientWidth;
  FooterLine.Height := 1;
  FooterLine.BevelOuter := bvNone;
  FooterLine.ParentBackground := False;
  FooterLine.Color := BorderCol;

  StepLabel := TNewStaticText.Create(WizardForm);
  StepLabel.Parent := WizardForm;
  StepLabel.Left := ScaleX(20);
  StepLabel.Top := WizardForm.NextButton.Top + ScaleY(4);
  StepLabel.Font.Name := 'Consolas';
  StepLabel.Font.Size := 8;
  StepLabel.Font.Color := MutedCol;
  StepLabel.AutoSize := True;
  StepLabel.Caption := 'STEP 1 / 4  ·  WELCOME';
end;

procedure CurPageChanged(CurPageID: Integer);
begin
  if StepLabel = nil then
    Exit;
  case CurPageID of
    wpWelcome:
      StepLabel.Caption := 'STEP 1 / 4  ·  WELCOME';
    wpSelectDir:
      StepLabel.Caption := 'STEP 2 / 4  ·  DESTINATION';
    wpSelectTasks:
      StepLabel.Caption := 'STEP 3 / 4  ·  OPTIONS';
    wpReady:
      StepLabel.Caption := 'STEP 4 / 4  ·  READY TO INSTALL';
    wpInstalling:
      StepLabel.Caption := 'INSTALLING POSTHAZE {#MyAppVersion}...';
    wpFinished:
      StepLabel.Caption := 'COMPLETE  ·  POSTHAZE {#MyAppVersion}';
  else
    StepLabel.Caption := '';
  end;
end;

function NormalizeDir(const S: string): string;
begin
  Result := Uppercase(RemoveBackslashUnlessRoot(Trim(S)));
end;

function PathContainsDir(const PathValue, Dir: string): Boolean;
var
  Remaining, Entry, Target: string;
  P: Integer;
begin
  Result := False;
  Target := NormalizeDir(Dir);
  Remaining := PathValue;
  while Remaining <> '' do
  begin
    P := Pos(';', Remaining);
    if P > 0 then
    begin
      Entry := Copy(Remaining, 1, P - 1);
      Delete(Remaining, 1, P);
    end
    else
    begin
      Entry := Remaining;
      Remaining := '';
    end;
    if NormalizeDir(Entry) = Target then
    begin
      Result := True;
      Exit;
    end;
  end;
end;

procedure EnvAddPath(const Dir: string);
var
  CurrentPath, UpdatedPath: string;
begin
  if not RegQueryStringValue(HKEY_CURRENT_USER, EnvironmentKey, 'Path', CurrentPath) then
    CurrentPath := '';

  if PathContainsDir(CurrentPath, Dir) then
    Exit;

  if (CurrentPath <> '') and (CurrentPath[Length(CurrentPath)] <> ';') then
    UpdatedPath := CurrentPath + ';' + Dir
  else
    UpdatedPath := CurrentPath + Dir;

  RegWriteExpandStringValue(HKEY_CURRENT_USER, EnvironmentKey, 'Path', UpdatedPath);
end;

procedure EnvRemovePath(const Dir: string);
var
  CurrentPath, Remaining, Entry, UpdatedPath, Target: string;
  P: Integer;
begin
  if not RegQueryStringValue(HKEY_CURRENT_USER, EnvironmentKey, 'Path', CurrentPath) then
    Exit;

  if not PathContainsDir(CurrentPath, Dir) then
    Exit;

  Target := NormalizeDir(Dir);
  Remaining := CurrentPath;
  UpdatedPath := '';

  while Remaining <> '' do
  begin
    P := Pos(';', Remaining);
    if P > 0 then
    begin
      Entry := Copy(Remaining, 1, P - 1);
      Delete(Remaining, 1, P);
    end
    else
    begin
      Entry := Remaining;
      Remaining := '';
    end;

    if (Trim(Entry) <> '') and (NormalizeDir(Entry) <> Target) then
    begin
      if UpdatedPath <> '' then
        UpdatedPath := UpdatedPath + ';';
      UpdatedPath := UpdatedPath + Entry;
    end;
  end;

  RegWriteExpandStringValue(HKEY_CURRENT_USER, EnvironmentKey, 'Path', UpdatedPath);
end;

procedure StopRunningPosthaze();
var
  ResultCode: Integer;
begin
  // Posthaze minimizes to the System Tray on WM_CLOSE, so ensure any running
  // tray instance exits before file replacement during upgrades or uninstalls.
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/IM {#MyAppExeName} /F', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  StopRunningPosthaze();
  Result := '';
end;

function InitializeUninstall(): Boolean;
begin
  StopRunningPosthaze();
  Result := True;
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if (CurStep = ssPostInstall) and WizardIsTaskSelected('addtopath') then
    EnvAddPath(ExpandConstant('{app}'));
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  DataDir: string;
begin
  if CurUninstallStep = usPostUninstall then
  begin
    EnvRemovePath(ExpandConstant('{app}'));

    DataDir := ExpandConstant('{localappdata}\posthaze');
    if (not UninstallSilent) and DirExists(DataDir) then
    begin
      if MsgBox(
        'Do you also want to remove your Posthaze user data (download history, settings, and diagnostic logs) from:' + #13#10 +
        DataDir + '?',
        mbConfirmation,
        MB_YESNO or MB_DEFBUTTON2
      ) = IDYES then
      begin
        DelTree(DataDir, True, True, True);
      end;
    end;
  end;
end;
