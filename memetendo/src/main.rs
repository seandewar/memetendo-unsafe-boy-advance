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
    audio::{self, SAMPLE_FREQUENCY},
    gba::Gba,
    keypad::{Key, Keypad},
    rom::{Bios, Cartridge, Rom},
    video::screen::{self, FrameBuffer, Screen},
};
use sdl2::{
    audio::{AudioQueue, AudioSpec, AudioSpecDesired},
    event::Event,
    keyboard::{KeyboardState, Scancode},
    pixels::{Color, PixelFormatEnum},
    render::{Texture, TextureCreator, WindowCanvas},
    video::WindowContext,
    AudioSubsystem, EventPump, Sdl, VideoSubsystem,
};

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

struct SdlAudioDevice {
    channels: u8,
    freq: u32,
    freq_counter: u32,
    freq_counter_accum: u32,
    sample_accum: (i32, i32),
    accum_extra_sample: bool,
    samples: Vec<i16>,
}

impl SdlAudioDevice {
    fn new(spec: &AudioSpec) -> Result<Self> {
        println!("audio spec: {spec:?}");
        if spec.channels > 2 {
            return Err(anyhow!(
                "only 1 (mono) or 2 (stereo) audio channels are currently supported (got {})",
                spec.channels
            ));
        }

        #[allow(clippy::cast_sign_loss)]
        let freq = spec.freq as u32;
        if freq > SAMPLE_FREQUENCY {
            // We could technically handle this, but it's probably not worth it.
            return Err(anyhow!(
                "audio frequency too high (got: {freq} Hz, max: {SAMPLE_FREQUENCY} Hz)"
            ));
        }

        Ok(Self {
            channels: spec.channels,
            freq,
            freq_counter: 0,
            freq_counter_accum: 0,
            sample_accum: (0, 0),
            accum_extra_sample: false,
            samples: Vec::new(),
        })
    }

    fn queue_samples(&mut self, queue: &AudioQueue<i16>) {
        // Try not to have too many old samples queued, otherwise there may be a noticeable
        // delay when playing our new samples. This can happen after lots of frame skip.
        // If there are still ~333ms worth of old samples not yet sent to the hardware, just clear
        // them all.
        if queue.size() > (self.freq * u32::from(self.channels)) / 3 {
            queue.clear();
        }

        if let Err(e) = queue.queue_audio(&self.samples[..]) {
            // Probably not fatal.
            println!("failed to queue {} audio samples: {e}", self.samples.len());
        }
        self.samples.clear();
    }
}

impl audio::Device for SdlAudioDevice {
    fn push_sample(&mut self, sample: (i16, i16)) {
        self.sample_accum.0 += i32::from(sample.0);
        self.sample_accum.1 += i32::from(sample.1);
        self.freq_counter += 1;
        if self.freq_counter < (SAMPLE_FREQUENCY / self.freq) + u32::from(self.accum_extra_sample) {
            return;
        }

        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        let sample = (
            (self.sample_accum.0 / self.freq_counter as i32) as i16,
            (self.sample_accum.1 / self.freq_counter as i32) as i16,
        );
        self.freq_counter = 0;
        self.sample_accum = (0, 0);

        // Driver frequency may not divide exactly with the sample output frequency, so we may
        // drift behind by a full sample; if so, accumulate an extra sample next time.
        self.freq_counter_accum += SAMPLE_FREQUENCY % self.freq;
        self.accum_extra_sample = self.freq_counter_accum >= self.freq;
        if self.accum_extra_sample {
            self.freq_counter_accum -= self.freq;
        }

        if self.channels > 1 {
            self.samples.push(sample.0);
            self.samples.push(sample.1);
        } else {
            self.samples.push(sample.0 / 2 + sample.1 / 2);
        }
    }
}

struct SdlOptionAudioDevice<'a>(Option<&'a mut SdlAudioDevice>);

impl audio::Device for SdlOptionAudioDevice<'_> {
    fn push_sample(&mut self, sample: (i16, i16)) {
        if let Some(ref mut device) = self.0 {
            device.push_sample(sample);
        }
    }
}

fn init_audio_device(context: &SdlContext) -> (Option<AudioQueue<i16>>, Option<SdlAudioDevice>) {
    let audio = if let Some(audio) = context.sdl_audio.as_ref() {
        audio
    } else {
        return (None, None);
    };

    let queue = match audio.open_queue(
        None,
        &AudioSpecDesired {
            freq: Some(44_100),
            channels: Some(2),
            samples: Some(2048),
        },
    ) {
        Ok(queue) => queue,
        Err(e) => {
            println!("failed to create sdl2 audio queue: {e}");
            return (None, None);
        }
    };
    let device = match SdlAudioDevice::new(queue.spec()) {
        Ok(device) => {
            queue.resume();
            Some(device)
        }
        Err(e) => {
            println!("failed to create sdl2 audio device: {e}");
            None
        }
    };

    (Some(queue), device)
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

    let (audio_queue, mut audio_device) = init_audio_device(&context);

    let mut next_redraw_time = Instant::now() + FRAME_DURATION;
    'main_loop: loop {
        const MAX_FRAME_SKIP: u32 = 3;

        let mut skipped_frames = 0;
        loop {
            while !take(&mut screen.new_frame) {
                gba.step(
                    &mut screen,
                    &mut SdlOptionAudioDevice(audio_device.as_mut()),
                    skipped_frames > 0,
                );
            }
            if let (Some(queue), Some(device)) = (audio_queue.as_ref(), audio_device.as_mut()) {
                device.queue_samples(queue);
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
