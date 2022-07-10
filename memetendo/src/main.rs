#![warn(clippy::pedantic)]

use std::{
    path::Path,
    thread::sleep,
    time::{Duration, Instant},
};

use anyhow::{anyhow, Context, Result};
use clap::{arg, command, Arg};
use libmemetendo::{
    gba::Gba,
    keypad::Key,
    rom::{Bios, Cartridge, Rom},
    video::screen::{self, FrameBuffer, Screen},
};
use sdl2::{
    event::Event,
    keyboard::Scancode,
    pixels::{Color, PixelFormatEnum},
    render::{Texture, TextureCreator, WindowCanvas},
    video::WindowContext,
    EventPump, Sdl, VideoSubsystem,
};

struct SdlContext {
    _sdl: Sdl,
    _sdl_video: VideoSubsystem,
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
            win_canvas,
            win_texture_creator,
            event_pump,
        })
    }
}

struct SdlScreen<'r> {
    frame_buf: FrameBuffer,
    is_stale: bool,
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
            frame_buf: FrameBuffer::new(),
            is_stale: true,
            texture,
        })
    }

    fn texture(&mut self) -> Result<&Texture> {
        if self.is_stale {
            self.texture
                .with_lock(None, |buf, _| buf.copy_from_slice(&self.frame_buf.0[..]))
                .map_err(|e| anyhow!("failed to lock screen texture: {e}"))?;
            self.is_stale = false;
        }

        Ok(&self.texture)
    }
}

impl Screen for SdlScreen<'_> {
    fn present_frame(&mut self, frame_buf: &FrameBuffer) {
        self.frame_buf.0.copy_from_slice(&frame_buf.0[..]);
        self.is_stale = true;
    }
}

fn main() -> Result<()> {
    const REDRAW_DURATION: Duration = Duration::from_nanos(1_000_000_000 / 60);

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

    let mut next_redraw_time = Instant::now() + REDRAW_DURATION;
    'main_loop: loop {
        for _ in 0..100_000 {
            gba.step(&mut screen);
        }

        let now = Instant::now();
        if now >= next_redraw_time {
            next_redraw_time += REDRAW_DURATION;
            if now - next_redraw_time >= 3 * REDRAW_DURATION {
                // A simple reschedule if we're too far behind.
                next_redraw_time = now + REDRAW_DURATION;
            }
            if now >= next_redraw_time {
                continue;
            }

            for event in context.event_pump.poll_iter() {
                if let Event::Quit { .. } = event {
                    break 'main_loop;
                }
            }

            let kb = context.event_pump.keyboard_state();
            gba.keypad.pressed[Key::A] = kb.is_scancode_pressed(Scancode::X);
            gba.keypad.pressed[Key::B] = kb.is_scancode_pressed(Scancode::Z);
            gba.keypad.pressed[Key::Select] = kb.is_scancode_pressed(Scancode::LShift)
                || kb.is_scancode_pressed(Scancode::RShift);
            gba.keypad.pressed[Key::Start] = kb.is_scancode_pressed(Scancode::Return);
            gba.keypad.pressed[Key::Up] = kb.is_scancode_pressed(Scancode::Up);
            gba.keypad.pressed[Key::Down] = kb.is_scancode_pressed(Scancode::Down);
            gba.keypad.pressed[Key::Left] = kb.is_scancode_pressed(Scancode::Left);
            gba.keypad.pressed[Key::Right] = kb.is_scancode_pressed(Scancode::Right);
            gba.keypad.pressed[Key::L] = kb.is_scancode_pressed(Scancode::A);
            gba.keypad.pressed[Key::R] = kb.is_scancode_pressed(Scancode::S);

            context.win_canvas.clear();
            context
                .win_canvas
                .copy(screen.texture()?, None, None)
                .map_err(|e| anyhow!("failed to draw screen texture: {e}"))?;
            context.win_canvas.present();
        }

        sleep(next_redraw_time - now);
    }

    Ok(())
}
