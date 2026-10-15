#!/usr/bin/env bash
set -euo pipefail

PROJECT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
KEY="$HOME/.config/tekitouos-secureboot/TekitouOS-MOK.key"
CERT="$HOME/.config/tekitouos-secureboot/TekitouOS-MOK.pem"
INPUT="$PROJECT_DIR/target/x86_64-unknown-uefi/release/bootloader.efi"
OUTPUT="$PROJECT_DIR/target/x86_64-unknown-uefi/release/bootloader.efi.signed"
ESP_LOADER="$PROJECT_DIR/esp/EFI/BOOT/grubx64.efi"

for file in "$KEY" "$CERT" "$INPUT"; do
    if [[ ! -f "$file" ]]; then
        echo "ERROR: File not found: $file" >&2
        exit 1
    fi
done

rm -f "$OUTPUT"
echo "[3/4] Signing TekitouOS EFI bootloader..."
sbsign --key "$KEY" --cert "$CERT" --output "$OUTPUT" "$INPUT"

echo "Verifying signature..."
sbverify --cert "$CERT" "$OUTPUT"

cp "$OUTPUT" "$ESP_LOADER"
echo "Signed bootloader copied to ESP."
