# M2 results

Measured evidence from M2 (document and export). Conditions go with every number.

## D5: PDF export speed (2026-11-06)

Target: a 100-step recording exports to PDF in under 10 seconds.

Test: the headless PDF self-test (`babelsteps-app --pdf-selftest <dir>`, feature `selftest`). It generates 100 steps with patterned PNGs, prints through a hidden WebView2 window, and inspects the real PDF. Release build, 24 logical CPUs, the owner's daily PC in normal use.

| Screenshot size | Total | Prepare images + HTML | Print | PDF size | Pages |
| --- | --- | --- | --- | --- | --- |
| 5120x1440, before the change | about 30 s | 8.7 s | 21–22 s | 495 MB | 34 |
| 5120x1440, after (no header) | 8.97 s | 2.10 s | 6.87 s | 225.6 MB | 34 |
| 5120x1440, after (header and footer) | 9.16 s | 2.22 s | 6.94 s | 225.6 MB | 34 |
| 1920x540, after | 3.21 s | 0.31 s | 2.90 s | 16.1 MB | not recorded |

**Met on this PC.** What changed (commit c03d2c7):

- HTML and PDF images are scaled to 2000 px wide (`PRINT_MAX_WIDTH`), which is over 250 dpi across a Letter page. Redaction boxes are painted before scaling, so scaling can only blend black into the box edges, never hidden pixels. A unit test checks this and can fail.
- Images are prepared on up to 8 threads. Errors are still reported in document order.
- Markdown exports keep full-size images.

Checked in the real PDF: the redaction box is black in all 100 extracted images, with no red from inside it; the control image proves the check can fail. No file path, user name or `file:` URL in raw bytes, decompressed streams or extracted text (with a negative control that leaves FooterUri unset).

**Not tested:**

- Lower-end hardware. The margin is under 1.1 s on a fast desktop, so a typical laptop may miss 10 s at ultrawide sizes (a guess).
- Real screenshots. The synthetic images compress poorly, so the PDF size is probably overstated.
- The dialog plugin's native pickers. (The live window and export commands were tested later; see the next section.)

## Editor UI in the real app (2026-11-06)

Debug build with the embedded frontend, driven through WebView2's local test connection (127.0.0.1 only, page-level input, no OS input). One synthetic test recording with made-up images; deleted afterwards.

Worked:

- Library lists the recording; opening it loads real screenshots through `read_screenshot`. The withheld step shows its notice and loads no image.
- A redaction box drawn with the mouse saved in source pixels: in the exported `step-001.png` every sampled pixel of the red test block is black (0 of 7,500 red left), pixels outside are unchanged, and the control image without a box still has all 7,500 red.
- Markdown export: two images, withheld step written as a text note. A second export to the same folder is refused with `output_not_empty`.
- PDF export: 2 pages, no file path, user name or `file:` in the bytes. (Pixel redaction in PDFs is covered by the PDF self-test.)
- Unsaved edits mark the document dirty; leaving offers Keep editing, Discard changes, Save and leave.
- Delete asks first, says it cannot be undone, and removes the folder.

Not tested: the native file pickers (argument shapes checked against the dialog plugin's source only), closing the window with unsaved changes, screen readers, keyboard-only use in WebView2. Markdown output has two level-1 headings when the document starts with a level-1 heading block (cosmetic).
