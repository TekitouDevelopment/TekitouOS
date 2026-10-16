echo [2/7] Building testapp1...
cargo --config "target.x86_64-unknown-none.rustflags=[]" rustc --manifest-path testapp1\Cargo.toml --release --target x86_64-unknown-none -- -C relocation-model=static -C link-arg=-Ttestapp1/linker.ld
if errorlevel 1 exit /b 1

echo [3/7] Building testapp2...
cargo --config "target.x86_64-unknown-none.rustflags=[]" rustc --manifest-path testapp2\Cargo.toml --release --target x86_64-unknown-none -- -C relocation-model=static -C link-arg=-Ttestapp2/linker.ld
if errorlevel 1 exit /b 1

echo [4/7] Converting applications to binary...
llvm-objcopy -O binary target\x86_64-unknown-none\release\testapp1 kernel\testapp1.bin
if errorlevel 1 exit /b 1

llvm-objcopy -O binary target\x86_64-unknown-none\release\testapp2 kernel\testapp2.bin
if errorlevel 1 exit /b 1

for %%I in (kernel\testapp1.bin kernel\testapp2.bin) do (
    if %%~zI LEQ 0 (
        echo ERROR: %%I is empty.
        exit /b 1
    )
    if %%~zI GTR 4096 (
        echo ERROR: %%I exceeds the 4096-byte code page limit.
        exit /b 1
    )
)

echo [5/7] Building kernel...
cargo build -p kernel --target x86_64-unknown-none --release
if errorlevel 1 exit /b 1

echo [6/7] Preparing EFI files...

wsl.exe --exec bash /mnt/c/Users/1510ty/TekitouDevelopment/TekitouOS/sign_bootloader.sh
if errorlevel 1 exit /b 1

copy /Y target\x86_64-unknown-none\release\kernel esp\kernel.elf
if errorlevel 1 exit /b 1

echo [7/7] Starting QEMU...
"C:\Program Files\qemu\qemu-system-x86_64.exe" ^
    -machine q35,smm=on ^
    -global driver=cfi.pflash01,property=secure,value=on ^
    -drive "if=pflash,format=raw,unit=0,file=esp\edk2-x86_64-secure-code.fd,readonly=on" ^
    -drive "if=pflash,format=raw,unit=1,file=esp\edk2-i386-vars.fd" ^
    -drive "format=raw,file=fat:rw:esp" ^
    -d int -D qemu-int.log

endlocal
