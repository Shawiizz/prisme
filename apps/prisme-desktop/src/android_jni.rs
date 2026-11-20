//! Calls from Rust into the Java side of the Android application, from any thread, and the
//! application the Java side's calls into Rust reach.

use jni::objects::{JClass, JObject, JValue};
use jni::{JNIEnv, JavaVM};
use std::sync::OnceLock;
use tauri::AppHandle;
use tracing::warn;

static APP: OnceLock<AppHandle> = OnceLock::new();

/// Lets the Java side's calls reach the application, which is to be set up already.
pub fn install(app: &AppHandle) {
    let _ = APP.set(app.clone());
}

/// The application, once set up.
pub fn app() -> Option<&'static AppHandle> {
    APP.get()
}

/// Runs `f` with the application's context, in a frame of its own for the references it makes;
/// `None`, the failure logged as what could not be done, if anything went wrong. An exception
/// thrown meanwhile is told and cleared: left pending, it would bring the next call of this
/// thread down.
pub fn call<T>(what: &str, f: impl FnOnce(&mut JNIEnv, &JObject) -> jni::errors::Result<T>) -> Option<T> {
    let android = ndk_context::android_context();
    // SAFETY: the virtual machine and the context are those the application runs in, which
    // keeps both alive.
    let (vm, context) = unsafe {
        match JavaVM::from_raw(android.vm().cast()) {
            Ok(vm) => (vm, JObject::from_raw(android.context().cast())),
            Err(e) => {
                warn!("could not {what}: {e}");
                return None;
            }
        }
    };
    let mut env = vm.attach_current_thread().inspect_err(|e| warn!("could not {what}: {e}")).ok()?;
    let result = env.with_local_frame(16, |env| f(env, &context));
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_describe();
        let _ = env.exception_clear();
    }
    result.inspect_err(|e| warn!("could not {what}: {e}")).ok()
}

/// The application's class `name` (`com.shawiizz.prisme.UsbAudio`), found through its class
/// loader: a native thread's own sees the system's classes only.
pub fn class<'local>(
    env: &mut JNIEnv<'local>,
    context: &JObject,
    name: &str,
) -> jni::errors::Result<JClass<'local>> {
    let loader = env.call_method(context, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])?.l()?;
    let name = env.new_string(name)?;
    let class = env.call_method(
        &loader,
        "loadClass",
        "(Ljava/lang/String;)Ljava/lang/Class;",
        &[JValue::Object(&name)],
    )?;
    Ok(JClass::from(class.l()?))
}
