
@echo off
setlocal
cd /d "%~dp0"

echo [1/4] Building bootloader...
cargo build -p bootloader --target x86_64-unknown-uefi --release
if errorlevel 1 exit /b 1

echo [2/4] Building kernel...
cargo build -p kernel --target x86_64-unknown-none --release
if errorlevel 1 exit /b 1

echo [3/4] Copying EFI files...
copy /Y target\x86_64-unknown-uefi\release\bootloader.efi esp\EFI\BOOT\BOOTX64.EFI
if errorlevel 1 exit /b 1

copy /Y target\x86_64-unknown-none\release\kernel esp\kernel.elf
if errorlevel 1 exit /b 1

echo [4/4] Starting QEMU...
"C:\Program Files\qemu\qemu-system-x86_64.exe" ^
    -drive "if=pflash,format=raw,unit=0,file=esp/edk2-x86_64-code.fd,readonly=on" ^
    -drive "format=raw,file=fat:rw:esp"

endlocal
