registerProcessor("audio-processor",
    class AudioProcessor extends AudioWorkletProcessor {
        constructor(options) {
            super();
            this.port.onmessage = (event) => this.queueSamples(event.data);

            // Store at most 0.25 seconds worth of samples in two ring buffers;
            // one for each channel (always assumes two channels).
            this.cap = sampleRate * 0.25;
            this.startIdx = 0;
            this.len = 0;
            this.samples = [
                new Float32Array(this.cap),
                new Float32Array(this.cap)
            ];
        }

        // allSamples is a Float32Array of all the samples for each of the two
        // channels concatenated together.
        queueSamples(allSamples) {
            const samplesLen = allSamples.length / 2;
            const samplesCopyLen = Math.min(samplesLen, this.cap);
            const appendLen = Math.min(samplesCopyLen, this.cap - this.len);
            const copy1Len = Math.min(
                appendLen,
                this.cap - (this.startIdx + this.len) % this.cap
            );
            const replaceLen = samplesCopyLen - appendLen;
            const copy3Len = Math.min(replaceLen, this.cap - this.startIdx);

            for (let i = 0; i < 2; ++i) {
                const ringBuf = this.samples[i];
                // If the number of samples is larger than the capacity, just
                // copy the newest ones.
                const samplesStartIdx =
                    (i * samplesLen) + (samplesLen - samplesCopyLen);
                const samples = allSamples.subarray(
                        samplesStartIdx,
                        samplesStartIdx + samplesCopyLen
                );
                // If not full, copy up to the remaining capacity.
                const appendSamples = samples.subarray(0, appendLen);
                ringBuf.set(
                    appendSamples.subarray(0, copy1Len),
                    (this.startIdx + this.len) % this.cap
                );
                ringBuf.set(appendSamples.subarray(copy1Len));
                // Replace oldest entries when at capacity.
                const replaceSamples = samples.subarray(appendLen);
                ringBuf.set(
                    replaceSamples.subarray(0, copy3Len),
                    this.startIdx
                );
                ringBuf.set(replaceSamples.subarray(copy3Len));
            }

            this.startIdx = (this.startIdx + replaceLen) % this.cap;
            this.len += appendLen;
        }

        process(inputs, outputs, params) {
            const chans = outputs[0];
            const samplesCopyLen = Math.min(chans[0].length, this.len);
            const copy1Len = Math.min(samplesCopyLen, this.cap - this.startIdx);

            for (let i = 0; i < 2; ++i) {
                const ringBuf = this.samples[i];
                chans[i].set(
                    ringBuf.subarray(this.startIdx, this.startIdx + copy1Len)
                );
                chans[i].set(
                    ringBuf.subarray(0, samplesCopyLen - copy1Len),
                    copy1Len
                );
            }

            this.startIdx = (this.startIdx + samplesCopyLen) % this.cap;
            this.len -= samplesCopyLen;
            return true;
        }
    }
);
