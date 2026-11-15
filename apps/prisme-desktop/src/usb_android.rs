//! The USB sound cards of an Android phone, which `UsbAudio.kt` opens for the audio to drive
//! them itself ([`prisme_audio_io::UsbHost`]).

use crate::android_jni;
use jni::objects::{JByteArray, JClass, JObject, JObjectArray, JString, JValue};
use jni::JNIEnv;
use prisme_audio_io::{OpenedCard, UsbCard, UsbHost};

const CLASS: &str = "com.shawiizz.prisme.UsbAudio";

/// Gives the audio its way to the cards.
pub fn install() {
    prisme_audio_io::set_usb_host(Box::new(AndroidUsb));
}

struct AndroidUsb;

impl UsbHost for AndroidUsb {
    fn cards(&self) -> Vec<UsbCard> {
        call("list the USB cards", |env, class| {
            // the system's name of each card, then its product's
            let array = JObjectArray::from(
                env.call_static_method(class, "cards", "()[Ljava/lang/String;", &[])?.l()?,
            );
            let length = env.get_array_length(&array)?;
            let mut text = |i| -> jni::errors::Result<String> {
                let text = JString::from(env.get_object_array_element(&array, i)?);
                let string: String = env.get_string(&text)?.into();
                env.delete_local_ref(text)?;
                Ok(string)
            };
            (0..length / 2).map(|i| Ok(UsbCard { id: text(2 * i)?, name: text(2 * i + 1)? })).collect()
        })
        .unwrap_or_default()
    }

    fn open(&self, id: &str) -> Option<OpenedCard> {
        call("open a USB card", |env, class| {
            let id = JObject::from(env.new_string(id)?);
            let fd = env
                .call_static_method(class, "open", "(Ljava/lang/String;)I", &[JValue::Object(&id)])?
                .i()?;
            if fd < 0 {
                return Ok(None);
            }
            let raw = JByteArray::from(
                env.call_static_method(
                    class,
                    "descriptors",
                    "(Ljava/lang/String;)[B",
                    &[JValue::Object(&id)],
                )?
                .l()?,
            );
            if raw.is_null() {
                return Ok(None);
            }
            Ok(Some(OpenedCard { fd, descriptors: env.convert_byte_array(&raw)? }))
        })
        .flatten()
    }

    fn close(&self, id: &str) {
        call("give a USB card back", |env, class| {
            let id = JObject::from(env.new_string(id)?);
            env.call_static_method(class, "close", "(Ljava/lang/String;)V", &[JValue::Object(&id)])?;
            Ok(())
        });
    }
}

/// Runs `f` with `UsbAudio` ([`android_jni::call`]).
fn call<T>(what: &str, f: impl FnOnce(&mut JNIEnv, &JClass) -> jni::errors::Result<T>) -> Option<T> {
    android_jni::call(what, |env, context| {
        let class = android_jni::class(env, context, CLASS)?;
        f(env, &class)
    })
}
