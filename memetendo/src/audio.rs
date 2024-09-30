use std::mem::size_of;

use libmemetendo::audio::{self, SAMPLE_FREQUENCY};
use log::info;
use sdl2::{
    audio::{AudioQueue, AudioSpec, AudioSpecDesired},
    AudioSubsystem,
};

struct Callback {
    spec: AudioSpec,
    freq_counter: u32,
    freq_counter_accum: u32,
    sample_accum: (i32, i32),
    accum_extra_sample: bool,

    // Circular sample buffer.
    samples: Box<[i16]>,
    samples_start_idx: usize,
    samples_len: usize,
}

impl Callback {
    fn new(spec: AudioSpec) -> Result<Self, String> {
        info!("{spec:?}");

        if spec.channels > 2 {
            return Err(format!(
                "only 1 (mono) or 2 (stereo) audio channels are currently supported (got {})",
                spec.channels
            ));
        }

        if spec.freq > i32::try_from(SAMPLE_FREQUENCY).unwrap() {
            // We could technically handle this, but it's probably not worth it.
            return Err(format!(
                "audio frequency too high (got: {} Hz, max: {SAMPLE_FREQUENCY} Hz)",
                spec.freq,
            ));
        }

        Ok(Self {
            spec,
            freq_counter: 0,
            freq_counter_accum: 0,
            sample_accum: (0, 0),
            accum_extra_sample: false,
            // Make the buffer twice the size of SDL's sample buffer. This gives us some leg room
            // in case we're writing samples slightly quicker than they're consumed.
            samples: vec![0; 2 * Self::samples_len(&spec)].into_boxed_slice(),
            samples_start_idx: 0,
            samples_len: 0,
        })
    }

    fn samples_len(spec: &AudioSpec) -> usize {
        usize::try_from(spec.size).unwrap() / size_of::<i16>()
    }
}

impl audio::Callback for Callback {
    fn push_sample(&mut self, sample: (i16, i16)) {
        self.sample_accum.0 += i32::from(sample.0);
        self.sample_accum.1 += i32::from(sample.1);

        self.freq_counter += 1;
        let freq = self.spec.freq.try_into().unwrap();
        if self.freq_counter < (SAMPLE_FREQUENCY / freq) + u32::from(self.accum_extra_sample) {
            return;
        }

        let sample = (
            i16::try_from(self.sample_accum.0 / i32::try_from(self.freq_counter).unwrap()).unwrap(),
            i16::try_from(self.sample_accum.1 / i32::try_from(self.freq_counter).unwrap()).unwrap(),
        );
        self.freq_counter = 0;
        self.sample_accum = (0, 0);

        // Driver frequency may not divide exactly with the sample output frequency, so we may
        // drift behind by a full sample; if so, accumulate an extra sample next time.
        self.freq_counter_accum += SAMPLE_FREQUENCY % freq;
        self.accum_extra_sample = self.freq_counter_accum >= freq;
        if self.accum_extra_sample {
            self.freq_counter_accum -= freq;
        }

        let mut push = |value| {
            if self.samples_len < self.samples.len() {
                let i = (self.samples_start_idx + self.samples_len) % self.samples.len();
                self.samples[i] = value;
                self.samples_len += 1;
            } else {
                // Overwrite the oldest value.
                self.samples[self.samples_start_idx] = value;
                self.samples_start_idx += 1;
                self.samples_start_idx %= self.samples.len();
            }
        };

        if self.spec.channels > 1 {
            push(sample.0);
            push(sample.1);
        } else {
            push(sample.0 / 2 + sample.1 / 2);
        }
    }
}

#[derive(Default)]
pub struct Audio(Option<(AudioQueue<i16>, Callback)>);

impl Audio {
    #[expect(clippy::result_large_err)]
    pub fn new(
        params: Option<(&AudioSubsystem, AudioSpecDesired)>,
    ) -> Result<Self, (String, Self)> {
        let Some((sdl_audio, spec)) = params else {
            return Ok(Self(None));
        };

        let queue = sdl_audio.open_queue(None, &spec).map_err(|e| {
            (
                format!("failed to create sdl2 audio queue: {e}"),
                Self(None),
            )
        })?;

        Callback::new(*queue.spec())
            .map(|cb| {
                queue.resume();
                Self(Some((queue, cb)))
            })
            .map_err(|e| (format!("failed to create audio callback: {e}"), Self(None)))
    }

    pub fn queue_samples(&mut self) -> Result<(), String> {
        let Some((queue, cb)) = self.0.as_mut() else {
            return Ok(());
        };

        // Limit the max amount of samples we can have enqueued, otherwise we risk having the
        // audio drift behind if the queue isn't being consumed fast enough.
        let count = cb
            .samples_len
            .min(Callback::samples_len(&cb.spec).saturating_sub(queue.size().try_into().unwrap()));
        if count == 0 {
            return Ok(());
        }

        let try_queue = || {
            if cb.samples_start_idx + count <= cb.samples.len() {
                queue.queue_audio(&cb.samples[cb.samples_start_idx..][..count])?;
            } else {
                // The circular buffer wrapped around, so write in two parts.
                let first_part = &cb.samples[cb.samples_start_idx..];
                queue.queue_audio(first_part)?;
                queue.queue_audio(&cb.samples[..count - first_part.len()])?;
            }

            Ok(())
        };
        let result = try_queue();
        cb.samples_start_idx += count;
        cb.samples_start_idx %= cb.samples.len();
        cb.samples_len -= count;

        result
    }
}

impl audio::Callback for Audio {
    fn push_sample(&mut self, sample: (i16, i16)) {
        if let Some((_, cb)) = self.0.as_mut() {
            cb.push_sample(sample);
        }
    }
}
