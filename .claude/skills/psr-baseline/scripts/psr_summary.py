"""Summarize Steps Recorder output: structure, step text, and field names.

Usage: python -I psr_summary.py OUTPUT.zip [IMAGE_DIR]
With IMAGE_DIR, each step's JPEG is saved there, named in step order. Those images
are full-screen and may show private content, so delete them when finished.
"""
import email
import html as htmllib
import os
import re
import sys
import zipfile
from email import policy

sys.stdout.reconfigure(encoding="utf-8", errors="replace")

archive = zipfile.ZipFile(sys.argv[1])
entries = archive.infolist()
print("Zip entries:", [(e.filename, e.file_size) for e in entries])
raw = archive.read(entries[0].filename)
message = email.message_from_bytes(raw, policy=policy.default)
parts = list(message.iter_parts())

kinds = {}
for part in parts:
    kinds[part.get_content_type()] = kinds.get(part.get_content_type(), 0) + 1
print("MHTML parts:", kinds)

images = [p for p in parts if p.get_content_type().startswith("image/")]
sizes = [len(p.get_payload(decode=True)) for p in images]
if sizes:
    print(f"Images: {len(sizes)}, average {sum(sizes) // len(sizes)} bytes")

page = [p for p in parts if p.get_content_type() == "text/html"][0].get_content()
text = re.sub(r"<(script|style)[^>]*>.*?</\1>", " ", page, flags=re.S | re.I)
text = re.sub(r"<[^>]+>", "\n", text)
lines = [re.sub(r"\s+", " ", htmllib.unescape(line)).strip() for line in text.splitlines()]
lines = [line for line in lines if line]

print("\nSteps (step number, then image index):")
step = 0
for line in lines:
    if line.startswith("User "):
        step += 1
        print(f"{step:03d} image {step - 1:03d}: {line[:120]}")

for line in lines:
    if line.startswith("Recorded Steps:"):
        print("\n" + line)
        break

# Only the field names: the values can hold machine-specific paths.
start = next((i for i, line in enumerate(lines) if line.startswith("Additional Details")), None)
if start is not None:
    labels = []
    for line in lines[start:]:
        found = re.match(r"^([A-Za-z][A-Za-z ]{2,30}):", line)
        if found and found.group(1) not in labels:
            labels.append(found.group(1))
    print("Additional details fields:", labels)

print("\nTyped test strings present in the file:")
for needle in ("hello", "secret"):
    print(f"  {needle!r}: {needle.encode() in raw or needle.encode('utf-16-le') in raw}")

if len(sys.argv) > 2:
    os.makedirs(sys.argv[2], exist_ok=True)
    for index, part in enumerate(images):
        with open(os.path.join(sys.argv[2], f"{index:03d}.jpg"), "wb") as handle:
            handle.write(part.get_payload(decode=True))
    print(f"\nWrote {len(images)} images to {sys.argv[2]}. Delete them when finished.")
