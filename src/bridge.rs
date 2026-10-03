use serde_json::{Value, json};

#[cfg(target_os = "android")]
fn with_env<T>(
    f: impl FnOnce(&mut jni::JNIEnv, &jni::objects::JObject) -> jni::errors::Result<T>,
) -> Option<T> {
    egui_mobile::with_native_activity(f)
}

pub fn send(value: Value) {
    #[cfg(target_os = "android")]
    with_env(|env, activity| {
        let text = env.new_string(value.to_string())?;
        env.call_method(
            activity,
            "galleryCommand",
            "(Ljava/lang/String;)V",
            &[(&text).into()],
        )?;
        Ok(())
    });
    #[cfg(not(target_os = "android"))]
    let _ = value;
}

#[cfg(target_os = "android")]
pub fn attach_surface(surface: &jni::objects::GlobalRef, width: u32, height: u32) {
    with_env(|env, activity| {
        env.call_method(
            activity,
            "galleryAttachVideoSurface",
            "(Landroid/view/Surface;II)V",
            &[
                surface.as_obj().into(),
                (width as i32).into(),
                (height as i32).into(),
            ],
        )?;
        Ok(())
    });
}

pub fn poll() -> Value {
    #[cfg(target_os = "android")]
    if let Some(text) = with_env(|env, activity| {
        let value = env
            .call_method(activity, "galleryPoll", "()Ljava/lang/String;", &[])?
            .l()?;
        let text: String = env.get_string(&jni::objects::JString::from(value))?.into();
        Ok(text)
    }) {
        return serde_json::from_str(&text).unwrap_or_else(|_| json!({}));
    }
    json!({})
}
