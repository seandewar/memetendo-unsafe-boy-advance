use js_sys::Float32Array;
use libmemetendo::audio::{self, SAMPLE_FREQUENCY};
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    AudioContext, AudioContextOptions, AudioWorkletNode, AudioWorkletNodeOptions, MessagePort,
};

struct Callback {
    ctx: AudioContext,
    port: MessagePort,
    freq: u32,
    freq_counter: u32,
    freq_counter_accum: u32,
    sample_accum: (i32, i32),
    accum_extra_sample: bool,
    samples: [Vec<i16>; 2],
}

impl Callback {
    async fn new() -> Result<Self, JsValue> {
        // TODO: maybe handle other sample rates or channel counts...
        let ctx = AudioContext::new_with_context_options(
            AudioContextOptions::new().sample_rate(44_100.0),
        )?;
        #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
        let freq = ctx.sample_rate() as u32;

        JsFuture::from(ctx.audio_worklet()?.add_module("audio_processor.js")?).await?;
        let node = AudioWorkletNode::new_with_options(
            &ctx,
            "audio-processor",
            AudioWorkletNodeOptions::new().output_channel_count(&js_sys::Array::of1(&2.into())),
        )?;
        node.connect_with_audio_node(&ctx.destination())?;

        Ok(Self {
            ctx,
            freq,
            port: node.port().unwrap(),
            freq_counter: 0,
            freq_counter_accum: 0,
            sample_accum: (0, 0),
            accum_extra_sample: false,
            samples: [Vec::new(), Vec::new()],
        })
    }
}

impl audio::Callback for Callback {
    fn push_sample(&mut self, sample: (i16, i16)) {
        self.sample_accum.0 += i32::from(sample.0);
        self.sample_accum.1 += i32::from(sample.1);

        self.freq_counter += 1;
        if self.freq_counter < (SAMPLE_FREQUENCY / self.freq) + u32::from(self.accum_extra_sample) {
            return;
        }

        let sample = (
            i16::try_from(self.sample_accum.0 / i32::try_from(self.freq_counter).unwrap()).unwrap(),
            i16::try_from(self.sample_accum.1 / i32::try_from(self.freq_counter).unwrap()).unwrap(),
        );
        self.freq_counter = 0;
        self.sample_accum = (0, 0);

        // Context frequency may not divide exactly with the sample output frequency, so we may
        // drift behind by a full sample; if so, accumulate an extra sample next time.
        self.freq_counter_accum += SAMPLE_FREQUENCY % self.freq;
        self.accum_extra_sample = self.freq_counter_accum >= self.freq;
        if self.accum_extra_sample {
            self.freq_counter_accum -= self.freq;
        }

        self.samples[0].push(sample.0);
        self.samples[1].push(sample.1);
    }
}

pub struct Audio(Option<Callback>);

impl Audio {
    pub async fn new() -> Result<Self, (JsValue, Self)> {
        Callback::new()
            .await
            .map(|cb| Self(Some(cb)))
            .map_err(|e| (e, Self(None)))
    }

    pub fn resume(&self) {
        if let Some(ref cb) = self.0 {
            _ = cb.ctx.resume().unwrap();
        }
    }

    pub fn queue_samples(&mut self) {
        let Some(ref mut cb) = self.0 else {
            return;
        };

        let samples: Vec<f32> = cb
            .samples
            .iter_mut()
            .flat_map(|chan| {
                chan.drain(..)
                    .map(|sample| f32::from(sample) / -f32::from(i16::MIN))
            })
            .collect();
        cb.port
            .post_message(&Float32Array::from(&samples[..]).into())
            .unwrap();
    }
}

impl audio::Callback for Audio {
    fn push_sample(&mut self, sample: (i16, i16)) {
        if let Some(ref mut cb) = self.0 {
            cb.push_sample(sample);
        }
    }
}
