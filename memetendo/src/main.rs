#![warn(clippy::pedantic)]

use std::{
    mem::take,
    path::Path,
    thread::sleep,
    time::{Duration, Instant},
};

use anyhow::{anyhow, Context, Result};
use clap::{arg, command, Arg};
use libmemetendo::{
    gba::Gba,
    keypad::{Key, Keypad},
    rom::{Bios, Cartridge, Rom},
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

fn main() -> Result<()> {
    const FRAME_DURATION: Duration = Duration::from_nanos(1_000_000_000 / 60);

    let matches = command!()
        .arg(
            Arg::new("skip-bios")
                .long("skip-bios")
                .help("Skip executing BIOS ROM after boot"),
        )
        .arg(arg!(-b --bios <BIOS_FILE> "BIOS ROM file to use").allow_invalid_utf8(true))
        .arg(arg!(<ROM_FILE> "Cartridge ROM file to execute").allow_invalid_utf8(true))
        .get_matches();

    let skip_bios = matches.is_present("skip-bios");
    let bios_file = Path::new(matches.value_of_os("bios").unwrap());
    let cart_file = Path::new(matches.value_of_os("ROM_FILE").unwrap());

    let bios_rom = Rom::from_file(bios_file).context("failed to read BIOS ROM file")?;
    let bios = Bios::new(&bios_rom).map_err(|_| anyhow!("invalid BIOS ROM size"))?;

    let cart_rom = Rom::from_file(cart_file).context("failed to read cartridge ROM file")?;
    let cart = Cartridge::new(&cart_rom).map_err(|_| anyhow!("invalid cartridge ROM size"))?;

    let mut context = SdlContext::init()?;
    let mut screen = SdlScreen::new(&context.win_texture_creator)?;
    context.win_canvas.set_draw_color(Color::BLACK);
    context.win_canvas.clear();
    context.win_canvas.present();

    let mut gba = Gba::new(bios, cart);
    gba.reset(skip_bios);

    let mut audio = Audio::new(context.sdl_audio.as_ref().map(|sdl_audio| {
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

    let mut next_redraw_time = Instant::now() + FRAME_DURATION;
    'main_loop: loop {
        const MAX_FRAME_SKIP: u32 = 3;

        let mut skipped_frames = 0;
        loop {
            while !take(&mut screen.new_frame) {
                gba.step(&mut screen, &mut audio, skipped_frames > 0);
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

            if skipped_frames >= MAX_FRAME_SKIP {
                break;
            }
            skipped_frames += 1;
        }

        for event in context.event_pump.poll_iter() {
            if let Event::Quit { .. } = event {
                break 'main_loop;
            }
        }
        update_keypad(&mut gba.keypad, &context.event_pump.keyboard_state());

        context.win_canvas.clear();
        context
            .win_canvas
            .copy(screen.update_texture(gba.video.frame())?, None, None)
            .map_err(|e| anyhow!("failed to draw screen texture: {e}"))?;
        context.win_canvas.present();

        if skipped_frames >= MAX_FRAME_SKIP {
            next_redraw_time = Instant::now() + FRAME_DURATION;
        }
    }

    Ok(())
}
