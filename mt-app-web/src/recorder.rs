//! The browser capture adapter: microphone → `Vec<f32>`. Pure glue over
//! web-sys — the only "decisions" here are the ones the browser forces on
//! us (constraints, graph wiring, teardown order). Anything resembling
//! analysis belongs in `mt-pitch`; anything resembling a state machine
//! belongs in `mt-session`.

use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use wasm_bindgen::closure::Closure;
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    AudioContext, AudioWorkletNode, MediaStream, MediaStreamAudioSourceNode,
    MediaStreamConstraints, MediaStreamTrack, MediaTrackConstraints, MessageEvent, MessagePort,
};

/// The processor name `public/recorder-worklet.js` registers.
const PROCESSOR_NAME: &str = "recorder-processor";

/// Where the worklet module is fetched from.
///
/// Root-relative on purpose. `mt-app-web/public/` is `Dioxus.toml`'s
/// `asset_dir`, and `dx` 0.7.9 copies its contents verbatim to the bundle
/// root — so this one URL resolves identically under `dx serve` and under
/// `mt-server`'s `ServeDir` of the built bundle. Note this file must *not*
/// go through the `asset!()` macro: `AudioWorklet.addModule` needs a plain
/// URL to a module the browser fetches at runtime, and the app never
/// imports it, so there is nothing for the asset pipeline to rewrite.
pub const WORKLET_URL: &str = "/recorder-worklet.js";

/// Shown when the browser hands back a blank track label (it does that
/// before permission is granted, and on some privacy-hardened setups).
const UNKNOWN_DEVICE: &str = "unknown input device";

/// One finished take: the samples, plus the two facts about them that only
/// the capture side knows.
#[derive(Clone, Debug, PartialEq)]
pub struct Recording {
    pub samples: Vec<f32>,
    /// The `AudioContext`'s **actual** rate, never an assumed 48000 (Q12).
    pub sample_rate: u32,
    pub device_label: String,
}

/// A live capture graph. Constructed by [`Recorder::start`], consumed by
/// [`Recorder::stop`]; holding one means the mic is open.
pub struct Recorder {
    ctx: AudioContext,
    stream: MediaStream,
    source: MediaStreamAudioSourceNode,
    node: AudioWorkletNode,
    port: MessagePort,
    chunks: Rc<RefCell<Vec<f32>>>,
    /// Owns the JS callback the worklet port posts into. Dropping the
    /// `Recorder` frees it, which is why the port is detached in `stop`
    /// before that can happen.
    _on_message: Closure<dyn FnMut(MessageEvent)>,
    sample_rate: u32,
    device_label: String,
}

impl Recorder {
    /// Opens the mic and starts collecting. Must be called from a user
    /// gesture: the `AudioContext` is constructed first, synchronously, so
    /// it inherits the click's user activation rather than resuming from a
    /// suspended state after the `getUserMedia` await.
    pub async fn start() -> Result<Self, String> {
        let ctx = AudioContext::new().map_err(|err| js_err("create AudioContext", &err))?;
        let wired = Self::wire_up(ctx.clone()).await;
        if wired.is_err() {
            // Don't leave an orphaned context (and its hardware claim)
            // behind when permission is denied or the worklet won't load.
            let _ = ctx.close();
        }
        wired
    }

    async fn wire_up(ctx: AudioContext) -> Result<Self, String> {
        let window = web_sys::window().ok_or("no window object")?;
        let media_devices = window
            .navigator()
            .media_devices()
            .map_err(|err| js_err("access navigator.mediaDevices", &err))?;

        // Raw voice: every browser "helpfulness" filter off, because they
        // all distort pitch and level, which is the thing being measured.
        let audio = MediaTrackConstraints::new();
        audio.set_echo_cancellation_bool(false);
        audio.set_noise_suppression_bool(false);
        audio.set_auto_gain_control_bool(false);
        let constraints = MediaStreamConstraints::new();
        constraints.set_audio(&audio);
        constraints.set_video(&JsValue::FALSE);

        let stream: MediaStream = JsFuture::from(
            media_devices
                .get_user_media_with_constraints(&constraints)
                .map_err(|err| js_err("call getUserMedia", &err))?,
        )
        .await
        .map_err(|err| js_err("get microphone permission", &err))?
        .unchecked_into();

        let device_label = stream
            .get_audio_tracks()
            .get(0)
            .dyn_into::<MediaStreamTrack>()
            .map(|track| track.label())
            .unwrap_or_default();
        let device_label = if device_label.trim().is_empty() {
            UNKNOWN_DEVICE.to_string()
        } else {
            device_label
        };

        JsFuture::from(
            ctx.audio_worklet()
                .map_err(|err| js_err("reach AudioContext.audioWorklet", &err))?
                .add_module(WORKLET_URL)
                .map_err(|err| js_err("call addModule", &err))?,
        )
        .await
        .map_err(|err| js_err(&format!("load worklet module {WORKLET_URL}"), &err))?;

        JsFuture::from(
            ctx.resume()
                .map_err(|err| js_err("resume AudioContext", &err))?,
        )
        .await
        .map_err(|err| js_err("resume AudioContext", &err))?;

        let node = AudioWorkletNode::new(&ctx, PROCESSOR_NAME)
            .map_err(|err| js_err("construct AudioWorkletNode", &err))?;
        let port = node
            .port()
            .map_err(|err| js_err("reach the worklet's message port", &err))?;

        let chunks = Rc::new(RefCell::new(Vec::<f32>::new()));
        let sink = Rc::clone(&chunks);
        let on_message = Closure::<dyn FnMut(MessageEvent)>::new(move |event: MessageEvent| {
            if let Ok(block) = event.data().dyn_into::<js_sys::Float32Array>() {
                sink.borrow_mut().extend_from_slice(&block.to_vec());
            }
        });
        // Assigning `onmessage` implicitly starts the port; no `start()`.
        port.set_onmessage(Some(on_message.as_ref().unchecked_ref()));

        let source = ctx
            .create_media_stream_source(&stream)
            .map_err(|err| js_err("create a MediaStreamAudioSourceNode", &err))?;
        source
            .connect_with_audio_node(&node)
            .map_err(|err| js_err("connect the mic to the worklet", &err))?;
        // Chrome only pulls a worklet node whose output reaches the
        // destination, so the graph has to terminate there. The processor
        // never writes to its output buffer, so what reaches the speakers
        // is silence — no monitoring, no feedback loop.
        node.connect_with_audio_node(&ctx.destination())
            .map_err(|err| js_err("connect the worklet to the destination", &err))?;

        let sample_rate = ctx.sample_rate().round() as u32;

        Ok(Recorder {
            ctx,
            stream,
            source,
            node,
            port,
            chunks,
            _on_message: on_message,
            sample_rate,
            device_label,
        })
    }

    /// Tears the graph down and hands back what was collected. Detaches the
    /// port first so no block can arrive after the samples are taken, then
    /// stops the tracks (this is what clears the browser's recording
    /// indicator) and closes the context.
    pub async fn stop(self) -> Recording {
        self.port.set_onmessage(None);
        let _ = self.source.disconnect();
        let _ = self.node.disconnect();

        for track in self.stream.get_tracks().iter() {
            if let Ok(track) = track.dyn_into::<MediaStreamTrack>() {
                track.stop();
            }
        }
        if let Ok(closing) = self.ctx.close() {
            let _ = JsFuture::from(closing).await;
        }

        let samples = std::mem::take(&mut *self.chunks.borrow_mut());
        Recording {
            samples,
            sample_rate: self.sample_rate,
            device_label: self.device_label,
        }
    }
}

/// Turns a `JsValue` rejection into something a human can read in the UI.
fn js_err(context: &str, err: &JsValue) -> String {
    let detail = err
        .as_string()
        .or_else(|| {
            err.dyn_ref::<js_sys::Error>()
                .map(|error| String::from(error.message()))
        })
        .unwrap_or_else(|| format!("{err:?}"));
    format!("failed to {context}: {detail}")
}
