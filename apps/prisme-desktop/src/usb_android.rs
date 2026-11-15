//! The USB sound cards of an Android phone, which `UsbAudio.kt` opens for the audio to drive
//! them itself ([`prisme_audio_io::UsbHost`]).

use jni::objects::{JByteArray, JClass, JObject, JObjectArray, JString, JValue};
use jni::{JNIEnv, JavaVM};
use prisme_audio_io::{OpenedCard, UsbHost};
use tracing::warn;

const CLASS: &str = "com.shawiizz.prisme.UsbAudio";

/// Gives the audio its way to the cards.
pub fn install() {
    prisme_audio_io::set_usb_host(Box::new(AndroidUsb));
}

struct AndroidUsb;

impl UsbHost for AndroidUsb {
    fn cards(&self) -> Vec<String> {
        call("list the USB cards", |env, class| {
            let array = JObjectArray::from(env.call_static_method(class, "cards", "()[Ljava/lang/String;", &[])?.l()?);
            (0..env.get_array_length(&array)?)
                .map(|i| {
                    let name = JString::from(env.get_object_array_element(&array, i)?);
                    let name: String = env.get_string(&name)?.into();
                    Ok(name)
                })
                .collect()
        })
        .unwrap_or_default()
    }

    fn open(&self, name: &str) -> Option<OpenedCard> {
        call("open a USB card", |env, class| {
            let jname = JObject::from(env.new_string(name)?);
            let fd = env.call_static_method(class, "open", "(Ljava/lang/String;)I", &[JValue::Object(&jname)])?.i()?;
            if fd < 0 {
                return Ok(None);
            }
            let raw = JByteArray::from(
                env.call_static_method(class, "descriptors", "(Ljava/lang/String;)[B", &[JValue::Object(&jname)])?.l()?,
            );
            if raw.is_null() {
                return Ok(None);
            }
            Ok(Some(OpenedCard { fd, descriptors: env.convert_byte_array(&raw)? }))
        })
        .flatten()
    }

    fn close(&self, name: &str) {
        call("give a USB card back", |env, class| {
            let jname = JObject::from(env.new_string(name)?);
            env.call_static_method(class, "close", "(Ljava/lang/String;)V", &[JValue::Object(&jname)])?;
            Ok(())
        });
    }
}

/// Runs `f` with `UsbAudio`, found through the application's class loader (a native thread's
/// own sees the system's classes only); `None`, the failure logged, if anything went wrong.
fn call<T>(what: &str, f: impl FnOnce(&mut JNIEnv, &JClass) -> jni::errors::Result<T>) -> Option<T> {
    let context = ndk_context::android_context();
    // SAFETY: the virtual machine and the activity are those the application runs in, which
    // keeps both alive.
    let (vm, activity) = unsafe {
        match JavaVM::from_raw(context.vm().cast()) {
            Ok(vm) => (vm, JObject::from_raw(context.context().cast())),
            Err(e) => {
                warn!("could not {what}: {e}");
                return None;
            }
        }
    };
    let mut env = vm.attach_current_thread().inspect_err(|e| warn!("could not {what}: {e}")).ok()?;
    let result = env.with_local_frame(16, |env| {
        let loader = env.call_method(&activity, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])?.l()?;
        let name = JObject::from(env.new_string(CLASS)?);
        let class = JClass::from(
            env.call_method(&loader, "loadClass", "(Ljava/lang/String;)Ljava/lang/Class;", &[JValue::Object(&name)])?
                .l()?,
        );
        f(env, &class)
    });
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_describe();
        let _ = env.exception_clear();
    }
    result.inspect_err(|e| warn!("could not {what}: {e}")).ok()
}
