//! The USB sound cards of an Android phone, which `UsbAudio.kt` opens for the audio to drive
//! them itself ([`prisme_audio_io::UsbHost`]), once the DJ let Prisme open them: Android asks,
//! and `UsbAudio.kt` tells the answer ([`prisme_audio_io::usb_asks`]).

use crate::android_jni;
use crate::state::{AppState, Section};
use jni::objects::{JByteArray, JClass, JObject, JObjectArray, JString, JValue};
use jni::sys::jboolean;
use jni::JNIEnv;
use prisme_audio_io::{usb_asks, OpenedCard, UsbCard, UsbHost};
use tauri::Manager;

const CLASS: &str = "com.shawiizz.prisme.UsbAudio";

/// Gives the audio its way to the cards.
pub fn install() {
    prisme_audio_io::set_usb_host(Box::new(AndroidUsb));
}

struct AndroidUsb;

impl UsbHost for AndroidUsb {
    fn cards(&self) -> Vec<UsbCard> {
        call("list the USB cards", |env, class| {
            // the system's name of each card, its product's, then its model's (vendor:product)
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
            (0..length / 3)
                .map(|i| Ok(UsbCard { id: text(3 * i)?, name: text(3 * i + 1)?, model: text(3 * i + 2)? }))
                .collect()
        })
        .unwrap_or_default()
    }

    fn allowed(&self, id: &str) -> bool {
        call("ask whether a USB card may be opened", |env, class| {
            let id = JObject::from(env.new_string(id)?);
            env.call_static_method(class, "allowed", "(Ljava/lang/String;)Z", &[JValue::Object(&id)])?.z()
        })
        .unwrap_or(false)
    }

    fn ask(&self, id: &str) {
        call("ask the DJ for a USB card", |env, class| {
            let id = JObject::from(env.new_string(id)?);
            env.call_static_method(class, "ask", "(Ljava/lang/String;)V", &[JValue::Object(&id)])?;
            Ok(())
        });
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

/// `UsbAudio.answered`: the DJ answered for card `id`, or opened Prisme for it plugged in.
#[no_mangle]
pub extern "system" fn Java_com_shawiizz_prisme_UsbAudio_answered(
    mut env: JNIEnv,
    _class: JClass,
    id: JString,
    allowed: jboolean,
) {
    if let Some(id) = card(&mut env, &id) {
        usb_asks::answered(&id, allowed != 0);
        told();
    }
}

/// `UsbAudio.unplugged`: card `id` was unplugged.
#[no_mangle]
pub extern "system" fn Java_com_shawiizz_prisme_UsbAudio_unplugged(
    mut env: JNIEnv,
    _class: JClass,
    id: JString,
) {
    if let Some(id) = card(&mut env, &id) {
        usb_asks::unplugged(&id);
        told();
    }
}

fn card(env: &mut JNIEnv, id: &JString) -> Option<String> {
    env.get_string(id).map(String::from).ok()
}

/// The interface hears of what the DJ is asked.
fn told() {
    if let Some(app) = android_jni::app() {
        app.state::<AppState>().moved(Section::Audio);
    }
}

/// Runs `f` with `UsbAudio` ([`android_jni::call`]).
fn call<T>(what: &str, f: impl FnOnce(&mut JNIEnv, &JClass) -> jni::errors::Result<T>) -> Option<T> {
    android_jni::call(what, |env, context| {
        let class = android_jni::class(env, context, CLASS)?;
        f(env, &class)
    })
}
