//! The runtime image `tr build` and `tr run` link against: every
//! runtime staticlib in link search order (`runtime_staticlib_envs!`
//! documents why that order), preprocessed when tr itself is compiled
//! and stripped to the atoms some program can reach.

pub static TORAJS_RUNTIME_IMAGE: &[u8] =
    torajs_core::runtime_staticlib_envs!(torajs_rtlinked_macro::runtime_image);
