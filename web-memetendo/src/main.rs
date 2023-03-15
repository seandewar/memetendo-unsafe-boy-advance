#![warn(clippy::pedantic)]

use std::{cell::RefCell, mem::take, panic, rc::Rc};

use anyhow::{anyhow, Result};
use js_sys::{Reflect, Uint8Array};
use libmemetendo::{
    audio, bios,
    cart::{self, Cartridge},
    gba::Gba,
    keypad::Key,
    video::screen::{self, FrameBuffer},
};
use log::{info, Level};
use wasm_bindgen::{prelude::*, Clamped, JsCast};
use web_sys::{
    CanvasRenderingContext2d, Document, Event, FileReader, HtmlCanvasElement, HtmlInputElement,
    ImageData, KeyboardEvent, Window,
};

struct Screen {
    canvas_ctx: CanvasRenderingContext2d,
    new_frame: bool,
}

impl screen::Screen for Screen {
    fn finished_frame(&mut self, frame: &FrameBuffer) {
        self.new_frame = true;

        // TODO: cringe
        let mut buf = [0xff; 4 * screen::WIDTH * screen::HEIGHT];
        for y in 0..screen::HEIGHT {
            for x in 0..screen::WIDTH {
                let i = 4 * (y * screen::WIDTH + x);
                let rgb = frame.pixel(x, y);
                buf[i] = rgb.r;
                buf[i + 1] = rgb.g;
                buf[i + 2] = rgb.b;
            }
        }

        // TODO: write some JS glue to avoid creating a new ImageData each time...
        #[allow(clippy::cast_possible_truncation)]
        let image_data = ImageData::new_with_u8_clamped_array_and_sh(
            Clamped(&buf[..]),
            screen::WIDTH as _,
            screen::HEIGHT as _,
        )
        .unwrap();

        self.canvas_ctx
            .put_image_data(&image_data, 0.0, 0.0)
            .unwrap();
    }
}

impl Screen {
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
            .ok_or_else(|| anyhow!("failed to get 2D canvas rendering context"))?;

        Ok(Self {
            canvas_ctx,
            new_frame: false,
        })
    }

    fn clear(&self) {
        #[allow(clippy::cast_precision_loss)]
        self.canvas_ctx
            .clear_rect(0.0, 0.0, screen::WIDTH as _, screen::HEIGHT as _);
    }
}

struct Context {
    window: Window,
    document: Document,
    screen: Rc<RefCell<Screen>>, // TODO: Rc<RefCell<>> sucks; is there a good way around it?
    gba: Option<Gba>,
    updater: Option<Closure<dyn Fn(f64)>>,
    max_frame_skip: u32,
    next_frame_ms: Option<f64>,
    selected_bios_rom: Option<bios::Rom>,
    selected_cart_rom: Option<cart::Rom>,
}

impl Context {
    fn new(window: &Window) -> Result<Self> {
        Ok(Self {
            window: window.clone(),
            document: window.document().unwrap(),
            screen: Rc::new(RefCell::new(Screen::new(window)?)),
            gba: None,
            updater: None,
            max_frame_skip: 3,
            next_frame_ms: None,
            selected_bios_rom: None,
            selected_cart_rom: None,
        })
    }
}

fn maybe_start_emulation(ctx: &Rc<RefCell<Context>>) -> bool {
    let mut borrowed_ctx = ctx.borrow_mut();
    let Some(ref bios_rom) = borrowed_ctx.selected_bios_rom else {
        return false;
    };
    let Some(ref cart_rom) = borrowed_ctx.selected_cart_rom else {
        return false;
    };

    let backup_type = cart_rom.parse_backup_type();
    info!("starting emulation - using cart backup type: {backup_type:?}");
    borrowed_ctx.gba = Some(Gba::new(
        bios_rom.clone(),
        Cartridge::new(cart_rom.clone(), backup_type),
    ));
    borrowed_ctx.screen.borrow().clear();
    borrowed_ctx.next_frame_ms = None;

    if borrowed_ctx.updater.is_none() {
        borrowed_ctx.updater = Some({
            let ctx = Rc::clone(ctx);
            let screen = Rc::clone(&borrowed_ctx.screen);

            Closure::new(move |ms: f64| {
                const FRAME_DURATION_MS: f64 = 1000.0 / 59.737;

                let mut borrowed_ctx = ctx.borrow_mut();
                let max_frame_skip = borrowed_ctx.max_frame_skip;
                let mut next_frame_ms = borrowed_ctx.next_frame_ms.unwrap_or(ms);

                let Some(ref mut gba) = borrowed_ctx.gba else {
                    borrowed_ctx.updater = None;
                    return;
                };
                let mut screen = screen.borrow_mut();

                let mut skipped_frames = 0;
                loop {
                    while !take(&mut screen.new_frame) {
                        // TODO: audio
                        gba.step(&mut *screen, &mut audio::NullCallback, skipped_frames > 0);
                    }

                    next_frame_ms += FRAME_DURATION_MS;
                    if next_frame_ms > ms {
                        borrowed_ctx.next_frame_ms = Some(next_frame_ms);
                        break;
                    }

                    if skipped_frames >= max_frame_skip {
                        // Too far behind; reschedule for the next frame.
                        borrowed_ctx.next_frame_ms = None;
                        break;
                    }
                    skipped_frames += 1;
                }

                borrowed_ctx
                    .window
                    .request_animation_frame(
                        borrowed_ctx
                            .updater
                            .as_ref()
                            .unwrap()
                            .as_ref()
                            .unchecked_ref(),
                    )
                    .unwrap();
            })
        });

        borrowed_ctx
            .window
            .request_animation_frame(
                borrowed_ctx
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

fn main() {
    panic::set_hook(Box::new(console_error_panic_hook::hook));
    console_log::init_with_level(Level::Info)
        .unwrap_or_else(|e| eprintln!("failed to init console logger: {e}"));

    let window = web_sys::window().unwrap();
    let ctx = match Context::new(&window) {
        Ok(ctx) => Rc::new(RefCell::new(ctx)),
        Err(e) => {
            alert(&window, format!("Loading failed: {e}"));
            return;
        }
    };

    init_file_input(&ctx.borrow(), "memetendo-bios-file", {
        let ctx = Rc::clone(&ctx);
        move |rom_buf: Vec<u8>| {
            let Ok(rom) = bios::Rom::new(Rc::from(rom_buf)) else {
                alert(&ctx.borrow().window, "Invalid BIOS ROM size!");
                return;
            };
            ctx.borrow_mut().selected_bios_rom = Some(rom);
            maybe_start_emulation(&ctx);
        }
    });
    init_file_input(&ctx.borrow(), "memetendo-cart-file", {
        let ctx = Rc::clone(&ctx);
        move |rom_buf: Vec<u8>| {
            let Ok(rom) = cart::Rom::new(Rc::from(rom_buf)) else {
                alert(&ctx.borrow().window, "Invalid cartridge ROM size!");
                return;
            };
            ctx.borrow_mut().selected_cart_rom = Some(rom);
            maybe_start_emulation(&ctx);
        }
    });

    let document = window.document().unwrap();
    document
        .add_event_listener_with_callback(
            "keydown",
            create_keypress_handler(&ctx, true)
                .into_js_value()
                .unchecked_ref(),
        )
        .unwrap();
    document
        .add_event_listener_with_callback(
            "keyup",
            create_keypress_handler(&ctx, false)
                .into_js_value()
                .unchecked_ref(),
        )
        .unwrap();

    let frame_skip_input = document
        .get_element_by_id("memetendo-frame-skip")
        .unwrap()
        .dyn_into::<HtmlInputElement>()
        .unwrap();
    frame_skip_input.set_value(&ctx.borrow().max_frame_skip.to_string());
    frame_skip_input
        .add_event_listener_with_callback("change", {
            let ctx = Rc::clone(&ctx);
            Closure::<dyn Fn(_)>::new(move |event: Event| {
                let input = event
                    .target()
                    .unwrap()
                    .dyn_into::<HtmlInputElement>()
                    .unwrap();
                ctx.borrow_mut().max_frame_skip = input.value().parse().unwrap();
            })
            .into_js_value()
            .unchecked_ref()
        })
        .unwrap();
}

// TODO: uses event.code(), so we need to have some sort of prompt that shows the actual key if the
// keyboard layout isn't QWERTY.
fn create_keypress_handler(
    ctx: &Rc<RefCell<Context>>,
    pressed: bool,
) -> Closure<dyn FnMut(KeyboardEvent)> {
    let ctx = Rc::clone(ctx);
    Closure::new(move |event: KeyboardEvent| {
        let Some(ref mut gba) = ctx.borrow_mut().gba else {
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

fn init_file_input(ctx: &Context, id: &str, mut callback: impl FnMut(Vec<u8>) + 'static) {
    let reader = FileReader::new().unwrap();
    reader
        .add_event_listener_with_callback("loadend", {
            let window = ctx.window.clone();
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

    let input = ctx
        .document
        .get_element_by_id(id)
        .unwrap()
        .dyn_into::<HtmlInputElement>()
        .unwrap();
    input.set_value("");
    input
        .add_event_listener_with_callback("change", {
            let window = ctx.window.clone();
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
