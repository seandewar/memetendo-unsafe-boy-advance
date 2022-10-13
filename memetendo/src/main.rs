#![warn(clippy::pedantic)]

use std::{
    fs,
    mem::take,
    path::Path,
    thread::sleep,
    time::{Duration, Instant},
};

use anyhow::{anyhow, Context, Result};
use clap::{arg, command, value_parser};
use libmemetendo::{
    bios::Bios,
    cart::{BackupType, Cartridge, Rom},
    gba::Gba,
    keypad::{Key, Keypad},
    video::screen::{self, FrameBuffer, Screen},
};
use sdl2::{
    audio::AudioSpecDesired,
    event::Event,
    keyboard::{KeyboardState, Scancode},
    pixels::{Color, PixelFormatEnum},
    render::{Texture, TextureCreator, WindowCanvas},
    video::WindowContext,
    AudioSubsystem, EventPump, Sdl, VideoSubsystem,
};

use crate::audio::Audio;

mod audio;

struct SdlContext {
    _sdl: Sdl,
    _sdl_video: VideoSubsystem,
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
                println!("failed to init sdl2 audio subsystem: {e}");
                None
            }
        };

        #[allow(clippy::cast_possible_truncation)]
        let window = sdl_video
            .window(
                "Memetendo Unsafe Boy Advance",
                screen::WIDTH as u32,
                screen::HEIGHT as u32,
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
            _sdl: sdl,
            _sdl_video: sdl_video,
            sdl_audio,
            win_canvas,
            win_texture_creator,
            event_pump,
        })
    }
}

struct SdlScreen<'r> {
    new_frame: bool,
    texture: Texture<'r>,
}

impl<'r> SdlScreen<'r> {
    fn new<T>(texture_creator: &'r TextureCreator<T>) -> Result<Self> {
        #[allow(clippy::cast_possible_truncation)]
        let texture = texture_creator
            .create_texture_streaming(
                PixelFormatEnum::RGB24,
                screen::WIDTH as u32,
                screen::HEIGHT as u32,
            )
            .context("failed to create screen texture")?;

        Ok(Self {
            new_frame: false,
            texture,
        })
    }

    fn update_texture(&mut self, frame: &FrameBuffer) -> Result<&Texture> {
        self.texture
            .with_lock(None, |buf, _| buf.copy_from_slice(&frame.0))
            .map_err(|e| anyhow!("failed to lock screen texture: {e}"))?;

        Ok(&self.texture)
    }
}

impl Screen for SdlScreen<'_> {
    fn finished_frame(&mut self, _frame: &FrameBuffer) {
        self.new_frame = true;
    }
}

fn main() -> Result<()> {
    let matches = command!()
        .arg(arg!(--"skip-bios" "Skip executing BIOS ROM after boot").required(false))
        .arg(arg!(-b --bios <BIOS_FILE> "BIOS ROM file to use").allow_invalid_utf8(true))
        .arg(
            arg!(--backup <BACKUP_TYPE> "Cartridge backup type to use")
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
            arg!(--"frame-skip" <FRAME_SKIP> "Maximum frames to skip when behind")
                .value_parser(value_parser!(u32))
                .default_value("3")
                .required(false),
        )
        .get_matches();

    let skip_bios = matches.is_present("skip-bios");
    let bios_file = Path::new(matches.value_of_os("bios").unwrap());
    let cart_backup_type = matches
        .get_one::<String>("backup")
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
    let cart_file = Path::new(matches.value_of_os("ROM_FILE").unwrap());
    let max_frame_skip = *matches.get_one::<u32>("frame-skip").unwrap();

    let bios_rom = fs::read(bios_file).context("failed to read BIOS ROM file")?;
    let bios = Bios::new(&bios_rom).context("invalid BIOS ROM size")?;

    let cart_rom_data = fs::read(cart_file).context("failed to read cartridge ROM file")?;
    let cart_rom = Rom::new(cart_rom_data.as_slice()).context("invalid cartridge ROM size")?;
    let cart_backup_type = cart_backup_type.unwrap_or_else(|| cart_rom.parse_backup_type());
    println!("cart backup type: {:?}", cart_backup_type);
    let cart = Cartridge::new(cart_rom, cart_backup_type);

    let mut sdl = SdlContext::init()?;
    let mut screen = SdlScreen::new(&sdl.win_texture_creator)?;
    sdl.win_canvas.set_draw_color(Color::BLACK);
    sdl.win_canvas.clear();
    sdl.win_canvas.present();

    let mut gba = Gba::new(bios, cart);
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
        println!("failed to initialize audio: {e}");
        audio
    });

    main_loop(
        &mut sdl.event_pump,
        &mut sdl.win_canvas,
        &mut screen,
        &mut audio,
        &mut gba,
        max_frame_skip,
    )
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
    screen: &mut SdlScreen,
    audio: &mut Audio,
    gba: &mut Gba,
    max_frame_skip: u32,
) -> Result<()> {
    const FRAME_DURATION: Duration = Duration::from_nanos(1_000_000_000 / 60);

    let mut next_redraw_time = Instant::now() + FRAME_DURATION;
    'main_loop: loop {
        let mut skipped_frames = 0;
        loop {
            while !take(&mut screen.new_frame) {
                gba.step(screen, audio, skipped_frames > 0);
            }
            if let Err(e) = audio.queue_samples() {
                println!("failed to queue audio samples: {e}");
            }

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
        win_canvas
            .copy(screen.update_texture(gba.video.frame())?, None, None)
            .map_err(|e| anyhow!("failed to draw screen texture: {e}"))?;
        win_canvas.present();

        if skipped_frames >= max_frame_skip {
            next_redraw_time = Instant::now() + FRAME_DURATION;
        }
    }

    Ok(())
}
