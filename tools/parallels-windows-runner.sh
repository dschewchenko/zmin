#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

vm_name="${ZMIN_PARALLELS_VM_NAME:-Skron Windows Runner}"
vm_home="${ZMIN_PARALLELS_VM_HOME:-$HOME/Parallels}"
vm_dir="$vm_home/$vm_name.pvm"
windows_iso="${ZMIN_PARALLELS_WINDOWS_ISO:-$HOME/Library/Containers/com.utmapp.UTM/Data/Documents/Win11_25H2_English_Arm64_v2.iso}"
boot_iso="${ZMIN_PARALLELS_BOOT_ISO:-/tmp/zmin-win11-arm-parallels-noprompt.iso}"
tools_iso="${ZMIN_PARALLELS_TOOLS_ISO:-/Applications/Parallels Desktop.app/Contents/Resources/Tools/prl-tools-win-arm.iso}"
tools_root="${ZMIN_PARALLELS_TOOLS_ROOT:-/Applications/Parallels Desktop.app/Contents/Resources/Tools}"
answer_iso="$vm_dir/zmin-autounattend.iso"
guest_user="${ZMIN_PARALLELS_GUEST_USER:-skron}"
guest_pass="${ZMIN_PARALLELS_GUEST_PASS:-SkronGit123!}"
memory_mb="${ZMIN_PARALLELS_MEMORY_MB:-8192}"
num_vcpus="${ZMIN_PARALLELS_NUM_VCPUS:-4}"
disk_mb="${ZMIN_PARALLELS_DISK_MB:-81920}"
min_free_gib="${ZMIN_PARALLELS_MIN_FREE_GIB:-35}"
guest_exec_timeout_seconds="${ZMIN_PARALLELS_GUEST_EXEC_TIMEOUT_SECONDS:-180}"

usage() {
  cat >&2 <<'USAGE'
Usage: tools/parallels-windows-runner.sh <command>

Commands:
  status              Show Parallels VM/resource status.
  stop                Stop the runner VM.
  create              Create/update a Windows ARM runner VM.
  start [gui|headless] Start the VM.
  screenshot [PATH]   Capture the VM screen.
  tools               Check whether Parallels guest exec works.
  guest CMD...        Run a command in the guest through Parallels Tools.
  bootstrap           Install/enable Git, Rust, OpenSSH in the guest.
  validate [MODE] [TEST_FILE] [CASE]
                      Run Windows-native validation in the guest.
  extended [MODE]     Run expanded Windows-native compatibility gate.
  upstream [MODE]     Run selected upstream Git t-suite audit in the guest.
  upstream-fast [MODE]
                      Rerun upstream audit with existing zmin.exe, no native
                      preflight, and detached polling.
  upstream-compat [MODE]
                      Run upstream audit with the faster Cargo compat profile,
                      no native preflight, and detached polling.
  upstream-poll OUT_DIR
                      Poll a detached upstream Git t-suite audit output dir.
  build-release       Build Windows release zmin.exe in the shared guest target.
  benchmark [REPEATS] [OPS]
                      Run Windows-native stock Git vs zmin benchmark.
                      OPS is a comma-separated operation allowlist.
  http-benchmark [REPEATS]
                      Run Windows-native smart HTTP stock Git vs zmin benchmark.
  cleanup             Remove local temp artifacts, not VM disk.
  destroy             Stop and remove the runner VM.

Environment:
	  ZMIN_PARALLELS_WINDOWS_ISO
  ZMIN_PARALLELS_BOOT_ISO
  ZMIN_PARALLELS_TOOLS_ISO
  ZMIN_PARALLELS_TOOLS_ROOT
  ZMIN_PARALLELS_MEMORY_MB
  ZMIN_PARALLELS_NUM_VCPUS
  ZMIN_PARALLELS_GUEST_USER
  ZMIN_PARALLELS_GUEST_PASS
  ZMIN_WINDOWS_VALIDATE_NO_FMT
  ZMIN_WINDOWS_VALIDATE_BUILD_PROFILE
  ZMIN_WINDOWS_EXTENDED_BUILD_PROFILE
  ZMIN_WINDOWS_BENCH_OPS
  ZMIN_WINDOWS_BENCH_SSH_TRACE
  ZMIN_WINDOWS_BENCH_SSH_PACKET_TRACE
  ZMIN_WINDOWS_BENCH_PHASE_TRACE_ONLY
  ZMIN_WINDOWS_BENCH_MAX_ZMIN_VS_GIT_MEAN_RATIO
  ZMIN_WINDOWS_BENCH_MAX_ZMIN_VS_GIT_MEDIAN_RATIO
  ZMIN_WINDOWS_BENCH_MAX_ZMIN_VS_GIT_PAIR_MEDIAN_RATIO
  ZMIN_WINDOWS_BENCH_MAX_ZMIN_VS_GIX_MEAN_RATIO
  ZMIN_WINDOWS_BENCH_MAX_ZMIN_VS_GIX_MEDIAN_RATIO
  ZMIN_WINDOWS_BENCH_MAX_ZMIN_VS_GIX_PAIR_MEDIAN_RATIO
  ZMIN_PARALLELS_GUEST_EXEC_TIMEOUT_SECONDS
  ZMIN_PARALLELS_UPSTREAM_DETACH
  ZMIN_PARALLELS_UPSTREAM_REUSE_BINARY
  ZMIN_PARALLELS_UPSTREAM_SKIP_PREFLIGHT
  ZMIN_PARALLELS_UPSTREAM_CARGO_PROFILE
  ZMIN_PARALLELS_CARGO_BUILD_JOBS
USAGE
}

require_command() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "missing command: $1" >&2
    exit 1
  fi
}

require_file() {
  local path="$1"
  if [[ ! -e "$path" ]]; then
    echo "missing: $path" >&2
    exit 1
  fi
}

free_gib() {
  mkdir -p "$vm_home"
  df -g "$vm_home" | awk 'NR == 2 { print $4 }'
}

require_space() {
  local free
  free="$(free_gib)"
  if (( free < min_free_gib )); then
    echo "not enough free space: ${free}GiB available, need at least ${min_free_gib}GiB" >&2
    exit 1
  fi
}

vm_exists() {
  prlctl list -a --no-header -o name | awk '{$1=$1; print}' | grep -Fxq "$vm_name"
}

stop_stale_host_guest_exec_sessions() {
  ps -axo pid=,command= | while read -r pid command; do
    if [[ "$pid" == "$$" ]]; then
      continue
    fi
    if [[ "$command" != *"prlctl exec ${vm_name}"* ]]; then
      continue
    fi
    case "$command" in
      *t0027-auto-crlf*|*zmin-direct*|*zmin-stockgit*|*zmin-upstream*)
        kill "$pid" 2>/dev/null || true
        ;;
      *.zmin-parallels-script.*.ps1*)
        local script_path="${command##*& }"
        script_path="${script_path#\"}"
        script_path="${script_path%\"}"
        script_path="${script_path//\\\\Mac\\Home/$HOME}"
        script_path="${script_path//\\\\Mac\\Home\//$HOME/}"
        if [[ -f "$script_path" ]] && rg -q 't0027-auto-crlf|zmin-direct|zmin-stockgit|zmin-upstream' "$script_path"; then
          kill "$pid" 2>/dev/null || true
        fi
        ;;
    esac
  done
}

ps_quote() {
  local value="${1//\'/\'\'}"
  printf "'%s'" "$value"
}

run_with_timeout() {
  local timeout_seconds="$1"
  local label="$2"
  shift 2

  "$@" &
  local pid="$!"
  local timeout_marker
  timeout_marker="$(mktemp "/tmp/zmin-runner-timeout.XXXXXX")"
  (
    sleep "$timeout_seconds"
    if kill -0 "$pid" 2>/dev/null; then
      echo "${label} timed out after ${timeout_seconds}s" >&2
      : >"$timeout_marker"
      kill "$pid" 2>/dev/null || true
      sleep 2
      kill -9 "$pid" 2>/dev/null || true
    fi
  ) &
  local watchdog="$!"

  local status=0
  wait "$pid" || status="$?"
  kill "$watchdog" 2>/dev/null || true
  wait "$watchdog" 2>/dev/null || true

  if [[ -s "$timeout_marker" ]]; then
    rm -f "$timeout_marker"
    return 124
  fi
  rm -f "$timeout_marker"
  return "$status"
}

make_boot_iso() {
  require_file "$windows_iso"
  local mount
  mount="$(mktemp -d)"
  hdiutil attach -nobrowse -readonly -mountpoint "$mount" "$windows_iso" >/dev/null
  rm -f "$boot_iso"
  hdiutil makehybrid -o "$boot_iso" "$mount" \
    -udf -iso -joliet \
    -default-volume-name "CCCOMA_A64FRE_EN-US_DV9" \
    -eltorito-boot "$mount/efi/microsoft/boot/efisys_noprompt.bin" \
    -no-emul-boot \
    -ov >/dev/null
  hdiutil detach "$mount" >/dev/null
  rmdir "$mount" 2>/dev/null || true
}

make_answer_iso() {
  local work
  work="$(mktemp -d)"
  mkdir -p "$work/src"

  cat >"$work/src/autounattend.xml" <<EOF
<?xml version="1.0" encoding="utf-8"?>
<unattend xmlns="urn:schemas-microsoft-com:unattend">
  <settings pass="windowsPE">
    <component name="Microsoft-Windows-International-Core-WinPE" processorArchitecture="arm64" publicKeyToken="31bf3856ad364e35" language="neutral" versionScope="nonSxS" xmlns:wcm="http://schemas.microsoft.com/WMIConfig/2002/State" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
      <SetupUILanguage>
        <UILanguage>en-US</UILanguage>
      </SetupUILanguage>
      <InputLocale>en-US</InputLocale>
      <SystemLocale>en-US</SystemLocale>
      <UILanguage>en-US</UILanguage>
      <UserLocale>en-US</UserLocale>
    </component>
    <component name="Microsoft-Windows-Setup" processorArchitecture="arm64" publicKeyToken="31bf3856ad364e35" language="neutral" versionScope="nonSxS" xmlns:wcm="http://schemas.microsoft.com/WMIConfig/2002/State" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
      <DiskConfiguration>
        <Disk wcm:action="add">
          <DiskID>0</DiskID>
          <WillWipeDisk>true</WillWipeDisk>
          <CreatePartitions>
            <CreatePartition wcm:action="add">
              <Order>1</Order>
              <Type>EFI</Type>
              <Size>260</Size>
            </CreatePartition>
            <CreatePartition wcm:action="add">
              <Order>2</Order>
              <Type>MSR</Type>
              <Size>16</Size>
            </CreatePartition>
            <CreatePartition wcm:action="add">
              <Order>3</Order>
              <Type>Primary</Type>
              <Extend>true</Extend>
            </CreatePartition>
          </CreatePartitions>
          <ModifyPartitions>
            <ModifyPartition wcm:action="add">
              <Order>1</Order>
              <PartitionID>1</PartitionID>
              <Format>FAT32</Format>
              <Label>System</Label>
            </ModifyPartition>
            <ModifyPartition wcm:action="add">
              <Order>2</Order>
              <PartitionID>3</PartitionID>
              <Format>NTFS</Format>
              <Label>Windows</Label>
              <Letter>C</Letter>
            </ModifyPartition>
          </ModifyPartitions>
        </Disk>
      </DiskConfiguration>
      <ImageInstall>
        <OSImage>
          <InstallTo>
            <DiskID>0</DiskID>
            <PartitionID>3</PartitionID>
          </InstallTo>
          <InstallToAvailablePartition>false</InstallToAvailablePartition>
        </OSImage>
      </ImageInstall>
      <UserData>
        <AcceptEula>true</AcceptEula>
        <ProductKey>
          <Key>VK7JG-NPHTM-C97JM-9MPGT-3V66T</Key>
          <WillShowUI>Never</WillShowUI>
        </ProductKey>
      </UserData>
      <RunSynchronous>
        <RunSynchronousCommand wcm:action="add">
          <Order>1</Order>
          <Path>reg add HKLM\SYSTEM\Setup\LabConfig /v BypassTPMCheck /t REG_DWORD /d 1 /f</Path>
        </RunSynchronousCommand>
        <RunSynchronousCommand wcm:action="add">
          <Order>2</Order>
          <Path>reg add HKLM\SYSTEM\Setup\LabConfig /v BypassSecureBootCheck /t REG_DWORD /d 1 /f</Path>
        </RunSynchronousCommand>
        <RunSynchronousCommand wcm:action="add">
          <Order>3</Order>
          <Path>reg add HKLM\SYSTEM\Setup\LabConfig /v BypassRAMCheck /t REG_DWORD /d 1 /f</Path>
        </RunSynchronousCommand>
      </RunSynchronous>
	    </component>
    <component name="Microsoft-Windows-PnpCustomizationsWinPE" processorArchitecture="arm64" publicKeyToken="31bf3856ad364e35" language="neutral" versionScope="nonSxS" xmlns:wcm="http://schemas.microsoft.com/WMIConfig/2002/State" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
      <DriverPaths>
        <PathAndCredentials wcm:keyValue="1" wcm:action="add"><Path>D:\netkvm\arm64</Path></PathAndCredentials>
        <PathAndCredentials wcm:keyValue="2" wcm:action="add"><Path>E:\netkvm\arm64</Path></PathAndCredentials>
        <PathAndCredentials wcm:keyValue="3" wcm:action="add"><Path>F:\netkvm\arm64</Path></PathAndCredentials>
      </DriverPaths>
    </component>
	  </settings>
  <settings pass="specialize">
    <component name="Microsoft-Windows-Deployment" processorArchitecture="arm64" publicKeyToken="31bf3856ad364e35" language="neutral" versionScope="nonSxS" xmlns:wcm="http://schemas.microsoft.com/WMIConfig/2002/State" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
      <RunSynchronous>
        <RunSynchronousCommand wcm:action="add">
          <Order>1</Order>
          <Path>reg add HKLM\SYSTEM\CurrentControlSet\Control\BitLocker /v PreventDeviceEncryption /t REG_DWORD /d 1 /f</Path>
          <WillReboot>Never</WillReboot>
        </RunSynchronousCommand>
        <RunSynchronousCommand wcm:action="add">
          <Order>2</Order>
          <Path>cmd /q /C "FOR %I IN (A D E F G H I J K L M N O P Q R S T U V W X Y Z) DO IF EXIST %I:\prl_tg\DrvInARM64.exe %I:\prl_tg\DrvInARM64.exe -installPrlTg %I:\prl_tg\arm64\prl_tg.inf"</Path>
          <WillReboot>Never</WillReboot>
        </RunSynchronousCommand>
        <RunSynchronousCommand wcm:action="add">
          <Order>3</Order>
          <Path>cmd /q /C "FOR %I IN (A D E F G H I J K L M N O P Q R S T U V W X Y Z) DO IF EXIST %I:\IGT_ARM64.exe COPY %I:\IGT_ARM64.exe %TEMP%\IGT.exe"</Path>
          <WillReboot>Never</WillReboot>
        </RunSynchronousCommand>
        <RunSynchronousCommand wcm:action="add">
          <Order>4</Order>
          <Path>%TEMP%\IGT.exe</Path>
          <WillReboot>Never</WillReboot>
        </RunSynchronousCommand>
        <RunSynchronousCommand wcm:action="add">
          <Order>5</Order>
          <Path>reg add HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\OOBE /v DisableVoice /t REG_DWORD /d 1 /f</Path>
          <WillReboot>Never</WillReboot>
        </RunSynchronousCommand>
      </RunSynchronous>
    </component>
  </settings>
	  <settings pass="oobeSystem">
    <component name="Microsoft-Windows-International-Core" processorArchitecture="arm64" publicKeyToken="31bf3856ad364e35" language="neutral" versionScope="nonSxS" xmlns:wcm="http://schemas.microsoft.com/WMIConfig/2002/State" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
      <InputLocale>en-US</InputLocale>
      <SystemLocale>en-US</SystemLocale>
      <UILanguage>en-US</UILanguage>
      <UserLocale>en-US</UserLocale>
    </component>
    <component name="Microsoft-Windows-Shell-Setup" processorArchitecture="arm64" publicKeyToken="31bf3856ad364e35" language="neutral" versionScope="nonSxS" xmlns:wcm="http://schemas.microsoft.com/WMIConfig/2002/State" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
      <OOBE>
        <HideEULAPage>true</HideEULAPage>
        <HideLocalAccountScreen>true</HideLocalAccountScreen>
        <HideOEMRegistrationScreen>true</HideOEMRegistrationScreen>
        <HideOnlineAccountScreens>true</HideOnlineAccountScreens>
        <HideWirelessSetupInOOBE>true</HideWirelessSetupInOOBE>
        <NetworkLocation>Work</NetworkLocation>
        <ProtectYourPC>3</ProtectYourPC>
      </OOBE>
      <UserAccounts>
        <LocalAccounts>
          <LocalAccount wcm:action="add">
            <Name>${guest_user}</Name>
            <Group>Administrators</Group>
            <Password>
              <Value>${guest_pass}</Value>
              <PlainText>true</PlainText>
            </Password>
          </LocalAccount>
        </LocalAccounts>
      </UserAccounts>
      <AutoLogon>
        <Enabled>true</Enabled>
        <Username>${guest_user}</Username>
        <Password>
          <Value>${guest_pass}</Value>
          <PlainText>true</PlainText>
        </Password>
      </AutoLogon>
      <FirstLogonCommands>
        <SynchronousCommand wcm:action="add">
          <Order>1</Order>
          <CommandLine>powershell -NoProfile -ExecutionPolicy Bypass -Command "Add-WindowsCapability -Online -Name OpenSSH.Server~~~~0.0.1.0; Set-Service sshd -StartupType Automatic; Start-Service sshd; New-NetFirewallRule -Name OpenSSH-Server-In-TCP -DisplayName 'OpenSSH Server (sshd)' -Enabled True -Direction Inbound -Protocol TCP -Action Allow -LocalPort 22"</CommandLine>
        </SynchronousCommand>
      </FirstLogonCommands>
    </component>
  </settings>
</unattend>
EOF

  ditto "$tools_root/prl_tg" "$work/src/prl_tg"
  ditto "$tools_root/netkvm" "$work/src/netkvm"
  ditto "$tools_root/IGT_ARM64.exe" "$work/src/IGT_ARM64.exe"

	  rm -f "$answer_iso"
  hdiutil makehybrid -iso -joliet -default-volume-name AUTOUNATTEND -o "$answer_iso" "$work/src" >/dev/null
  rm -rf "$work"
}

create_vm() {
  require_command prlctl
  require_file "$windows_iso"
  require_file "$tools_iso"
  require_file "$tools_root/IGT_ARM64.exe"
  require_space

  if [[ ! -e "$boot_iso" ]]; then
    make_boot_iso
  fi
  require_file "$boot_iso"

  if ! vm_exists; then
    prlctl create "$vm_name" -o windows --no-hdd --dst "$vm_home"
  fi

  mkdir -p "$vm_dir"
  make_answer_iso

  prlctl set "$vm_name" \
    --cpus "$num_vcpus" \
    --memsize "$memory_mb" \
    --bios-type efi-arm64 \
    --efi-secure-boot on \
    --tpm crb \
    --startup-view window \
    --on-window-close keep-running \
    --autostop stop

  if ! prlctl list -i "$vm_name" | grep -q '^  hdd0 '; then
    prlctl set "$vm_name" --device-add hdd --type expand --size "$disk_mb" --iface nvme --alloc-policy sparse
  fi

  if prlctl list -i "$vm_name" | grep -q '^  cdrom0 '; then
    prlctl set "$vm_name" --device-set cdrom0 --image "$boot_iso" --connect
  else
    prlctl set "$vm_name" --device-add cdrom --image "$boot_iso" --connect --iface sata
  fi

  if prlctl list -i "$vm_name" | grep -q '^  cdrom1 '; then
    prlctl set "$vm_name" --device-set cdrom1 --image "$answer_iso" --connect
  else
    prlctl set "$vm_name" --device-add cdrom --image "$answer_iso" --connect --iface sata
  fi

  prlctl set "$vm_name" --device-bootorder "cdrom0 hdd0"
  status
}

status() {
  require_command prlctl
  prlctl list -a
  echo
  if vm_exists; then
    prlctl list -i "$vm_name" | sed -n '1,220p'
    echo
    du -sh "$vm_dir" 2>/dev/null || true
  fi
  df -h "$vm_home" | sed -n '1,2p'
  echo
  ps axww -o pid=,%cpu=,%mem=,rss=,command= | rg "prl_vm_app|prl_client_app|$vm_name" || true
}

stop_vm() {
  if vm_exists; then
    prlctl stop "$vm_name" --force >/dev/null 2>&1 || prlctl stop "$vm_name" --kill >/dev/null 2>&1 || true
  fi
}

copy_repo_to_guest() {
  local remote_root="$1"
  local tmp_dir archive file_list
  tmp_dir="$(mktemp -d "$HOME/.zmin-parallels-worktree.XXXXXX")"
  archive="$tmp_dir/zmin-worktree.tar.gz"
  file_list="$tmp_dir/files.list"
  (
    cd "$repo_root"
    git ls-files -z --cached --others --exclude-standard |
      while IFS= read -r -d '' path; do
        if [[ -e "$path" || -L "$path" ]]; then
          printf '%s\0' "$path"
        fi
      done >"$file_list"
    COPYFILE_DISABLE=1 tar --no-xattrs --null -T "$file_list" -czf "$archive"
  )
  prlctl exec "$vm_name" -u "$guest_user" --password "$guest_pass" powershell -NoProfile -Command "New-Item -ItemType Directory -Force -Path $(ps_quote "$remote_root") | Out-Null"
  prlctl exec "$vm_name" -u "$guest_user" --password "$guest_pass" powershell -NoProfile -Command "Copy-Item -LiteralPath $(ps_quote "\\\\Mac\\Home${archive#$HOME}") -Destination $(ps_quote "$remote_root\\zmin-worktree.tar.gz") -Force"
  rm -rf "$tmp_dir"
}

run_guest_powershell() {
  local script="$1"
  local tmp
  tmp="$(mktemp "$HOME/.zmin-parallels-script.XXXXXX")"
  mv "$tmp" "$tmp.ps1"
  tmp="$tmp.ps1"
  {
    echo "\$GuestHome = 'C:\\Users\\${guest_user}'"
    echo '$GitDir = Join-Path $GuestHome "PortableGit\cmd"'
    echo '$GitUsrBinDir = Join-Path $GuestHome "PortableGit\usr\bin"'
    echo '$GitClangDir = Join-Path $GuestHome "PortableGit\clangarm64\bin"'
    echo '$LlvmMingwDir = "\\Mac\Home\.skron-parallels-cache\llvm-mingw-20260602-ucrt-aarch64\bin"'
    echo '$CargoDir = Join-Path $GuestHome ".cargo\bin"'
    echo '$env:Path = "$GitDir;$GitUsrBinDir;$GitClangDir;$LlvmMingwDir;$CargoDir;$env:Path"'
    echo '$env:RUSTUP_TOOLCHAIN = "stable-aarch64-pc-windows-gnullvm"'
    printf '%s\n' "$script"
  } >"$tmp"
  prlctl exec "$vm_name" -u "$guest_user" --password "$guest_pass" powershell -NoProfile -ExecutionPolicy Bypass -Command "& \\\\Mac\\Home${tmp#$HOME}"
  rm -f "$tmp"
}

bootstrap_guest() {
  local remote_root="C:\\Users\\${guest_user}\\zmin-bootstrap"
  copy_repo_to_guest "$remote_root"
  run_guest_powershell "\$ErrorActionPreference = 'Stop'
Set-Location $(ps_quote "$remote_root")
tar -xzf .\\zmin-worktree.tar.gz
.\\tools\\windows-local-bootstrap.ps1 -InstallMissingTools"
}

validate_guest() {
  local mode="${1:-targeted}"
  local test_file="${2:-git_cli_failure_compat}"
  local test_case="${3:-invalid_option_combinations_match_stock_git_failures}"
  local job="zmin-$(date -u +%Y%m%dT%H%M%SZ)-$$"
  local remote_root="C:\\Users\\${guest_user}\\${job}"
  local no_fmt_arg=""
  if [[ "${ZMIN_WINDOWS_VALIDATE_NO_FMT:-0}" == "1" ]]; then
    no_fmt_arg=" -NoFmt"
  fi
  local build_profile_arg=""
  if [[ -n "${ZMIN_WINDOWS_VALIDATE_BUILD_PROFILE:-}" ]]; then
    build_profile_arg=" -BuildProfile $(ps_quote "$ZMIN_WINDOWS_VALIDATE_BUILD_PROFILE")"
  fi
  copy_repo_to_guest "$remote_root"
  run_guest_powershell "\$ErrorActionPreference = 'Stop'
	Set-Location $(ps_quote "$remote_root")
	tar -xzf .\\zmin-worktree.tar.gz
	\$env:CARGO_TARGET_DIR = 'C:\\Users\\${guest_user}\\zmin-target'
	\$env:CARGO_BUILD_JOBS = '1'
	\$env:CARGO_TERM_COLOR = 'never'
		.\\tools\\windows-native-validate.ps1 -Mode $(ps_quote "$mode") -TestFile $(ps_quote "$test_file") -Case $(ps_quote "$test_case")${no_fmt_arg}${build_profile_arg}"
}

benchmark_guest() {
  local repeats="${1:-5}"
  local ops="${2:-${ZMIN_WINDOWS_BENCH_OPS:-}}"
  local job="zmin-bench-$(date -u +%Y%m%dT%H%M%SZ)-$$"
  local remote_root="C:\\Users\\${guest_user}\\${job}"
  local out_dir="C:\\Users\\${guest_user}\\${job}-out"
  local trace_arg=""
  local ssh_trace_arg=""
  local ops_arg=""
  local ratio_gate_arg=""
  if [[ "${ZMIN_WINDOWS_BENCH_PHASE_TRACE:-0}" == "1" ]]; then
    trace_arg=" -ZminPhaseTraceDir $(ps_quote "$out_dir\\phase-traces")"
    if [[ "${ZMIN_WINDOWS_BENCH_PHASE_TRACE_ONLY:-0}" == "1" ]]; then
      trace_arg+=" -SkipCheckoutPhaseTrace"
    fi
  fi
  if [[ "${ZMIN_WINDOWS_BENCH_SSH_TRACE:-0}" == "1" ]]; then
    ssh_trace_arg=" -SshTraceDir $(ps_quote "$out_dir\\ssh-traces")"
  fi
  local ssh_packet_trace_arg=""
  if [[ "${ZMIN_WINDOWS_BENCH_SSH_PACKET_TRACE:-0}" == "1" ]]; then
    ssh_packet_trace_arg=" -SshPacketTraceDir $(ps_quote "$out_dir\\ssh-packet-traces")"
  fi
  if [[ -n "$ops" ]]; then
    ops_arg=" -Ops $(ps_quote "$ops")"
  fi
  if [[ -n "${ZMIN_WINDOWS_BENCH_MAX_ZMIN_VS_GIT_MEAN_RATIO:-}" ]]; then
    ratio_gate_arg+=" -MaxZminVsGitMeanRatio ${ZMIN_WINDOWS_BENCH_MAX_ZMIN_VS_GIT_MEAN_RATIO}"
  fi
  if [[ -n "${ZMIN_WINDOWS_BENCH_MAX_ZMIN_VS_GIT_MEDIAN_RATIO:-}" ]]; then
    ratio_gate_arg+=" -MaxZminVsGitMedianRatio ${ZMIN_WINDOWS_BENCH_MAX_ZMIN_VS_GIT_MEDIAN_RATIO}"
  fi
  if [[ -n "${ZMIN_WINDOWS_BENCH_MAX_ZMIN_VS_GIT_PAIR_MEDIAN_RATIO:-}" ]]; then
    ratio_gate_arg+=" -MaxZminVsGitPairMedianRatio ${ZMIN_WINDOWS_BENCH_MAX_ZMIN_VS_GIT_PAIR_MEDIAN_RATIO}"
  fi
  if [[ -n "${ZMIN_WINDOWS_BENCH_MAX_ZMIN_VS_GIX_MEAN_RATIO:-}" ]]; then
    ratio_gate_arg+=" -MaxZminVsGixMeanRatio ${ZMIN_WINDOWS_BENCH_MAX_ZMIN_VS_GIX_MEAN_RATIO}"
  fi
  if [[ -n "${ZMIN_WINDOWS_BENCH_MAX_ZMIN_VS_GIX_MEDIAN_RATIO:-}" ]]; then
    ratio_gate_arg+=" -MaxZminVsGixMedianRatio ${ZMIN_WINDOWS_BENCH_MAX_ZMIN_VS_GIX_MEDIAN_RATIO}"
  fi
  if [[ -n "${ZMIN_WINDOWS_BENCH_MAX_ZMIN_VS_GIX_PAIR_MEDIAN_RATIO:-}" ]]; then
    ratio_gate_arg+=" -MaxZminVsGixPairMedianRatio ${ZMIN_WINDOWS_BENCH_MAX_ZMIN_VS_GIX_PAIR_MEDIAN_RATIO}"
  fi
  copy_repo_to_guest "$remote_root"
  run_guest_powershell "\$ErrorActionPreference = 'Stop'
	Set-Location $(ps_quote "$remote_root")
	tar -xzf .\\zmin-worktree.tar.gz
	\$env:CARGO_TARGET_DIR = 'C:\\Users\\${guest_user}\\zmin-target'
	.\\tools\\windows-native-benchmark.ps1 -Repeats ${repeats} -OutDir $(ps_quote "$out_dir")${trace_arg}${ssh_trace_arg}${ssh_packet_trace_arg}${ops_arg}${ratio_gate_arg}"
}

http_benchmark_guest() {
  local repeats="${1:-3}"
  local job="zmin-http-bench-$(date -u +%Y%m%dT%H%M%SZ)-$$"
  local remote_root="C:\\Users\\${guest_user}\\${job}"
  local out_dir="C:\\Users\\${guest_user}\\${job}-out"
  copy_repo_to_guest "$remote_root"
  run_guest_powershell "\$ErrorActionPreference = 'Stop'
	Set-Location $(ps_quote "$remote_root")
	tar -xzf .\\zmin-worktree.tar.gz
	\$env:CARGO_TARGET_DIR = 'C:\\Users\\${guest_user}\\zmin-target'
	.\\tools\\windows-native-http-benchmark.ps1 -Repeats ${repeats} -OutDir $(ps_quote "$out_dir")
	if (\$LASTEXITCODE -ne 0) { throw \"Windows smart HTTP benchmark failed with exit code \$LASTEXITCODE\" }"
}

extended_guest() {
  local mode="${1:-quick}"
  local repeats="${2:-0}"
  local job="zmin-extended-$(date -u +%Y%m%dT%H%M%SZ)-$$"
  local remote_root="C:\\Users\\${guest_user}\\${job}"
  local build_profile_arg=""
  if [[ -n "${ZMIN_WINDOWS_EXTENDED_BUILD_PROFILE:-}" ]]; then
    build_profile_arg=" -BuildProfile $(ps_quote "$ZMIN_WINDOWS_EXTENDED_BUILD_PROFILE")"
  fi
  copy_repo_to_guest "$remote_root"
  run_guest_powershell "\$ErrorActionPreference = 'Stop'
	Set-Location $(ps_quote "$remote_root")
	tar -xzf .\\zmin-worktree.tar.gz
	\$env:CARGO_TARGET_DIR = 'C:\\Users\\${guest_user}\\zmin-target'
	.\\tools\\windows-native-extended-compat.ps1 -Mode $(ps_quote "$mode") -BenchmarkRepeats ${repeats}${build_profile_arg}"
}

build_release_guest() {
  local job="zmin-build-$(date -u +%Y%m%dT%H%M%SZ)-$$"
  local remote_root="C:\\Users\\${guest_user}\\${job}"
  local out_dir="C:\\Users\\${guest_user}\\${job}-out"
  copy_repo_to_guest "$remote_root"
  run_guest_powershell "\$ErrorActionPreference = 'Stop'
	Set-Location $(ps_quote "$remote_root")
	tar -xzf .\\zmin-worktree.tar.gz
	\$env:CARGO_TARGET_DIR = 'C:\\Users\\${guest_user}\\zmin-target'
	New-Item -ItemType Directory -Force -Path $(ps_quote "$out_dir") | Out-Null
	Get-Process cargo,rustc,bash,sh,expr,uniq,cp,git,zmin -ErrorAction SilentlyContinue | Stop-Process -Force
	Remove-Item -Force 'C:\\Users\\${guest_user}\\zmin-target\\release\\zmin.exe' -ErrorAction SilentlyContinue
	\$StdoutLog = Join-Path $(ps_quote "$out_dir") 'cargo-build.stdout.log'
	\$StderrLog = Join-Path $(ps_quote "$out_dir") 'cargo-build.stderr.log'
	\$CargoExe = Join-Path \$CargoDir 'cargo.exe'
	\$env:CARGO_BUILD_JOBS = '1'
	\$env:CARGO_TERM_COLOR = 'never'
	\$Process = Start-Process -FilePath \$CargoExe -ArgumentList @('build', '-p', 'zmin-cli', '--release', '--bin', 'zmin') -WorkingDirectory (Get-Location).Path -RedirectStandardOutput \$StdoutLog -RedirectStandardError \$StderrLog -NoNewWindow -PassThru
	while (-not \$Process.WaitForExit(20000)) {
		Write-Host \"release build still running; stdout=\$StdoutLog stderr=\$StderrLog\"
	}
	\$Process.WaitForExit()
	\$Process.Refresh()
	\$StdoutText = if (Test-Path \$StdoutLog) { Get-Content -LiteralPath \$StdoutLog -Raw } else { '' }
	\$StderrText = if (Test-Path \$StderrLog) { Get-Content -LiteralPath \$StderrLog -Raw } else { '' }
	if (\$StdoutText) { Write-Host \$StdoutText }
	if (\$StderrText) { Write-Host \$StderrText }
	\$ZminExe = 'C:\\Users\\${guest_user}\\zmin-target\\release\\zmin.exe'
	\$BuildExitCode = \$Process.ExitCode
	if (\$null -eq \$BuildExitCode -and (Test-Path \$ZminExe)) {
		\$BuildExitCode = 0
	}
	if (\$BuildExitCode -ne 0) { throw (\"release build failed with exit code \" + \$BuildExitCode) }
	Get-Item \$ZminExe | Select-Object Length,LastWriteTime"
}

upstream_guest() {
  local mode="${1:-quick}"
  local job="zmin-upstream-$(date -u +%Y%m%dT%H%M%SZ)-$$"
  local remote_root="C:\\Users\\${guest_user}\\${job}"
  local out_dir="C:\\Users\\${guest_user}\\${job}-out"
  local cargo_profile="${ZMIN_PARALLELS_UPSTREAM_CARGO_PROFILE:-release}"
  local zmin_exe_windows="C:\\Users\\${guest_user}\\zmin-target\\${cargo_profile}\\zmin.exe"
  local zmin_bin_posix="/c/Users/${guest_user}/zmin-target/${cargo_profile}/zmin.exe"
  local cargo_target_posix="/c/Users/${guest_user}/zmin-target"
  local out_dir_posix="/c/Users/${guest_user}/${job}-out"
  local remote_root_posix="/c/Users/${guest_user}/${job}"
  local stdout_log_posix="${out_dir_posix}/upstream-runner.stdout.log"
  local stderr_log_posix="${out_dir_posix}/upstream-runner.stderr.log"
  local exit_file_posix="${out_dir_posix}/upstream-runner.exit"
  local exit_file_windows="${out_dir}\\upstream-runner.exit"
  local build_args_powershell
  local build_command_bash
  if [[ "$cargo_profile" == "release" ]]; then
    build_args_powershell="@('build', '-p', 'zmin-cli', '--release', '--bin', 'zmin')"
    build_command_bash="cargo build --manifest-path $(printf '%q' "$remote_root_posix/Cargo.toml") --release -p zmin-cli --bin zmin"
  else
    build_args_powershell="@('build', '-p', 'zmin-cli', '--profile', $(ps_quote "$cargo_profile"), '--bin', 'zmin')"
    build_command_bash="cargo build --manifest-path $(printf '%q' "$remote_root_posix/Cargo.toml") --profile $(printf '%q' "$cargo_profile") -p zmin-cli --bin zmin"
  fi
  local upstream_run_script
  upstream_run_script="#!/usr/bin/env bash
set -euo pipefail
cd $(printf '%q' "$remote_root_posix")
exec >$(printf '%q' "$stdout_log_posix") 2>$(printf '%q' "$stderr_log_posix")
write_upstream_exit() {
  local rc=\$?
  printf '%s\n' \"\$rc\" >$(printf '%q' "$exit_file_posix")
}
trap write_upstream_exit EXIT
export PATH=/c/Users/${guest_user}/.cargo/bin://Mac/Home/.skron-parallels-cache/llvm-mingw-20260602-ucrt-aarch64/bin:/clangarm64/bin:/usr/bin:\$PATH
export CARGO_TARGET_DIR=$(printf '%q' "$cargo_target_posix")
export CARGO_BUILD_JOBS=$(printf '%q' "${ZMIN_PARALLELS_CARGO_BUILD_JOBS:-2}")
export RUSTUP_TOOLCHAIN=stable-aarch64-pc-windows-gnullvm
export ZMIN_BIN=$(printf '%q' "$zmin_bin_posix")
export ZMIN_UPSTREAM_OUT_DIR=$(printf '%q' "$out_dir_posix")
export ZMIN_PARALLELS_UPSTREAM_REUSE_BINARY=$(printf '%q' "${ZMIN_PARALLELS_UPSTREAM_REUSE_BINARY:-0}")"
  if [[ -n "${ZMIN_UPSTREAM_TEST_FLAGS:-}" ]]; then
    upstream_run_script+="
export ZMIN_UPSTREAM_TEST_FLAGS=$(printf '%q' "$ZMIN_UPSTREAM_TEST_FLAGS")"
  fi
  if [[ -n "${ZMIN_UPSTREAM_TEST_LIST:-}" ]]; then
    upstream_run_script+="
export ZMIN_UPSTREAM_TEST_LIST=$(printf '%q' "$ZMIN_UPSTREAM_TEST_LIST")"
  fi
  if [[ -n "${ZMIN_UPSTREAM_SKIP_UNSUPPORTED_REFTABLE:-}" ]]; then
    upstream_run_script+="
export ZMIN_UPSTREAM_SKIP_UNSUPPORTED_REFTABLE=$(printf '%q' "$ZMIN_UPSTREAM_SKIP_UNSUPPORTED_REFTABLE")"
  fi
  if [[ -n "${ZMIN_UPSTREAM_ALLOW_FAILURES:-}" ]]; then
    upstream_run_script+="
export ZMIN_UPSTREAM_ALLOW_FAILURES=$(printf '%q' "$ZMIN_UPSTREAM_ALLOW_FAILURES")"
  fi
  if [[ -n "${ZMIN_UPSTREAM_BOUNDED_RUN:-}" ]]; then
    upstream_run_script+="
export ZMIN_UPSTREAM_BOUNDED_RUN=$(printf '%q' "$ZMIN_UPSTREAM_BOUNDED_RUN")"
  fi
  if [[ -n "${ZMIN_UPSTREAM_STOCK_GIT_CONTROL:-}" ]]; then
    upstream_run_script+="
export ZMIN_UPSTREAM_STOCK_GIT_CONTROL=$(printf '%q' "$ZMIN_UPSTREAM_STOCK_GIT_CONTROL")"
  fi
  upstream_run_script+="
if [[ \"\${ZMIN_UPSTREAM_STOCK_GIT_CONTROL:-0}\" != \"1\" ]]; then
  if [[ \"\${ZMIN_PARALLELS_UPSTREAM_REUSE_BINARY:-0}\" != \"1\" || ! -x \"\$ZMIN_BIN\" ]]; then
    rm -f \"\$ZMIN_BIN\"
    $build_command_bash
  fi
fi
set +e
./tools/git-upstream-compat-suite.sh $(printf '%q' "$mode")
rc=\$?
printf '%s\n' \"\$rc\" >$(printf '%q' "$exit_file_posix")
exit \"\$rc\"
"
  stop_stale_host_guest_exec_sessions
  copy_repo_to_guest "$remote_root"
  run_guest_powershell "\$ErrorActionPreference = 'Stop'
	Set-Location $(ps_quote "$remote_root")
	tar -xzf .\\zmin-worktree.tar.gz
	\$env:CARGO_TARGET_DIR = 'C:\\Users\\${guest_user}\\zmin-target'
	New-Item -ItemType Directory -Force -Path $(ps_quote "$out_dir") | Out-Null
	Remove-Item -Force $(ps_quote "$exit_file_windows") -ErrorAction SilentlyContinue
		\$ActiveUpstreamTasks = @(Get-ScheduledTask | Where-Object { \$_.TaskName -like 'ZminUpstream-*' -and \$_.State -eq 'Running' })
		\$ActiveUpstreamProcesses = @(Get-CimInstance Win32_Process | Where-Object {
			\$_.Name -in @('bash.exe', 'sh.exe', 'git.exe', 'zmin.exe', 'cargo.exe', 'rustc.exe') -and
			\$_.CommandLine -like '*zmin-upstream-*'
		})
		if (\$ActiveUpstreamTasks.Count -gt 0 -or \$ActiveUpstreamProcesses.Count -gt 0) {
			\$TaskNames = (@(\$ActiveUpstreamTasks | ForEach-Object { \$_.TaskName }) -join ',')
			\$ProcessIds = (@(\$ActiveUpstreamProcesses | ForEach-Object { [string]\$_.ProcessId }) -join ',')
			throw \"refusing to start upstream run while another upstream run is active; tasks=\$TaskNames processes=\$ProcessIds; run cleanup after confirming stale state\"
		}
		Get-ScheduledTask | Where-Object {
			(\$_.TaskName -like 'ZminUpstream-*' -or \$_.TaskName -like 'ZminDiag*' -or \$_.TaskName -like 'ZminProbe*') -and
			\$_.State -ne 'Running'
		} | ForEach-Object {
			Unregister-ScheduledTask -TaskName \$_.TaskName -Confirm:\$false -ErrorAction SilentlyContinue
		}
		if ('${ZMIN_UPSTREAM_STOCK_GIT_CONTROL:-0}' -ne '1' -and '${ZMIN_PARALLELS_UPSTREAM_DETACH:-0}' -ne '1') {
			if ('${ZMIN_PARALLELS_UPSTREAM_REUSE_BINARY:-0}' -ne '1' -or -not (Test-Path $(ps_quote "$zmin_exe_windows"))) {
				Remove-Item -Force $(ps_quote "$zmin_exe_windows") -ErrorAction SilentlyContinue
				\$BuildStdoutLog = Join-Path $(ps_quote "$out_dir") 'upstream-build.stdout.log'
				\$BuildStderrLog = Join-Path $(ps_quote "$out_dir") 'upstream-build.stderr.log'
				\$CargoExe = Join-Path \$CargoDir 'cargo.exe'
				\$env:CARGO_BUILD_JOBS = '1'
				\$env:CARGO_TERM_COLOR = 'never'
				\$BuildProcess = Start-Process -FilePath \$CargoExe -ArgumentList ${build_args_powershell} -WorkingDirectory (Get-Location).Path -RedirectStandardOutput \$BuildStdoutLog -RedirectStandardError \$BuildStderrLog -NoNewWindow -PassThru
				while (-not \$BuildProcess.WaitForExit(20000)) {
					Write-Host \"upstream runner build still running; stdout=\$BuildStdoutLog stderr=\$BuildStderrLog\"
				}
				\$BuildProcess.WaitForExit()
				\$BuildProcess.Refresh()
				\$BuildStdoutText = if (Test-Path \$BuildStdoutLog) { Get-Content -LiteralPath \$BuildStdoutLog -Raw } else { '' }
				\$BuildStderrText = if (Test-Path \$BuildStderrLog) { Get-Content -LiteralPath \$BuildStderrLog -Raw } else { '' }
				if (\$BuildStdoutText) { Write-Host \$BuildStdoutText }
				if (\$BuildStderrText) { Write-Host \$BuildStderrText }
				\$BuildExitCode = \$BuildProcess.ExitCode
				if (\$null -eq \$BuildExitCode -and (Test-Path $(ps_quote "$zmin_exe_windows"))) {
					\$BuildExitCode = 0
				}
				if (\$BuildExitCode -ne 0) {
					throw (\"upstream runner build failed with exit code \" + \$BuildExitCode)
				}
			}
		}
		if ('${ZMIN_PARALLELS_UPSTREAM_SKIP_PREFLIGHT:-0}' -ne '1') {
			.\\tools\\windows-native-extended-compat.ps1 -Mode quick -SkipBenchmark
		}
	\$BashExe = Join-Path \$GuestHome 'PortableGit\\bin\\bash.exe'
	\$RunScriptPath = Join-Path (Get-Location).Path 'zmin-upstream-run.sh'
	\$StdoutLog = Join-Path $(ps_quote "$out_dir") 'upstream-runner.stdout.log'
	\$StderrLog = Join-Path $(ps_quote "$out_dir") 'upstream-runner.stderr.log'
	\$RunnerScript = @'
$upstream_run_script
'@
	[System.IO.File]::WriteAllText(\$RunScriptPath, \$RunnerScript, [System.Text.Encoding]::ASCII)
	\$TaskName = $(ps_quote "ZminUpstream-${job}")
	Unregister-ScheduledTask -TaskName \$TaskName -Confirm:\$false -ErrorAction SilentlyContinue
	\$Action = New-ScheduledTaskAction -Execute \$BashExe -Argument ('\"' + \$RunScriptPath + '\"') -WorkingDirectory (Get-Location).Path
	\$Trigger = New-ScheduledTaskTrigger -Once -At (Get-Date).AddYears(1)
	Register-ScheduledTask -TaskName \$TaskName -Action \$Action -Trigger \$Trigger -Force | Out-Null
	try {
		Start-ScheduledTask -TaskName \$TaskName
		if ('${ZMIN_PARALLELS_UPSTREAM_DETACH:-0}' -eq '1') {
			Start-Sleep -Seconds 2
			\$Task = Get-ScheduledTask -TaskName \$TaskName -ErrorAction SilentlyContinue
			\$TaskState = if (\$Task) { \$Task.State } else { 'Missing' }
			Write-Host \"detached upstream Git compatibility audit task=\$TaskName state=\$TaskState out=$(ps_quote "$out_dir") stdout=\$StdoutLog stderr=\$StderrLog\"
			return
		}
		while (-not (Test-Path $(ps_quote "$exit_file_windows"))) {
			Start-Sleep -Seconds 20
			\$Task = Get-ScheduledTask -TaskName \$TaskName -ErrorAction SilentlyContinue
			\$TaskState = if (\$Task) { \$Task.State } else { 'Missing' }
			Write-Host \"upstream Git compatibility audit still running; stdout=\$StdoutLog stderr=\$StderrLog state=\$TaskState\"
			if (Test-Path $(ps_quote "$exit_file_windows")) {
				break
			}
			\$RunnerProcess = Get-CimInstance Win32_Process | Where-Object {
				\$_.Name -in @('bash.exe', 'sh.exe') -and \$_.CommandLine -like (\"*\" + \$RunScriptPath + \"*\")
			}
			if (-not \$RunnerProcess) {
				Start-Sleep -Seconds 2
				if (Test-Path $(ps_quote "$exit_file_windows")) {
					break
				}
				\$RunnerProcess = Get-CimInstance Win32_Process | Where-Object {
					\$_.Name -in @('bash.exe', 'sh.exe') -and \$_.CommandLine -like (\"*\" + \$RunScriptPath + \"*\")
				}
				if (-not \$RunnerProcess) {
					\$TaskInfo = Get-ScheduledTaskInfo -TaskName \$TaskName -ErrorAction SilentlyContinue
					\$LastTaskResult = if (\$TaskInfo) { \$TaskInfo.LastTaskResult } else { 'unknown' }
					throw \"upstream Git compatibility audit stopped before writing $(ps_quote "$exit_file_windows") state=\$TaskState lastTaskResult=\$LastTaskResult\"
				}
			}
		}
		\$StdoutText = if (Test-Path \$StdoutLog) { Get-Content -LiteralPath \$StdoutLog -Raw } else { '' }
		\$StderrText = if (Test-Path \$StderrLog) { Get-Content -LiteralPath \$StderrLog -Raw } else { '' }
		if (\$StdoutText) { Write-Host \$StdoutText }
		if (\$StderrText) { Write-Host \$StderrText }
		\$ExitCode = [int](Get-Content $(ps_quote "$exit_file_windows"))
		if (\$ExitCode -ne 0) { throw (\"upstream Git compatibility audit failed with exit code \" + \$ExitCode) }
		} finally {
			if (Test-Path $(ps_quote "$exit_file_windows")) {
				Stop-ScheduledTask -TaskName \$TaskName -ErrorAction SilentlyContinue
				Unregister-ScheduledTask -TaskName \$TaskName -Confirm:\$false -ErrorAction SilentlyContinue
				Get-Process bash,sh,expr,uniq,cp,git,zmin,cargo,rustc -ErrorAction SilentlyContinue | Stop-Process -Force
			} else {
				\$Task = Get-ScheduledTask -TaskName \$TaskName -ErrorAction SilentlyContinue
				\$TaskState = if (\$Task) { \$Task.State } else { 'Missing' }
				\$RunnerProcess = Get-CimInstance Win32_Process | Where-Object {
					\$_.Name -in @('bash.exe', 'sh.exe') -and \$_.CommandLine -like (\"*\" + \$RunScriptPath + \"*\")
				}
				if (\$TaskState -eq 'Running' -and \$RunnerProcess) {
					Write-Host \"upstream Git compatibility audit still owns running scheduled task \$TaskName; leaving it running because no exit sentinel exists yet\"
				} else {
					Write-Host \"upstream Git compatibility audit stopped without exit sentinel; cleaning task \$TaskName state=\$TaskState\"
					Stop-ScheduledTask -TaskName \$TaskName -ErrorAction SilentlyContinue
					Unregister-ScheduledTask -TaskName \$TaskName -Confirm:\$false -ErrorAction SilentlyContinue
					Get-Process bash,sh,expr,uniq,cp,git,zmin,cargo,rustc -ErrorAction SilentlyContinue | Stop-Process -Force
				}
			}
		}"
}

upstream_poll_guest() {
  local out_dir="${1:-}"
  if [[ -z "$out_dir" ]]; then
    echo "usage: tools/parallels-windows-runner.sh upstream-poll OUT_DIR" >&2
    exit 2
  fi
  run_guest_powershell "\$ErrorActionPreference = 'Stop'
	\$OutDir = $(ps_quote "$out_dir")
	\$Leaf = Split-Path -Leaf \$OutDir
	\$Job = if (\$Leaf.EndsWith('-out')) { \$Leaf.Substring(0, \$Leaf.Length - 4) } else { \$Leaf }
	\$TaskName = 'ZminUpstream-' + \$Job
	\$ExitFile = Join-Path \$OutDir 'upstream-runner.exit'
	\$StdoutLog = Join-Path \$OutDir 'upstream-runner.stdout.log'
	\$StderrLog = Join-Path \$OutDir 'upstream-runner.stderr.log'
	\$Summary = Join-Path \$OutDir 'summary.tsv'
	\$Task = Get-ScheduledTask -TaskName \$TaskName -ErrorAction SilentlyContinue
	\$TaskState = if (\$Task) { \$Task.State } else { 'Missing' }
	Write-Host \"upstream poll task=\$TaskName state=\$TaskState out=\$OutDir\"
	if (Test-Path \$ExitFile) {
		\$StdoutText = if (Test-Path \$StdoutLog) { Get-Content -LiteralPath \$StdoutLog -Raw } else { '' }
		\$StderrText = if (Test-Path \$StderrLog) { Get-Content -LiteralPath \$StderrLog -Raw } else { '' }
		if (\$StdoutText) { Write-Host \$StdoutText }
		if (\$StderrText) { Write-Host \$StderrText }
		if (Test-Path \$Summary) { Get-Content -LiteralPath \$Summary -Raw }
		\$ExitCode = [int](Get-Content \$ExitFile)
		Stop-ScheduledTask -TaskName \$TaskName -ErrorAction SilentlyContinue
		Unregister-ScheduledTask -TaskName \$TaskName -Confirm:\$false -ErrorAction SilentlyContinue
		Get-Process bash,sh,expr,uniq,cp,git,zmin,cargo,rustc -ErrorAction SilentlyContinue | Stop-Process -Force
		exit \$ExitCode
	}
	if (Test-Path \$StdoutLog) { Get-Content -LiteralPath \$StdoutLog -Tail 20 }
	if (Test-Path \$StderrLog) { Get-Content -LiteralPath \$StderrLog -Tail 40 }
	if (\$TaskState -ne 'Running') {
		Write-Host \"upstream poll found no exit sentinel; artifact inventory:\"
		if (Test-Path \$OutDir) {
			Get-ChildItem -LiteralPath \$OutDir -Force |
				Select-Object Name,Length,LastWriteTime |
				Format-Table -AutoSize | Out-String -Width 200
		}
		if (Test-Path \$Summary) {
			Write-Host \"upstream poll summary tail:\"
			Get-Content -LiteralPath \$Summary -Tail 20
		}
		Get-ChildItem -LiteralPath \$OutDir -Filter '*.log' -ErrorAction SilentlyContinue |
			Where-Object { \$_.Length -eq 0 } |
			ForEach-Object { Write-Host (\"zero-byte log: \" + \$_.Name) }
	}
	Get-Process bash,sh,expr,uniq,cp,git,zmin,cargo,rustc -ErrorAction SilentlyContinue | Select-Object ProcessName,Id,CPU,StartTime
	exit 3"
}

cleanup_guest_artifacts() {
  if ! vm_exists; then
    return 0
  fi

  stop_stale_host_guest_exec_sessions
  local cleanup_script="\$ErrorActionPreference = 'SilentlyContinue'
\$Tasks = @(Get-ScheduledTask | Where-Object { \$_.TaskName -like 'ZminUpstream-*' -or \$_.TaskName -like 'ZminDiag*' -or \$_.TaskName -like 'ZminProbe*' })
\$Tasks | ForEach-Object {
  Stop-ScheduledTask -TaskName \$_.TaskName -ErrorAction SilentlyContinue
  Unregister-ScheduledTask -TaskName \$_.TaskName -Confirm:\$false -ErrorAction SilentlyContinue
}
\$ProcessNames = @('git','git-daemon','bash','sh','expr','uniq','cp','cargo','rustc','zmin')
\$Processes = @(Get-Process \$ProcessNames -ErrorAction SilentlyContinue)
\$Processes | Stop-Process -Force -ErrorAction SilentlyContinue
\$Roots = @(Get-ChildItem C:\\Users\\${guest_user} -Directory -ErrorAction SilentlyContinue | Where-Object { \$_.Name -match '^zmin-(20[0-9]{6}T|bench-|bootstrap\$)|^daemon-' })
\$Roots | Remove-Item -Recurse -Force -ErrorAction SilentlyContinue
\$TempRoots = @(Get-ChildItem \$env:TEMP -Directory -Filter 'zmin-*' -ErrorAction SilentlyContinue)
\$TempRoots | Remove-Item -Recurse -Force -ErrorAction SilentlyContinue
Write-Host ('guest cleanup complete tasks={0} procs={1} roots={2} temp_roots={3}' -f \$Tasks.Count,\$Processes.Count,\$Roots.Count,\$TempRoots.Count)"
  run_with_timeout "$guest_exec_timeout_seconds" "guest cleanup" \
    prlctl exec "$vm_name" -u "$guest_user" --password "$guest_pass" \
      powershell -NoProfile -ExecutionPolicy Bypass -Command "$cleanup_script" || {
        echo "warning: guest cleanup did not complete; inspect guest state before Windows validation" >&2
      }
}

cmd="${1:-}"
case "$cmd" in
  status)
    status
    ;;
  stop)
    stop_vm
    status
    ;;
  create)
    create_vm
    ;;
  start)
    require_command prlctl
    prlctl start "$vm_name"
    if [[ "${2:-gui}" == "headless" ]]; then
      prlctl set "$vm_name" --startup-view headless >/dev/null
    fi
    ;;
  screenshot)
    prlctl capture "$vm_name" --file "${2:-/tmp/zmin-parallels-runner.png}"
    ;;
  tools)
    prlctl exec "$vm_name" -u "$guest_user" --password "$guest_pass" cmd /c ver
    ;;
  guest)
    shift
    prlctl exec "$vm_name" -u "$guest_user" --password "$guest_pass" "$@"
    ;;
  bootstrap)
    bootstrap_guest
    ;;
  validate)
    validate_guest "${2:-targeted}" "${3:-git_cli_failure_compat}" "${4:-invalid_option_combinations_match_stock_git_failures}"
    ;;
  extended)
    extended_guest "${2:-quick}" "${3:-0}"
    ;;
  upstream)
    upstream_guest "${2:-quick}"
    ;;
  upstream-fast)
    ZMIN_PARALLELS_UPSTREAM_REUSE_BINARY=1 \
      ZMIN_PARALLELS_UPSTREAM_SKIP_PREFLIGHT=1 \
      ZMIN_PARALLELS_UPSTREAM_DETACH=1 \
      upstream_guest "${2:-quick}"
    ;;
  upstream-compat)
    ZMIN_PARALLELS_UPSTREAM_CARGO_PROFILE=compat \
      ZMIN_PARALLELS_UPSTREAM_SKIP_PREFLIGHT=1 \
      ZMIN_PARALLELS_UPSTREAM_DETACH=1 \
      upstream_guest "${2:-quick}"
    ;;
  upstream-poll)
    upstream_poll_guest "${2:-}"
    ;;
  build-release)
    build_release_guest
    ;;
  benchmark)
    benchmark_guest "${2:-5}" "${3:-${ZMIN_WINDOWS_BENCH_OPS:-}}"
    ;;
  http-benchmark)
    http_benchmark_guest "${2:-3}"
    ;;
  cleanup)
    cleanup_guest_artifacts
    rm -rf /tmp/zmin-parallels-* /tmp/prlctl-* 2>/dev/null || true
    rm -f "$boot_iso" 2>/dev/null || true
    status
    ;;
  destroy)
    stop_vm
    if vm_exists; then
      prlctl delete "$vm_name"
    fi
    rm -rf "$vm_dir"
    ;;
  *)
    usage
    exit 2
    ;;
esac
