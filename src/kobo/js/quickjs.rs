use quickjs_runtime::{builder::QuickJsRuntimeBuilder, jsutils::Script, values::JsValueFacade};

pub fn extract_href(code: &str) -> Option<String> {
    let rt = QuickJsRuntimeBuilder::new().build();
    match rt.eval_sync(None, Script::new("<main>", code)).ok()? {
        JsValueFacade::String { val } => Some(val.to_string()),
        _ => None,
    }
}
