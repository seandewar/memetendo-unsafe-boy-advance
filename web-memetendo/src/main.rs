#![warn(clippy::pedantic)]

use std::{cell::RefCell, mem::take, panic, rc::Rc};

use js_sys::{Reflect, Uint8Array};
use libmemetendo::{
    audio, bios,
    cart::{self, Cartridge},
    gba::Gba,
    video::screen::{self, FrameBuffer},
};
use log::{info, Level};
use wasm_bindgen::{prelude::*, Clamped, JsCast};
use web_sys::{
    CanvasRenderingContext2d, Document, Element, Event, FileReader, HtmlCanvasElement,
    HtmlInputElement, ImageData, Window,
};

fn init_file_input(
    window: &Window,
    input: &Element,
    mut callback: impl FnMut(Vec<u8>) + 'static,
) -> Result<(), JsValue> {
    let reader = FileReader::new()?;
    {
        let window = window.clone();
        reader.set_onloadend(Some(
            Closure::<dyn FnMut(_)>::new(move |event: Event| {
                let reader = event.target().unwrap().dyn_into::<FileReader>().unwrap();
                let array_buf = reader.result().unwrap();
                if array_buf.is_null() {
                    let dom_exception = reader.error().unwrap();
                    window
                        .alert_with_message(&format!(
                            "Failed to open file: {} (code {}).",
                            dom_exception.message(),
                            dom_exception.code(),
                        ))
                        .unwrap();
                    return;
                }

                callback(Uint8Array::new(&array_buf).to_vec());
            })
            .into_js_value()
            .unchecked_ref(),
        ));
    }

    {
        let window = window.clone();
        input.add_event_listener_with_callback(
            "change",
            Closure::<dyn Fn(_)>::new(move |event: Event| {
                let input = event
                    .target()
                    .unwrap()
                    .dyn_into::<HtmlInputElement>()
                    .unwrap();
                let files = input.files().unwrap();
                if files.length() != 1 {
                    window
                        .alert_with_message("One file must be selected!")
                        .unwrap();
                    return;
                }

                let file = files.item(0).unwrap();
                reader.read_as_array_buffer(&file).unwrap();
            })
            .into_js_value()
            .unchecked_ref(),
        )
    }
}

struct Context {
    window: Window,
    document: Document,
    screen: Rc<RefCell<Screen>>, // TODO: this sucks

    gba: Option<Gba>,
    selected_bios_rom: Option<bios::Rom>,
    selected_cart_rom: Option<cart::Rom>,
}

struct Screen {
    canvas_ctx: CanvasRenderingContext2d,
    new_frame: bool,
}

impl screen::Screen for Screen {
    fn finished_frame(&mut self, frame: &FrameBuffer) {
        self.new_frame = true;

        // TODO: cringe
        let mut buf = [0u8; 4 * screen::WIDTH * screen::HEIGHT];
        for y in 0..screen::HEIGHT {
            for x in 0..screen::WIDTH {
                let i = 4 * (y * screen::WIDTH + x);
                let rgb = frame.pixel(x, y);
                buf[i] = rgb.r;
                buf[i + 1] = rgb.g;
                buf[i + 2] = rgb.b;
                buf[i + 3] = 0xff;
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

fn maybe_start_emulation(ctx: &Rc<RefCell<Context>>) -> Result<bool, JsValue> {
    let mut borrowed = ctx.borrow_mut();
    let Some(ref bios_rom) = borrowed.selected_bios_rom else {
        return Ok(false);
    };
    let Some(ref cart_rom) = borrowed.selected_cart_rom else {
        return Ok(false);
    };

    let backup_type = cart_rom.parse_backup_type();
    info!("starting emulation - using cart backup type: {backup_type:?}");

    borrowed.gba = Some(Gba::new(
        bios_rom.clone(),
        Cartridge::new(cart_rom.clone(), backup_type),
    ));

    let ctx = Rc::clone(ctx);
    let screen = Rc::clone(&borrowed.screen);

    // TODO: omgwtfbbq?!?!?!?
    let closure: Rc<RefCell<Option<Closure<dyn Fn()>>>> = Rc::new(RefCell::new(None));
    let closure_cloned = Rc::clone(&closure);
    *closure_cloned.borrow_mut() = Some(Closure::new(move || {
        // TODO: sucks
        let mut borrowed = ctx.borrow_mut();
        let mut screen = screen.borrow_mut();
        while !take(&mut screen.new_frame) {
            borrowed
                .gba
                .as_mut()
                .unwrap()
                .step(&mut *screen, &mut audio::NullCallback, false);
        }

        borrowed
            .window
            .request_animation_frame(closure.borrow().as_ref().unwrap().as_ref().unchecked_ref())
            .unwrap();
    }));

    borrowed.window.request_animation_frame(
        closure_cloned
            .borrow()
            .as_ref()
            .unwrap()
            .as_ref()
            .unchecked_ref(),
    )?;
    Ok(true)
}

fn main() {
    panic::set_hook(Box::new(console_error_panic_hook::hook));
    console_log::init_with_level(Level::Info)
        .unwrap_or_else(|e| eprintln!("failed to init console logger: {e}"));

    let window = web_sys::window().unwrap();
    let document = window.document().unwrap();

    let screen_canvas = document
        .get_element_by_id("memetendo-screen")
        .unwrap()
        .dyn_into::<HtmlCanvasElement>()
        .unwrap();
    let Some(screen_canvas_ctx) = screen_canvas
        .get_context_with_context_options("2d", &*{
            let options = js_sys::Object::new();
            Reflect::set(&options, &"alpha".into(), &false.into()).unwrap();
            Reflect::set(&options, &"desynchronized".into(), &true.into()).unwrap();
            options
        })
        .unwrap()
        .map(|ctx| ctx.dyn_into::<CanvasRenderingContext2d>().unwrap())
    else {
        window
            .alert_with_message("Loading failed: could not get canvas 2D drawing context!")
            .unwrap();
        return;
    };

    let ctx = Rc::new(RefCell::new(Context {
        window: window.clone(),
        document: document.clone(),
        screen: Rc::new(RefCell::new(Screen {
            canvas_ctx: screen_canvas_ctx,
            new_frame: false,
        })),
        gba: None,
        selected_bios_rom: None,
        selected_cart_rom: None,
    }));

    init_file_input(
        &window,
        &document.get_element_by_id("memetendo-bios-file").unwrap(),
        {
            let ctx = Rc::clone(&ctx);
            move |rom_buf: Vec<u8>| {
                let Ok(rom) = bios::Rom::new(Rc::from(rom_buf)) else {
                    ctx.borrow().window
                        .alert_with_message("Invalid BIOS ROM size!")
                        .unwrap();
                    return;
                };

                ctx.borrow_mut().selected_bios_rom = Some(rom);
                maybe_start_emulation(&ctx).unwrap();
            }
        },
    )
    .expect("failed to init BIOS ROM file input");

    init_file_input(
        &window,
        &document.get_element_by_id("memetendo-cart-file").unwrap(),
        {
            let ctx = Rc::clone(&ctx);
            move |rom_buf: Vec<u8>| {
                let Ok(rom) = cart::Rom::new(Rc::from(rom_buf)) else {
                    ctx.borrow().window
                        .alert_with_message("Invalid cartridge ROM size!")
                        .unwrap();
                    return;
                };

                ctx.borrow_mut().selected_cart_rom = Some(rom);
                maybe_start_emulation(&ctx).unwrap();
            }
        },
    )
    .expect("failed to init cartridge ROM file input");
}
