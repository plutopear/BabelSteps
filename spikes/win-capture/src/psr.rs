//! Steps Recorder baseline: drive `psr.exe` from its command line against our own test
//! window, then keep its output for inspection. `/recordpid` limits it to this process,
//! so it does not record what the person at the machine is doing.

use crate::imgops::Frame;
use crate::testwin::TestWindow;
use anyhow::{bail, Context, Result};
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;
use std::thread::sleep;
use std::time::{Duration, Instant};

const PSR: &str = r"C:\Windows\System32\psr.exe";
const EXTRA_CLICKS: usize = 20;

/// psr.exe has `uiAccess` in its manifest, and CreateProcess refuses such programs with
/// error 740. The shell's launch path handles them, so go through `cmd /c start`.
fn shell_run(args: &str) -> Result<()> {
    let status = Command::new("cmd").raw_arg(format!("/C start \"\" \"{PSR}\" {args}")).status()?;
    if !status.success() {
        bail!("cmd /c start returned {status:?}");
    }
    Ok(())
}

/// Report the color at a screen point in each saved JPEG, and write one crop of the
/// test window so it can be looked at without exposing the rest of the screen.
pub fn inspect_images(dir: PathBuf, probe: (i32, i32), crop: (i32, i32, i32, i32)) -> Result<()> {
    let mut files: Vec<_> = std::fs::read_dir(&dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().map(|x| x == "jpg").unwrap_or(false))
        .collect();
    files.sort();
    let mut wrote_crop = false;
    for path in &files {
        let mut decoder = jpeg_decoder::Decoder::new(std::io::BufReader::new(std::fs::File::open(path)?));
        let rgb = decoder.decode()?;
        let info = decoder.info().context("no JPEG info")?;
        let frame = Frame {
            width: info.width as u32,
            height: info.height as u32,
            origin_x: 0,
            origin_y: 0,
            bgra: rgb.chunks_exact(3).flat_map(|p| [p[2], p[1], p[0], 255]).collect(),
        };
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        println!("{name}: {}x{} probe {:?}", frame.width, frame.height, frame.sample(probe.0, probe.1));
        if !wrote_crop {
            let c = frame.crop(crop.0, crop.1, crop.2, crop.3);
            std::fs::write(dir.join("crop-first.png"), c.encode_png(png::Compression::Fast)?)?;
            wrote_crop = true;
        }
    }
    Ok(())
}

pub fn run(out_dir: PathBuf) -> Result<()> {
    std::fs::create_dir_all(&out_dir)?;
    let zip = std::path::absolute(out_dir.join("psr-output.zip"))?;
    let _ = std::fs::remove_file(&zip);

    let win = TestWindow::spawn()?;
    sleep(Duration::from_millis(300));
    win.allow_focus(true);
    let (user, pass, save, bg, probe) =
        (win.user_center()?, win.pass_center()?, win.save_center()?, win.background_point()?, win.probe_point()?);
    println!("Window rect: {:?}", win.rect()?);
    println!("Probe point: {probe:?}");

    let pid = std::process::id().to_string();
    // The default limit is 25 screenshots (seen in an earlier run), which would discard
    // the early steps this run is here to look at.
    shell_run(&format!("/start /output \"{}\" /sc 1 /maxsc 100 /gui 0 /recordpid {pid}", zip.display()))
        .context("start psr.exe")?;
    sleep(Duration::from_secs(3));

    // One of each action Steps Recorder might describe.
    win.safe_click(user.0, user.1)?;
    sleep(Duration::from_millis(600));
    win.safe_type("hello")?;
    sleep(Duration::from_millis(400));
    win.safe_ctrl_a()?;
    sleep(Duration::from_millis(400));
    win.safe_click(pass.0, pass.1)?;
    sleep(Duration::from_millis(600));
    win.safe_type("secret")?;
    sleep(Duration::from_millis(400));

    // The window turns red on press here, which shows whether its image is before or after.
    win.set_red(false);
    sleep(Duration::from_millis(400));
    win.safe_click(bg.0, bg.1)?;
    sleep(Duration::from_millis(800));
    win.set_red(false);
    sleep(Duration::from_millis(400));
    win.safe_right_click(bg.0, bg.1)?;
    sleep(Duration::from_millis(600));
    win.safe_click(save.0, save.1)?;
    sleep(Duration::from_millis(600));

    // Repeated press-reacting clicks: does each image show the state before or after?
    for _ in 0..EXTRA_CLICKS {
        win.set_red(false);
        sleep(Duration::from_millis(250));
        win.safe_click(bg.0, bg.1)?;
        sleep(Duration::from_millis(350));
    }

    shell_run("/stop").context("stop psr.exe")?;

    let t0 = Instant::now();
    let mut last_size = 0;
    while t0.elapsed() < Duration::from_secs(60) {
        let size = std::fs::metadata(&zip).map(|m| m.len()).unwrap_or(0);
        if size > 0 && size == last_size {
            println!("Output: {} ({size} bytes)", zip.display());
            return Ok(());
        }
        last_size = size;
        sleep(Duration::from_secs(1));
    }
    bail!("psr.exe wrote no output within 60 seconds")
}
