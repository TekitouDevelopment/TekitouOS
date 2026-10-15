
@echo off
setlocal
cd /d "%~dp0"

echo [1/4] Building bootloader...
cargo build -p bootloader --target x86_64-unknown-uefi --release
if errorlevel 1 exit /b 1




echo [2/4] Building kernel...
cargo build -p kernel --target x86_64-unknown-none --release
if errorlevel 1 exit /b 1






echo [3/4] Preparing EFI files...

wsl.exe --exec bash /mnt/c/Users/1510ty/TekitouDevelopment/TekitouOS/sign_bootloader.sh
if errorlevel 1 exit /b 1

copy /Y target\x86_64-unknown-none\release\kernel esp\kernel.elf
if errorlevel 1 exit /b 1

echo [4/4] Starting QEMU...
"C:\Program Files\qemu\qemu-system-x86_64.exe" ^
    -machine q35,smm=on ^
    -global driver=cfi.pflash01,property=secure,value=on ^
    -drive "if=pflash,format=raw,unit=0,file=esp\edk2-x86_64-secure-code.fd,readonly=on" ^
    -drive "if=pflash,format=raw,unit=1,file=esp\edk2-i386-vars.fd" ^
    -drive "format=raw,file=fat:rw:esp"

endlocal
