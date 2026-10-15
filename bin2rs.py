from pathlib import Path

data = Path("testapp1.bin").read_bytes()

if len(data) > 4096:
    raise SystemExit(f"大きすぎる！ {len(data)} bytes")

lines = ["const USER_TEST_CODE: &[u8] = &["]

for i in range(0, len(data), 12):
    chunk = data[i:i + 12]
    lines.append("    " + ", ".join(f"0x{b:02X}" for b in chunk) + ",")

lines.append("];")

Path("user_code_snippet.txt").write_text(
    "\n".join(lines) + "\n",
    encoding="ascii",
)

print(f"変換完了！ {len(data)} bytes")