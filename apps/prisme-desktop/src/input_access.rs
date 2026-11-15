//! Listening to an input. Android asks the DJ before an application does, whether the
//! input is the phone's microphone or a sound card's, where the timecode of the turntables
//! comes in: it is asked when an input is first wanted, not before. iOS asks by itself as
//! the input opens, and a computer does not ask.

/// Whether the inputs may be listened to. Where they may not yet, the DJ is asked: the
/// answer comes later ([`newly_allowed`]), and the audio is to be applied again then.
#[cfg(not(target_os = "android"))]
pub fn allowed() -> bool {
    true
}

/// Whether the DJ let Prisme listen to the inputs since this was last asked.
#[cfg(not(target_os = "android"))]
pub fn newly_allowed() -> bool {
    false
}

#[cfg(target_os = "android")]
pub use android::{allowed, newly_allowed};

#[cfg(target_os = "android")]
mod android {
    use crate::android_jni;
    use jni::objects::{JObject, JValue};
    use jni::JNIEnv;
    use std::sync::atomic::{AtomicBool, Ordering};

    const RECORD_AUDIO: &str = "android.permission.RECORD_AUDIO";
    /// `PackageManager.PERMISSION_GRANTED`
    const GRANTED: i32 = 0;
    /// Where the DJ is asked (`MainActivity.kt`).
    const ACTIVITY: &str = "com.shawiizz.prisme.MainActivity";

    /// The DJ let Prisme listen to the inputs, and the audio has not looked since.
    static ALLOWED: AtomicBool = AtomicBool::new(false);

    pub fn allowed() -> bool {
        android_jni::call("ask for the inputs", |env, context| {
            let permission = env.new_string(RECORD_AUDIO)?;
            let state = env
                .call_method(
                    context,
                    "checkSelfPermission",
                    "(Ljava/lang/String;)I",
                    &[JValue::Object(&permission)],
                )?
                .i()?;
            if state == GRANTED {
                return Ok(true);
            }
            let activity = android_jni::class(env, context, ACTIVITY)?;
            env.call_static_method(&activity, "askForTheInputs", "()V", &[])?;
            Ok(false)
        })
        .unwrap_or(false)
    }

    pub fn newly_allowed() -> bool {
        ALLOWED.swap(false, Ordering::AcqRel)
    }

    /// `MainActivity.inputsAllowed`: the DJ said yes.
    #[no_mangle]
    pub extern "system" fn Java_com_shawiizz_prisme_MainActivity_inputsAllowed(
        _env: JNIEnv,
        _activity: JObject,
    ) {
        ALLOWED.store(true, Ordering::Release);
    }
}
