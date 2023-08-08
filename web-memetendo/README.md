# Web Memetendo

Web Memetendo is the WASM front-end for Memetendo, designed to be ran in a
web browser.

## Building

Install [wasm-pack](https://github.com/rustwasm/wasm-pack), then (assuming this
directory is the current directory) run:

```
wasm-pack build -t web --no-typescript --no-pack -d www/build --release
```

Optionally, replace the `--release` flag with `--dev` to build with debug
information enabled and optimizations disabled.

## Running

You'll need a HTTPS server that can serve the files in the `www` directory after
building (secure context is needed due to the use of
[AudioWorklets](https://developer.mozilla.org/en-US/docs/Web/API/AudioWorklet)).


Using [http](https://github.com/thecoshman/http), for example:

```
http --gen-ssl -- www
```
