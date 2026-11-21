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
/// `None`, the failure logged as what could not be done, if anything went wrong.
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
    clear_exception(&mut env, what);
    result.inspect_err(|e| warn!("could not {what}: {e}")).ok()
}

/// Runs `f` with this thread known to the virtual machine, attached for the time of `f` if it
/// was not: some of the system's native calls need it, and abort the application without it.
pub fn attached<T>(f: impl FnOnce() -> T) -> T {
    let android = ndk_context::android_context();
    // SAFETY: the virtual machine is the one the application runs in, which keeps it alive.
    match unsafe { JavaVM::from_raw(android.vm().cast()) } {
        Ok(vm) => match vm.attach_current_thread() {
            Ok(mut env) => {
                let result = f();
                clear_exception(&mut env, "call the system");
                result
            }
            Err(e) => {
                warn!("thread not attached to the virtual machine: {e}");
                f()
            }
        },
        Err(e) => {
            warn!("no virtual machine to attach the thread to: {e}");
            f()
        }
    }
}

/// Tells and clears an exception thrown while trying to do `what`: left pending, it would bring
/// the next call of this thread down, and the application with it once the thread is detached.
fn clear_exception(env: &mut JNIEnv, what: &str) {
    if env.exception_check().unwrap_or(false) {
        warn!("Java exception while trying to {what}");
        let _ = env.exception_describe();
        let _ = env.exception_clear();
    }
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
