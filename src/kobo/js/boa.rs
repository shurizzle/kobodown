use boa_engine::{value::JsValue, Context, Source};

pub fn extract_href(code: &str) -> Option<String> {
    let mut ctx = Context::default();
    match ctx.eval(Source::from_bytes(&code)).ok()? {
        JsValue::String(s) => Some(s.to_std_string_lossy()),
        _ => None,
    }
}
