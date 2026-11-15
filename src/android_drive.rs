use std::{
    thread,
    time::{Duration, Instant},
};

use jni::{
    jni_sig, jni_str,
    objects::{JObject, JString, JValue},
    refs::Global,
    Env, JavaVM,
};

use crate::oauth::AuthResult;

const UNAVAILABLE: &str = "Google Drive authorization is unavailable in this Android build.";

pub fn available() -> bool {
    take_result().is_ok()
}

pub fn authorize(interactive: bool) -> AuthResult<String> {
    with_activity(|env, activity| {
        env.call_method(
            activity,
            jni_str!("startDriveAuthorization"),
            jni_sig!("(Z)V"),
            &[JValue::Bool(interactive)],
        )?;
        Ok(())
    })?;
    let timeout = if interactive { 150 } else { 15 };
    let deadline = Instant::now() + Duration::from_secs(timeout);
    while Instant::now() < deadline {
        if let Some(value) = take_result()? {
            if let Some(token) = value.strip_prefix('+') {
                return Ok(token.to_owned());
            }
            return Err(match value.as_str() {
                "-cancelled" => "Google Drive authorization was cancelled.",
                "-expired" => "Google Drive access expired. Sign in again from Settings.",
                _ => "Google Play Services could not authorize Drive. Check the Android OAuth client registration.",
            });
        }
        thread::sleep(Duration::from_millis(150));
    }
    Err("Google Drive authorization timed out. Try again from Settings.")
}

fn take_result() -> AuthResult<Option<String>> {
    with_activity(|env, activity| {
        let result = env
            .call_method(
                activity,
                jni_str!("takeDriveAuthorizationResult"),
                jni_sig!("()Ljava/lang/String;"),
                &[],
            )?
            .l()?;
        if result.is_null() {
            return Ok(None);
        }
        env.cast_local::<JString>(result)?
            .try_to_string(env)
            .map(Some)
    })
}

fn with_activity<T>(
    call: impl for<'local> FnOnce(&mut Env<'local>, &JObject<'local>) -> jni::errors::Result<T>,
) -> AuthResult<T> {
    let context = ndk_context::android_context();
    if context.vm().is_null() || context.context().is_null() {
        return Err(UNAVAILABLE);
    }
    // The Android activity has installed its JavaVM and global Activity reference.
    let vm = unsafe { JavaVM::from_raw(context.vm().cast()) };
    let activity: jni::sys::jobject = context.context().cast();
    vm.attach_current_thread(|env| {
        let reference = unsafe { env.as_cast_raw::<Global<JObject<'static>>>(&activity) }?;
        let local = env.new_local_ref(reference.as_ref())?;
        call(env, &local)
    })
    .map_err(|_| UNAVAILABLE)
}
