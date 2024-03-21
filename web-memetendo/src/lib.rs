#![warn(clippy::pedantic)]

use std::{cell::RefCell, mem::take, panic, rc::Rc};

use anyhow::{Context, Result};
use audio::Audio;
use js_sys::{Reflect, Uint8Array};
use libmemetendo::{
    bios,
    cart::{self, Cartridge},
    gba::Gba,
    keypad::Key,
    util::video::FrameBuffer,
    video::{self, HBLANK_DOT, VBLANK_DOT},
};
use log::{info, Level};
use wasm_bindgen::{prelude::*, Clamped, JsCast};
use web_sys::{
    CanvasRenderingContext2d, Document, Event, FileReader, HtmlCanvasElement, HtmlFieldSetElement,
    HtmlInputElement, HtmlParagraphElement, ImageData, KeyboardEvent, Window,
};

mod audio;

struct VideoCallback {
    canvas_ctx: CanvasRenderingContext2d,
    new_frame: bool,
    frame_skipping: bool,
    buf: FrameBuffer<4>,
}

impl video::Callback for VideoCallback {
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

        let image_data = ImageData::new_with_u8_clamped_array_and_sh(
            Clamped(&self.buf.0),
            HBLANK_DOT.into(),
            VBLANK_DOT.into(),
        )
        .unwrap();
        self.canvas_ctx
            .put_image_data(&image_data, 0.0, 0.0)
            .unwrap();
    }

    fn is_frame_skipping(&self) -> bool {
        self.frame_skipping
    }
}

impl VideoCallback {
    fn new(window: &Window) -> Result<Self> {
        let canvas = window
            .document()
            .unwrap()
            .get_element_by_id("memetendo-screen")
            .unwrap()
            .dyn_into::<HtmlCanvasElement>()
            .unwrap();

        let canvas_ctx = canvas
            .get_context_with_context_options("2d", &*{
                let options = js_sys::Object::new();
                Reflect::set(&options, &"alpha".into(), &false.into()).unwrap();
                Reflect::set(&options, &"desynchronized".into(), &true.into()).unwrap();
                options
            })
            .unwrap()
            .map(|ctx| ctx.dyn_into::<CanvasRenderingContext2d>().unwrap())
            .context("failed to get 2D canvas rendering context")?;

        Ok(Self {
            canvas_ctx,
            new_frame: false,
            frame_skipping: false,
            buf: FrameBuffer::new(0xff),
        })
    }

    fn clear(&self) {
        self.canvas_ctx
            .clear_rect(0.0, 0.0, HBLANK_DOT.into(), VBLANK_DOT.into());
    }
}

struct State {
    window: Window,
    document: Document,
    // TODO: Rc<RefCell<T>> sucks; is there a good way around it?
    audio: Rc<RefCell<Audio>>,
    video_cb: Rc<RefCell<VideoCallback>>,
    gba: Option<Gba>,
    updater: Option<Closure<dyn Fn(f64)>>,
    max_frame_skip: u32,
    next_frame_ms: Option<f64>,
    selected_bios_rom: Option<bios::Rom>,
    selected_cart_rom: Option<cart::Rom>,
}

impl State {
    async fn new(window: &Window) -> Result<Self> {
        let audio = Audio::new().await.unwrap_or_else(|(e, audio)| {
            alert(
                window,
                format!("Audio initialization failed; sound will be muted: {e:?}."),
            );
            audio
        });

        Ok(Self {
            window: window.clone(),
            document: window.document().unwrap(),
            audio: Rc::new(RefCell::new(audio)),
            video_cb: Rc::new(RefCell::new(VideoCallback::new(window)?)),
            gba: None,
            updater: None,
            max_frame_skip: 3,
            next_frame_ms: None,
            selected_bios_rom: None,
            selected_cart_rom: None,
        })
    }
}

fn maybe_start_emulation(state: &Rc<RefCell<State>>) -> bool {
    let mut borrowed_state = state.borrow_mut();
    let Some(ref bios_rom) = borrowed_state.selected_bios_rom else {
        return false;
    };
    let Some(ref cart_rom) = borrowed_state.selected_cart_rom else {
        return false;
    };

    let backup_type = cart_rom.parse_backup_type();
    info!("starting emulation - using cart backup type: {backup_type:?}");
    borrowed_state.gba = Some(Gba::new(
        bios_rom.clone(),
        Cartridge::new(cart_rom.clone(), backup_type),
    ));
    borrowed_state.video_cb.borrow().clear();
    borrowed_state.audio.borrow().resume();
    borrowed_state.next_frame_ms = None;

    if borrowed_state.updater.is_none() {
        borrowed_state.updater = Some({
            let state = Rc::clone(state);
            let video_cb = Rc::clone(&borrowed_state.video_cb);
            let audio = Rc::clone(&borrowed_state.audio);

            Closure::new(move |ms: f64| {
                const FRAME_DURATION_MS: f64 = 1000.0 / 59.737;

                let mut borrowed_state = state.borrow_mut();
                let mut next_frame_ms = borrowed_state.next_frame_ms.unwrap_or(ms);

                if ms >= next_frame_ms {
                    let max_frame_skip = borrowed_state.max_frame_skip;

                    let Some(ref mut gba) = borrowed_state.gba else {
                        borrowed_state.updater = None;
                        return;
                    };
                    let mut video_cb = video_cb.borrow_mut();
                    let mut audio = audio.borrow_mut();

                    let mut skipped_frames = 0;
                    loop {
                        video_cb.frame_skipping = skipped_frames > 0;
                        while !take(&mut video_cb.new_frame) {
                            gba.step(&mut *video_cb, &mut *audio);
                        }
                        audio.queue_samples();

                        next_frame_ms += FRAME_DURATION_MS;
                        if next_frame_ms > ms {
                            borrowed_state.next_frame_ms = Some(next_frame_ms);
                            break;
                        }

                        if skipped_frames >= max_frame_skip {
                            // Too far behind; reschedule for the next frame.
                            borrowed_state.next_frame_ms = None;
                            break;
                        }
                        skipped_frames += 1;
                    }
                }

                borrowed_state
                    .window
                    .request_animation_frame(
                        borrowed_state
                            .updater
                            .as_ref()
                            .unwrap()
                            .as_ref()
                            .unchecked_ref(),
                    )
                    .unwrap();
            })
        });

        borrowed_state
            .window
            .request_animation_frame(
                borrowed_state
                    .updater
                    .as_ref()
                    .unwrap()
                    .as_ref()
                    .unchecked_ref(),
            )
            .unwrap();
    }

    true
}

fn alert(window: &Window, message: impl AsRef<str>) {
    window.alert_with_message(message.as_ref()).unwrap();
}

async fn memetendo_main() {
    let window = web_sys::window().unwrap();
    let state = match State::new(&window).await {
        Ok(state) => Rc::new(RefCell::new(state)),
        Err(e) => {
            alert(&window, format!("Loading failed: {e}"));
            return;
        }
    };

    init_file_input(&state.borrow(), "memetendo-bios-file", {
        let state = Rc::clone(&state);
        move |rom_buf: Vec<u8>| {
            let Ok(rom) = bios::Rom::new(Rc::from(rom_buf)) else {
                alert(&state.borrow().window, "Invalid BIOS ROM size!");
                return;
            };
            state.borrow_mut().selected_bios_rom = Some(rom);
            maybe_start_emulation(&state);
        }
    });
    init_file_input(&state.borrow(), "memetendo-cart-file", {
        let state = Rc::clone(&state);
        move |rom_buf: Vec<u8>| {
            let Ok(rom) = cart::Rom::new(Rc::from(rom_buf)) else {
                alert(&state.borrow().window, "Invalid cartridge ROM size!");
                return;
            };
            state.borrow_mut().selected_cart_rom = Some(rom);
            maybe_start_emulation(&state);
        }
    });

    let document = window.document().unwrap();
    document
        .add_event_listener_with_callback(
            "keydown",
            create_keypress_handler(&state, true)
                .into_js_value()
                .unchecked_ref(),
        )
        .unwrap();
    document
        .add_event_listener_with_callback(
            "keyup",
            create_keypress_handler(&state, false)
                .into_js_value()
                .unchecked_ref(),
        )
        .unwrap();

    let frame_skip_input = document
        .get_element_by_id("memetendo-frame-skip")
        .unwrap()
        .dyn_into::<HtmlInputElement>()
        .unwrap();
    frame_skip_input.set_value(&state.borrow().max_frame_skip.to_string());
    frame_skip_input
        .add_event_listener_with_callback("change", {
            let state = Rc::clone(&state);
            Closure::<dyn Fn(_)>::new(move |event: Event| {
                let input = event
                    .target()
                    .unwrap()
                    .dyn_into::<HtmlInputElement>()
                    .unwrap();
                state.borrow_mut().max_frame_skip = input.value().parse().unwrap();
            })
            .into_js_value()
            .unchecked_ref()
        })
        .unwrap();

    document
        .get_element_by_id("memetendo-options")
        .unwrap()
        .dyn_into::<HtmlFieldSetElement>()
        .unwrap()
        .set_disabled(false);
    document
        .get_element_by_id("memetendo-status")
        .unwrap()
        .dyn_into::<HtmlParagraphElement>()
        .unwrap()
        .set_inner_text("Select a BIOS and Cartridge ROM file to start!");
}

// TODO: uses event.code(), so we need to have some sort of prompt that shows the actual key if the
// keyboard layout isn't QWERTY.
fn create_keypress_handler(
    state: &Rc<RefCell<State>>,
    pressed: bool,
) -> Closure<dyn FnMut(KeyboardEvent)> {
    let state = Rc::clone(state);
    Closure::new(move |event: KeyboardEvent| {
        let Some(ref mut gba) = state.borrow_mut().gba else {
            return;
        };
        let key = match event.code().as_str() {
            "KeyX" => Key::A,
            "KeyZ" => Key::B,
            "ShiftLeft" | "ShiftRight" => Key::Select,
            "Enter" => Key::Start,
            "ArrowUp" => Key::Up,
            "ArrowDown" => Key::Down,
            "ArrowLeft" => Key::Left,
            "ArrowRight" => Key::Right,
            "KeyA" => Key::L,
            "KeyS" => Key::R,
            _ => return,
        };
        gba.keypad.set_pressed(key, pressed);
        event.prevent_default();
    })
}

fn init_file_input(state: &State, id: &str, mut callback: impl FnMut(Vec<u8>) + 'static) {
    let reader = FileReader::new().unwrap();
    reader
        .add_event_listener_with_callback("loadend", {
            let window = state.window.clone();
            Closure::<dyn FnMut(_)>::new(move |event: Event| {
                let reader = event.target().unwrap().dyn_into::<FileReader>().unwrap();
                let array_buf = reader.result().unwrap();
                if array_buf.is_null() {
                    let dom_exception = reader.error().unwrap();
                    alert(
                        &window,
                        format!(
                            "Failed to open file: {} (code {}).",
                            dom_exception.message(),
                            dom_exception.code(),
                        ),
                    );
                    return;
                }

                callback(Uint8Array::new(&array_buf).to_vec());
            })
            .into_js_value()
            .unchecked_ref()
        })
        .unwrap();

    let input = state
        .document
        .get_element_by_id(id)
        .unwrap()
        .dyn_into::<HtmlInputElement>()
        .unwrap();
    input.set_value("");
    input
        .add_event_listener_with_callback("change", {
            let window = state.window.clone();
            Closure::<dyn Fn(_)>::new(move |event: Event| {
                let input = event
                    .target()
                    .unwrap()
                    .dyn_into::<HtmlInputElement>()
                    .unwrap();

                let files = input.files().unwrap();
                if files.length() != 1 {
                    alert(&window, "One file must be selected!");
                    return;
                }
                let file = files.item(0).unwrap();
                reader.read_as_array_buffer(&file).unwrap();
            })
            .into_js_value()
            .unchecked_ref()
        })
        .unwrap();
}

#[wasm_bindgen(start)]
pub fn main() {
    panic::set_hook(Box::new(console_error_panic_hook::hook));
    console_log::init_with_level(Level::Info)
        .unwrap_or_else(|e| eprintln!("failed to init console logger: {e}"));

    wasm_bindgen_futures::spawn_local(memetendo_main());
}
