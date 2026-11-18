//! StreamingCodecDisabler (Equicord `src/equicordplugins/streamingCodecDisabler`, GPL-3.0-or-later): the video codecs
//! it turns off go into the codec list of a voice connection with `encode: false` (`filterCodecs`). This client offers
//! H264 only, so `disableH264Codec` is the one with something to act on; with it on the camera and the stream find no
//! codec to send. Not part of the pinned client.

use crate::plugins::State;
use serde_json::Value;

const ID: &str = "StreamingCodecDisabler";

fn h264_off(state: &State) -> bool {
    let flag = |key: &str| state.value(ID, key) == Value::Bool(true);
    state.is_on(ID) && flag("disableH264Codec")
}

/// Before each frame: the voice side's codec filter follows the setting (an atomic store; off it is false).
pub(in crate::ui) fn sync_streaming_codecs() {
    let off = !crate::plugins::bypassed() && crate::plugins::with(h264_off);
    dc_voice::set_h264_encode_disabled(off);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_switch_takes_h264_out() {
        let mut state = State::default();
        assert!(!h264_off(&state));
        state.set_enabled(ID, true);
        assert!(!h264_off(&state));
        state.set_value(ID, "disableH264Codec", &Value::Bool(true));
        assert!(h264_off(&state));
    }
}
