#![warn(clippy::pedantic)]

use std::{
    fmt::Write,
    fs, io,
    mem::take,
    path::Path,
    rc::Rc,
    thread::sleep,
    time::{Duration, Instant},
};

use anyhow::{anyhow, Context, Result};
use clap::{arg, command, value_parser};
use libmemetendo::{
    bios,
    cart::{self, BackupType, Cartridge},
    gba::Gba,
    keypad::{Key, Keypad},
    util::video::FrameBuffer,
    video::{self, HBLANK_DOT, VBLANK_DOT},
};
use log::{error, info, warn};
use sdl2::{
    audio::AudioSpecDesired,
    event::Event,
    keyboard::{KeyboardState, Scancode},
    pixels::{Color, PixelFormatEnum},
    render::{Texture, TextureCreator, WindowCanvas},
    video::WindowContext,
    AudioSubsystem, EventPump,
};

use crate::audio::Audio;

mod audio;

struct SdlContext {
    sdl_audio: Option<AudioSubsystem>,
    win_canvas: WindowCanvas,
    win_texture_creator: TextureCreator<WindowContext>,
    event_pump: EventPump,
}

impl SdlContext {
    fn init() -> Result<Self> {
        let sdl = sdl2::init().map_err(|e| anyhow!("failed to init sdl2: {e}"))?;

        let event_pump = sdl
            .event_pump()
            .map_err(|e| anyhow!("failed to get sdl2 event pump: {e}"))?;

        let sdl_video = sdl
            .video()
            .map_err(|e| anyhow!("failed to init sdl2 video subsystem: {e}"))?;

        let sdl_audio = match sdl.audio() {
            Ok(audio) => Some(audio),
            Err(e) => {
                error!("failed to init sdl2 audio subsystem: {e}");
                None
            }
        };

        let window = sdl_video
            .window(
                "Memetendo Unsafe Boy Advance",
                HBLANK_DOT.into(),
                VBLANK_DOT.into(),
            )
            .position_centered()
            .resizable()
            .build()
            .context("failed to create sdl2 window")?;

        let win_canvas = window
            .into_canvas()
            .build()
            .context("failed to get sdl2 window canvas")?;

        let win_texture_creator = win_canvas.texture_creator();

        Ok(Self {
            sdl_audio,
            win_canvas,
            win_texture_creator,
            event_pump,
        })
    }
}

struct VideoCallback<'r> {
    texture: Texture<'r>,
    new_frame: bool,
    frame_skipping: bool,
    buf: FrameBuffer,
}

impl<'r> VideoCallback<'r> {
    fn new<T>(texture_creator: &'r TextureCreator<T>) -> Result<Self> {
        let texture = texture_creator
            .create_texture_streaming(PixelFormatEnum::RGB24, HBLANK_DOT.into(), VBLANK_DOT.into())
            .context("failed to create screen texture")?;

        Ok(Self {
            texture,
            new_frame: false,
            frame_skipping: false,
            buf: FrameBuffer::default(),
        })
    }
}

impl video::Callback for VideoCallback<'_> {
    fn put_dot(&mut self, x: u8, y: u8, dot: video::Dot) {
        self.buf.put_dot(x, y, dot);
    }

    fn end_frame(&mut self, green_swap: bool) {
        self.new_frame = true;
        if self.frame_skipping {
            return;
        }

        if green_swap {
            self.buf.green_swap();
        }

        if let Err(e) = self.texture.with_lock(None, |texture_buf, _| {
            texture_buf.copy_from_slice(&self.buf.0);
        }) {
            warn!("failed to lock screen texture: {e}");
        }
    }

    fn is_frame_skipping(&self) -> bool {
        self.frame_skipping
    }
}

fn load_cart(
    rom: cart::Rom,
    backup_path: &impl AsRef<Path>,
    fallback_backup_type: Option<BackupType>,
) -> Cartridge {
    match fs::read(backup_path) {
        Ok(buf) => {
            let len = buf.len();
            let cart = Cartridge::try_from_backup(&rom, Some(buf.into_boxed_slice()));
            if cart.is_none() {
                error!(
                    "failed to determine cart backup type from file {} (len: {len})",
                    backup_path.as_ref().to_string_lossy()
                );
            }

            cart
        }
        Err(e) => {
            if e.kind() != io::ErrorKind::NotFound {
                error!(
                    "failed to read cart backup file {}: {e}",
                    backup_path.as_ref().to_string_lossy()
                );
            }

            None
        }
    }
    .unwrap_or_else(|| {
        let backup_type = fallback_backup_type.unwrap_or_else(|| rom.parse_backup_type());
        info!("using backup type: {backup_type:?}");

        Cartridge::new(rom, backup_type)
    })
}

fn main() -> Result<()> {
    env_logger::builder()
        .format_timestamp(None)
        .parse_env(env_logger::Env::default().default_filter_or("info"))
        .init();

    let matches = command!()
        .arg(arg!(--"skip-bios" "Skip executing BIOS ROM after boot").required(false))
        .arg(arg!(-b --bios <FILE> "BIOS ROM file to use").allow_invalid_utf8(true))
        .arg(
            arg!(--"backup-fallback" <TYPE> "Cartridge backup type to fallback to")
                .value_parser([
                    "none",
                    "eeprom-unknown",
                    "eeprom-512",
                    "eeprom-8k",
                    "sram-32k",
                    "flash-64k",
                    "flash-128k",
                ])
                .required(false),
        )
        .arg(arg!(<ROM_FILE> "Cartridge ROM file to execute").allow_invalid_utf8(true))
        .arg(
            arg!(--"frame-skip" <FRAMES> "Maximum frames to skip when behind")
                .value_parser(value_parser!(u32))
                .default_value("3")
                .required(false),
        )
        .get_matches();

    let skip_bios = matches.is_present("skip-bios");
    let bios_path = Path::new(matches.value_of_os("bios").unwrap());
    let cart_fallback_backup_type =
        matches
            .get_one::<String>("backup-fallback")
            .map(|s| match s.as_str() {
                "none" => BackupType::None,
                "eeprom-unknown" => BackupType::EepromUnknownSize,
                "eeprom-512" => BackupType::Eeprom512B,
                "eeprom-8k" => BackupType::Eeprom8KiB,
                "sram-32k" => BackupType::Sram32KiB,
                "flash-64k" => BackupType::Flash64KiB,
                "flash-128k" => BackupType::Flash128KiB,
                _ => unreachable!(),
            });
    let cart_path = Path::new(matches.value_of_os("ROM_FILE").unwrap());
    let max_frame_skip = *matches.get_one::<u32>("frame-skip").unwrap();

    let bios_rom_buf = fs::read(bios_path).context("failed to read BIOS ROM file")?;
    let bios_rom = bios::Rom::new(Rc::from(bios_rom_buf)).context("invalid BIOS ROM size")?;

    let cart_rom_buf = fs::read(cart_path).context("failed to read cartridge ROM file")?;
    let cart_rom = cart::Rom::new(Rc::from(cart_rom_buf)).context("invalid cartridge ROM size")?;
    let mut cart_backup_path = cart_path.to_owned();
    cart_backup_path.set_extension("sav");
    let cart = load_cart(cart_rom, &cart_backup_path, cart_fallback_backup_type);

    let mut sdl = SdlContext::init()?;
    let mut video_cb = VideoCallback::new(&sdl.win_texture_creator)?;
    sdl.win_canvas.set_draw_color(Color::BLACK);
    sdl.win_canvas.clear();
    sdl.win_canvas.present();

    let mut gba = Gba::new(bios_rom, cart);
    gba.reset(skip_bios);

    let mut audio = Audio::new(sdl.sdl_audio.as_ref().map(|sdl_audio| {
        (
            sdl_audio,
            AudioSpecDesired {
                freq: Some(44_100),
                channels: Some(2),
                samples: Some(2048),
            },
        )
    }))
    .unwrap_or_else(|(e, audio)| {
        error!("failed to initialize audio: {e}");
        audio
    });

    main_loop(
        &mut sdl.event_pump,
        &mut sdl.win_canvas,
        &mut video_cb,
        &mut audio,
        &mut gba,
        max_frame_skip,
    );

    if let Some(cart_backup_buf) = gba.cart.backup_buffer() {
        info!(
            "writing to cart backup file: {}",
            cart_backup_path.to_string_lossy()
        );
        if let Err(e) = fs::write(cart_backup_path, cart_backup_buf) {
            error!("failed to write backup file: {e}");
        }
    }

    Ok(())
}

fn update_keypad(kp: &mut Keypad, kb: &KeyboardState) {
    let pressed = |scancode| kb.is_scancode_pressed(scancode);

    kp.set_pressed(Key::A, pressed(Scancode::X));
    kp.set_pressed(Key::B, pressed(Scancode::Z));

    kp.set_pressed(
        Key::Select,
        pressed(Scancode::LShift) || pressed(Scancode::RShift),
    );
    kp.set_pressed(Key::Start, pressed(Scancode::Return));

    kp.set_pressed(Key::Up, pressed(Scancode::Up));
    kp.set_pressed(Key::Down, pressed(Scancode::Down));
    kp.set_pressed(Key::Left, pressed(Scancode::Left));
    kp.set_pressed(Key::Right, pressed(Scancode::Right));

    kp.set_pressed(Key::L, pressed(Scancode::A));
    kp.set_pressed(Key::R, pressed(Scancode::S));
}

fn main_loop(
    event_pump: &mut EventPump,
    win_canvas: &mut WindowCanvas,
    video_cb: &mut VideoCallback,
    audio: &mut Audio,
    gba: &mut Gba,
    max_frame_skip: u32,
) {
    const FRAME_DURATION: Duration = Duration::from_nanos(1_000_000_000 / 60);

    let mut next_redraw_time = Instant::now() + FRAME_DURATION;
    let mut next_second_time = Instant::now() + Duration::from_secs(1);
    let (mut frame_counter, mut unskipped_frame_counter) = (0u32, 0u32);
    let mut title_text_buf = String::new();

    'main_loop: loop {
        {
            let now = Instant::now();
            if now >= next_second_time {
                title_text_buf.clear();
                write!(
                    &mut title_text_buf,
                    "Memetendo Unsafe Boy Advance | FPS: {unskipped_frame_counter}"
                )
                .unwrap();
                if frame_counter != unskipped_frame_counter {
                    write!(&mut title_text_buf, " ({frame_counter})").unwrap();
                }

                win_canvas.window_mut().set_title(&title_text_buf).unwrap();
                next_second_time = now + Duration::from_secs(1);
                (frame_counter, unskipped_frame_counter) = (0, 0);
            }
        }

        let mut skipped_frames = 0;
        loop {
            video_cb.frame_skipping = skipped_frames > 0;
            while !take(&mut video_cb.new_frame) {
                gba.step(video_cb, audio);
            }
            if let Err(e) = audio.queue_samples() {
                warn!("failed to queue audio samples: {e}");
            }

            if skipped_frames == 0 {
                unskipped_frame_counter += 1;
            }
            frame_counter += 1;

            let rem_time = next_redraw_time - Instant::now();
            next_redraw_time += FRAME_DURATION;
            if rem_time > Duration::ZERO {
                sleep(rem_time);
                break;
            }

            if skipped_frames >= max_frame_skip {
                break;
            }
            skipped_frames += 1;
        }

        for event in event_pump.poll_iter() {
            if let Event::Quit { .. } = event {
                break 'main_loop;
            }
        }
        update_keypad(&mut gba.keypad, &event_pump.keyboard_state());

        win_canvas.clear();
        if let Err(e) = win_canvas.copy(&video_cb.texture, None, None) {
            warn!("failed to draw screen texture: {e}");
        }
        win_canvas.present();

        if skipped_frames >= max_frame_skip {
            next_redraw_time = Instant::now() + FRAME_DURATION;
        }
    }
}
