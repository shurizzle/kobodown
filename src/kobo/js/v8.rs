use mini_v8::MiniV8;

pub fn extract_href(code: &str) -> Option<String> {
    let mv8 = MiniV8::new();
    mv8.eval::<_, String>(code).ok()
}
