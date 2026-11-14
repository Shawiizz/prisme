//! Listening to an input. Android asks the DJ before an application does, whether the
//! input is the phone's microphone or a sound card's, where the timecode of the turntables
//! comes in: it is asked when an input is first wanted, not before. iOS asks by itself as
//! the input opens, and a computer does not ask.

/// Whether the inputs may be listened to. Where they may not yet, the DJ is asked: the
/// answer comes later, and the audio is to be applied again then.
#[cfg(not(target_os = "android"))]
pub fn allowed() -> bool {
    true
}

#[cfg(target_os = "android")]
pub fn allowed() -> bool {
    android::allowed().unwrap_or_else(|e| {
        tracing::warn!("the inputs could not be asked for: {e}");
        false
    })
}

#[cfg(target_os = "android")]
mod android {
    use jni::objects::{JObject, JValue};
    use jni::JavaVM;

    const RECORD_AUDIO: &str = "android.permission.RECORD_AUDIO";
    /// `PackageManager.PERMISSION_GRANTED`
    const GRANTED: i32 = 0;

    pub fn allowed() -> jni::errors::Result<bool> {
        let context = ndk_context::android_context();
        // SAFETY: the virtual machine and the activity are those the application runs in,
        // which keeps both alive.
        let (vm, activity) =
            unsafe { (JavaVM::from_raw(context.vm().cast())?, JObject::from_raw(context.context().cast())) };
        let mut env = vm.attach_current_thread()?;
        let permission = env.new_string(RECORD_AUDIO)?;
        let state = env
            .call_method(
                &activity,
                "checkSelfPermission",
                "(Ljava/lang/String;)I",
                &[JValue::Object(&permission)],
            )?
            .i()?;
        if state == GRANTED {
            return Ok(true);
        }
        let wanted = env.new_object_array(1, "java/lang/String", &permission)?;
        env.call_method(
            &activity,
            "requestPermissions",
            "([Ljava/lang/String;I)V",
            &[JValue::Object(&wanted), JValue::Int(0)],
        )?;
        Ok(false)
    }
}
